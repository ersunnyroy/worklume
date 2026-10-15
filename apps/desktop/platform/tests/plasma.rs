//! The KDE window adapter (E13-S06) against a real KWin: version 5.27, on its virtual backend,
//! which each test starts with folders of its own. The windows are the test's own
//! (`xdg_toplevel` with a shared-memory buffer). KWin grants Plasma window management through a
//! desktop entry naming the test binary, as the packages' entry names the agent. They need
//! `kwin_wayland` and a session bus of their own: run with `--ignored` under `dbus-run-session`,
//! as CI does on Linux.

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly when its compositor is missing"
)]

use std::collections::HashSet;
use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use wayland_client::globals::{GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
use worklume_platform::fake::MemorySecretStore;
use worklume_platform::linux::PlasmaWindow;
use worklume_platform::{
    ActiveWindow, CapabilityStatus, Clock, DeviceSensors, Feature, ForegroundApp, OsClock,
    ReportedPlatform, SecretStore, SensorError,
};

/// Tests share the session bus, where KWin takes its name, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A KWin on its virtual backend, stopped when dropped.
struct KWin {
    child: Child,
    dir: TempDir,
}

impl KWin {
    /// Starts KWin; `granted` installs a desktop entry granting this test binary Plasma window
    /// management, as the packages do for the agent.
    fn start(granted: bool) -> Self {
        // A short path: the socket's name must fit in a socket address.
        let dir = tempfile::Builder::new()
            .prefix("kw")
            .tempdir_in("/tmp")
            .expect("a runtime folder");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("a private runtime folder");
        let applications = dir.path().join("data/applications");
        for folder in [
            &applications,
            &dir.path().join("config"),
            &dir.path().join("cache"),
        ] {
            std::fs::create_dir_all(folder).expect("a folder");
        }
        if granted {
            let exe = std::env::current_exe().expect("the test binary");
            std::fs::write(
                applications.join("com.worklume.tests.desktop"),
                format!(
                    "[Desktop Entry]\nType=Application\nName=Worklume tests\nExec={}\n\
                     NoDisplay=true\nX-KDE-Wayland-Interfaces=org_kde_plasma_window_management\n",
                    exe.display()
                ),
            )
            .expect("the grant");
        }
        // The installed binary has file capabilities, which an unprivileged start refuses; a
        // copy has none. It must keep its name, which KWin's platform plugin checks.
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).expect("a folder");
        std::fs::copy(kwin_binary(), bin.join("kwin_wayland")).expect("a copy of KWin");
        let child = Command::new(bin.join("kwin_wayland"))
            .args([
                "--virtual",
                "--width",
                "640",
                "--height",
                "480",
                "--socket",
                "wayland-k",
                "--no-global-shortcuts",
            ])
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("XDG_DATA_HOME", dir.path().join("data"))
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .env("XDG_CACHE_HOME", dir.path().join("cache"))
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("KWin is installed");
        let kwin = Self { child, dir };
        let started = Instant::now();
        while UnixStream::connect(kwin.socket()).is_err() {
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "KWin didn't start"
            );
            sleep(Duration::from_millis(100));
        }
        kwin
    }

    fn socket(&self) -> PathBuf {
        self.dir.path().join("wayland-k")
    }
}

impl Drop for KWin {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

fn kwin_binary() -> PathBuf {
    ["/usr/bin/kwin_wayland", "/usr/local/bin/kwin_wayland"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
        .expect("kwin_wayland is installed")
}

/// An applications folder with an editor's entry.
fn entries() -> TempDir {
    let dir = TempDir::new().expect("a folder");
    std::fs::write(
        dir.path().join("org.example.Editor.desktop"),
        "[Desktop Entry]\nType=Application\nName=Example Editor\nExec=example-editor %F\n",
    )
    .expect("an entry");
    dir
}

/// The test's own Wayland client, which opens windows.
struct Client {
    queue: EventQueue<Configured>,
    globals: GlobalList,
    compositor: wl_compositor::WlCompositor,
    wm: xdg_wm_base::XdgWmBase,
}

/// The `xdg_surface`s configured so far.
#[derive(Default)]
struct Configured(HashSet<u32>);

/// A window of the test's.
struct Window {
    surface: wl_surface::WlSurface,
    xdg: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
    _buffer: (File, wl_buffer::WlBuffer),
}

impl Client {
    fn connect(kwin: &KWin) -> Self {
        let conn = Connection::from_socket(UnixStream::connect(kwin.socket()).expect("KWin"))
            .expect("a connection");
        let (globals, queue) = registry_queue_init::<Configured>(&conn).expect("the globals");
        let qh = queue.handle();
        let compositor = globals.bind(&qh, 4..=4, ()).expect("wl_compositor");
        let wm = globals.bind(&qh, 1..=2, ()).expect("xdg_wm_base");
        Self {
            queue,
            globals,
            compositor,
            wm,
        }
    }

    fn roundtrip(&mut self) {
        self.queue
            .roundtrip(&mut Configured::default())
            .expect("KWin answers");
    }

    /// Maps a window with `app_id` and `title`, which KWin then activates.
    fn open(&mut self, app_id: &str, title: &str) -> Window {
        let qh = self.queue.handle();
        let surface = self.compositor.create_surface(&qh, ());
        let xdg = self.wm.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg.get_toplevel(&qh, ());
        toplevel.set_app_id(app_id.to_owned());
        toplevel.set_title(title.to_owned());
        surface.commit();
        let mut configured = Configured::default();
        let started = Instant::now();
        while !configured.0.contains(&xdg.id().protocol_id()) {
            assert!(started.elapsed() < Duration::from_secs(5), "no configure");
            self.queue
                .blocking_dispatch(&mut configured)
                .expect("KWin answers");
        }
        let buffer = shm_buffer(&self.globals, &qh, 64, 64);
        surface.attach(Some(&buffer.1), 0, 0);
        surface.commit();
        self.roundtrip();
        Window {
            surface,
            xdg,
            toplevel,
            _buffer: buffer,
        }
    }

    fn retitle(&mut self, window: &Window, title: &str) {
        window.toplevel.set_title(title.to_owned());
        window.surface.commit();
        self.roundtrip();
    }

    fn close(&mut self, window: &Window) {
        window.toplevel.destroy();
        window.xdg.destroy();
        window.surface.destroy();
        self.roundtrip();
    }
}

fn shm_buffer(
    globals: &GlobalList,
    qh: &QueueHandle<Configured>,
    width: i32,
    height: i32,
) -> (File, wl_buffer::WlBuffer) {
    let shm: wl_shm::WlShm = globals.bind(qh, 1..=1, ()).expect("wl_shm");
    let stride = width * 4;
    let size = stride * height;
    let file = tempfile::tempfile().expect("a buffer file");
    file.set_len(u64::try_from(size).unwrap()).unwrap();
    file.write_all_at(&vec![0x80; usize::try_from(size).unwrap()], 0)
        .unwrap();
    let pool = shm.create_pool(file.as_fd(), size, qh, ());
    let buffer = pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
    pool.destroy();
    (file, buffer)
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Configured {
    fn event(
        _: &mut Self,
        wm: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for Configured {
    fn event(
        configured: &mut Self,
        xdg: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg.ack_configure(serial);
            configured.0.insert(xdg.id().protocol_id());
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Configured {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(Configured: ignore xdg_toplevel::XdgToplevel);
delegate_noop!(Configured: ignore wl_shm::WlShm);
delegate_noop!(Configured: ignore wl_surface::WlSurface);
delegate_noop!(Configured: ignore wl_buffer::WlBuffer);
delegate_noop!(Configured: wl_compositor::WlCompositor);
delegate_noop!(Configured: wl_shm_pool::WlShmPool);

fn reader(kwin: &KWin, entries: &Path) -> PlasmaWindow {
    PlasmaWindow::connect_to(&kwin.socket(), vec![entries.to_owned()])
        .expect("KWin grants Plasma window management")
}

fn app(identifier: &str, name: &str, title: Option<&str>) -> ForegroundApp {
    ForegroundApp {
        identifier: identifier.to_owned(),
        name: name.to_owned(),
        title: title.map(str::to_owned),
    }
}

/// Waits until `reader` reads `expected`, and returns the last reading.
fn wait_until(
    reader: &PlasmaWindow,
    with_title: bool,
    expected: Option<&ForegroundApp>,
) -> Result<Option<ForegroundApp>, SensorError> {
    let started = Instant::now();
    loop {
        let reading = reader.foreground(with_title);
        if reading.as_ref().ok().map(Option::as_ref) == Some(expected)
            || started.elapsed() > Duration::from_secs(5)
        {
            return reading;
        }
        sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "needs KWin and a session bus"]
fn the_app_in_front_follows_focus_with_its_title_only_when_asked() {
    let _turn = serial();
    let kwin = KWin::start(true);
    let dir = entries();
    let windows = reader(&kwin, dir.path());
    assert_eq!(windows.foreground(false), Ok(None));
    let mut client = Client::connect(&kwin);

    // An app with a desktop entry, named by it.
    let editor = client.open("org.example.Editor", "notes.txt");
    let named = app("org.example.Editor", "Example Editor", None);
    assert_eq!(wait_until(&windows, false, Some(&named)), Ok(Some(named)));
    assert_eq!(
        windows.foreground(true),
        Ok(Some(app(
            "org.example.Editor",
            "Example Editor",
            Some("notes.txt")
        )))
    );

    // Another app comes forward; without an entry it is named by its app ID.
    let terminal = client.open("Kitty", "~/projects");
    let kitty = app("kitty", "Kitty", None);
    assert_eq!(wait_until(&windows, false, Some(&kitty)), Ok(Some(kitty)));

    // A title changed behind it shows once the editor is in front again.
    client.retitle(&editor, "plan.md");
    client.close(&terminal);
    let back = app("org.example.Editor", "Example Editor", Some("plan.md"));
    assert_eq!(wait_until(&windows, true, Some(&back)), Ok(Some(back)));

    // Minimized, nothing else to activate: nothing in front.
    editor.toplevel.set_minimized();
    client.roundtrip();
    assert_eq!(wait_until(&windows, false, None), Ok(None));

    // Nothing in front once it closes.
    client.close(&editor);
    assert_eq!(wait_until(&windows, false, None), Ok(None));
}

#[test]
#[ignore = "needs KWin and a session bus"]
fn without_the_grant_kwin_withholds_the_protocol() {
    let _turn = serial();
    let kwin = KWin::start(false);
    let dir = entries();
    assert_eq!(
        PlasmaWindow::connect_to(&kwin.socket(), vec![dir.path().to_owned()]).map(drop),
        Err(SensorError::Unsupported)
    );
}

#[test]
#[ignore = "needs KWin and a session bus"]
fn a_compositor_that_goes_away_is_reported_and_dropping_the_reader_is_prompt() {
    let _turn = serial();
    let dir = entries();
    let mut kwin = KWin::start(true);
    let windows = reader(&kwin, dir.path());
    drop(kwin.child.kill());
    drop(kwin.child.wait());
    let started = Instant::now();
    while windows.foreground(false).is_ok() {
        assert!(started.elapsed() < Duration::from_secs(5), "still reading");
        sleep(Duration::from_millis(50));
    }
    assert!(matches!(
        windows.foreground(false),
        Err(SensorError::Failed(_))
    ));

    let kwin = KWin::start(true);
    let windows = reader(&kwin, dir.path());
    let dropped = Instant::now();
    drop(windows);
    assert!(dropped.elapsed() < Duration::from_secs(1));
}

/// Set for the child that prints its session's report.
const REPORT_CHILD: &str = "WORKLUME_TEST_REPORT_CHILD";

/// Prints the apps capability of the session it runs in; only in the child of the test below,
/// which sets up that session's variables.
#[test]
#[ignore = "run by a_kde_session_reports_the_app_in_front_and_a_missing_grant"]
fn print_this_sessions_report() {
    if std::env::var_os(REPORT_CHILD).is_none() {
        return;
    }
    let clock: Arc<dyn Clock> = Arc::new(OsClock::new().expect("a clock"));
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
    let sensors = DeviceSensors::start(&clock, &secrets);
    let report = sensors.report();
    let apps = report.get(Feature::Apps);
    let reasons: Vec<&str> = apps
        .reasons()
        .iter()
        .map(|reason| reason.as_str())
        .collect();
    println!(
        "REPORT {} {:?} apps:{}:{}",
        report.platform.as_str(),
        report.desktop,
        apps.status().as_str(),
        reasons.join(",")
    );
}

/// The report line of a child in a KDE session on `kwin`.
fn report_in(kwin: &KWin) -> String {
    let output = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "print_this_sessions_report",
            "--ignored",
            "--nocapture",
        ])
        .env(REPORT_CHILD, "1")
        .env("XDG_RUNTIME_DIR", kwin.dir.path())
        .env("WAYLAND_DISPLAY", "wayland-k")
        .env("XDG_SESSION_TYPE", "wayland")
        .env("XDG_CURRENT_DESKTOP", "KDE")
        .env_remove("DISPLAY")
        .output()
        .expect("the child ran");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("REPORT "))
        .unwrap_or_else(|| panic!("no report in {stdout}"))
        .to_owned()
}

#[test]
#[ignore = "needs KWin and a session bus"]
fn a_kde_session_reports_the_app_in_front_and_a_missing_grant() {
    let _turn = serial();
    let platform = ReportedPlatform::LinuxWayland.as_str();
    {
        let kwin = KWin::start(true);
        assert_eq!(
            report_in(&kwin),
            format!(
                "{platform} Some(Kde) apps:{}:",
                CapabilityStatus::Granted.as_str()
            )
        );
    }
    let kwin = KWin::start(false);
    assert_eq!(
        report_in(&kwin),
        format!("{platform} Some(Kde) apps:denied:permission_denied")
    );
}
