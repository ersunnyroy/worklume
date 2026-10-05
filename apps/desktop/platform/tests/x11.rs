//! The X11 adapters (E12-S03) against a real X server, with input faked through XTEST. They
//! need an X server: run with `--ignored` under `xvfb-run`, as CI does on Linux.

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    reason = "a test fails loudly when its X server is missing"
)]

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use std::path::PathBuf;

use worklume_platform::fake::MemorySecretStore;
use worklume_platform::linux::{X11Capture, X11Idle, X11Input, X11Window};
use worklume_platform::{
    ActiveWindow, CapabilityReason, CapabilityStatus, Clock, DeviceSensors, Feature, ForegroundApp,
    Frame, IdleSource, InputActivity, InputEvent, InputKind, Monitors, OsClock, ReportedPlatform,
    ScreenCapture, SecretStore, SensorError,
};
use x11rb::COPY_DEPTH_FROM_PARENT;
use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, CreateWindowAux,
    GrabMode, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT, PropMode, Window,
    WindowClass,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Tests share one X server, and each other's input would disturb them.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
/// Held for the whole run: Xvfb resets when its last client leaves, and refuses connections
/// while it does.
static KEEP_ALIVE: OnceLock<RustConnection> = OnceLock::new();

fn serial() -> MutexGuard<'static, ()> {
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    KEEP_ALIVE.get_or_init(|| x11rb::connect(None).expect("an X server").0);
    guard
}

/// Fakes input as if from the keyboard and mouse.
struct Hands {
    conn: RustConnection,
    root: Window,
}

/// The `a` key in the default keymap.
const KEY_A: u8 = 38;

impl Hands {
    fn new() -> Self {
        let (conn, screen) = x11rb::connect(None).expect("an X server");
        let root = conn.setup().roots[screen].root;
        Self { conn, root }
    }

    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) {
        self.conn
            .xtest_fake_input(kind, detail, 0, self.root, x, y, 0)
            .expect("XTEST");
    }

    fn key(&self) {
        self.fake(KEY_PRESS_EVENT, KEY_A, 0, 0);
        self.fake(KEY_RELEASE_EVENT, KEY_A, 0, 0);
    }

    fn button(&self, button: u8) {
        self.fake(BUTTON_PRESS_EVENT, button, 0, 0);
        self.fake(BUTTON_RELEASE_EVENT, button, 0, 0);
    }

    /// A relative move: absolute XTEST motion produces no raw motion.
    fn nudge(&self) {
        self.fake(MOTION_NOTIFY_EVENT, 1, 3, 2);
    }

    fn atom(&self, name: &str) -> u32 {
        self.conn
            .intern_atom(false, name.as_bytes())
            .expect("request")
            .reply()
            .expect("reply")
            .atom
    }

    /// A window with `WM_CLASS` (instance and class), as an application maps one.
    fn window(&self, class: Option<(&str, &str)>) -> Window {
        let window = self.conn.generate_id().expect("an ID");
        self.conn
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                self.root,
                0,
                0,
                10,
                10,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new(),
            )
            .expect("request");
        if let Some((instance, class)) = class {
            let value = format!("{instance}\0{class}\0");
            self.set(
                window,
                AtomEnum::WM_CLASS.into(),
                AtomEnum::STRING.into(),
                value.as_bytes(),
            );
        }
        window
    }

    fn set(&self, window: Window, property: u32, kind: u32, value: &[u8]) {
        self.conn
            .change_property8(PropMode::REPLACE, window, property, kind, value)
            .expect("request");
    }

    fn set_utf8(&self, window: Window, property: &str, value: &str) {
        let (property, kind) = (self.atom(property), self.atom("UTF8_STRING"));
        self.set(window, property, kind, value.as_bytes());
    }

    /// Acts as the window manager: puts `window` in front, 0 for nothing.
    fn activate(&self, window: Window) {
        let active = self.atom("_NET_ACTIVE_WINDOW");
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                active,
                AtomEnum::WINDOW,
                &[window],
            )
            .expect("request");
        self.sync();
    }

    /// Acts as a window manager without EWMH.
    fn no_window_manager(&self) {
        for property in ["_NET_ACTIVE_WINDOW", "_NET_SUPPORTED"] {
            let atom = self.atom(property);
            self.conn.delete_property(self.root, atom).expect("request");
        }
        self.sync();
    }

    /// Acts as an EWMH window manager that hasn't activated a window yet.
    fn fresh_window_manager(&self) {
        let (active, supported) = (self.atom("_NET_ACTIVE_WINDOW"), self.atom("_NET_SUPPORTED"));
        self.conn
            .delete_property(self.root, active)
            .expect("request");
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.root,
                supported,
                AtomEnum::ATOM,
                &[active],
            )
            .expect("request");
        self.sync();
    }

    /// A `RandR` monitor over part of the screen, as `xrandr --setmonitor` defines one.
    fn monitor(&self, name: &str, x: i16, y: i16, width: u16, height: u16) -> u32 {
        let name = self.atom(name);
        let info = randr::MonitorInfo {
            name,
            primary: false,
            automatic: false,
            x,
            y,
            width,
            height,
            width_in_millimeters: u32::from(width) / 4,
            height_in_millimeters: u32::from(height) / 4,
            outputs: Vec::new(),
        };
        self.conn
            .randr_set_monitor(self.root, info)
            .expect("request")
            .check()
            .expect("the monitor is set");
        name
    }

    fn remove_monitor(&self, name: u32) {
        self.conn
            .randr_delete_monitor(self.root, name)
            .expect("request")
            .check()
            .expect("the monitor is removed");
    }

    /// A borderless window filling a rectangle with `pixel`, on top of everything.
    fn paint(&self, x: i16, y: i16, width: u16, height: u16, pixel: u32) -> Window {
        let window = self.conn.generate_id().expect("an ID");
        self.conn
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                self.root,
                x,
                y,
                width,
                height,
                0,
                WindowClass::INPUT_OUTPUT,
                0,
                &CreateWindowAux::new()
                    .background_pixel(pixel)
                    .override_redirect(1),
            )
            .expect("request");
        self.conn.map_window(window).expect("request");
        window
    }

    /// Moves the pointer to `(x, y)` on the screen.
    fn point_at(&self, x: i16, y: i16) {
        self.fake(MOTION_NOTIFY_EVENT, 0, x, y);
        self.sync();
    }

    /// Waits until the server has handled everything sent so far.
    fn sync(&self) {
        self.conn
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("reply");
    }
}

fn count(events: &[InputEvent], kind: InputKind) -> usize {
    events.iter().filter(|event| event.kind == kind).count()
}

/// Drains until `expected` events arrived or a second passed.
fn drain_until(input: &X11Input, expected: usize) -> Vec<InputEvent> {
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut events = Vec::new();
    while events.len() < expected && Instant::now() < deadline {
        events.extend(input.drain().expect("the reader works"));
        sleep(Duration::from_millis(20));
    }
    sleep(Duration::from_millis(100));
    events.extend(input.drain().expect("the reader works"));
    events
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn idle_time_grows_until_the_next_input() {
    let _one = serial();
    let hands = Hands::new();
    let idle = X11Idle::connect().expect("the screen saver extension");
    hands.key();
    hands.sync();
    sleep(Duration::from_millis(400));
    let away = idle.idle_for().expect("a reading");
    assert!(away >= Duration::from_millis(300), "{away:?}");
    hands.nudge();
    hands.sync();
    let back = idle.idle_for().expect("a reading");
    assert!(back < Duration::from_millis(200), "{back:?}");
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn input_arrives_with_its_kind_and_the_agents_time() {
    let _one = serial();
    let hands = Hands::new();
    let clock = Arc::new(OsClock::new().expect("a clock"));
    let input = X11Input::start(Arc::clone(&clock) as _).expect("XInput 2.2");
    drop(input.drain());
    let before = clock.read().expect("a reading").mono_ms;
    for _ in 0..3 {
        hands.key();
    }
    hands.button(1);
    hands.button(3);
    hands.button(4);
    hands.button(5);
    for _ in 0..4 {
        hands.nudge();
    }
    hands.sync();
    let events = drain_until(&input, 11);
    let after = clock.read().expect("a reading").mono_ms;

    assert_eq!(count(&events, InputKind::Key), 3, "{events:?}");
    assert_eq!(count(&events, InputKind::Click), 2, "{events:?}");
    assert_eq!(count(&events, InputKind::Scroll), 2, "{events:?}");
    assert_eq!(count(&events, InputKind::Pointer), 4, "{events:?}");
    assert!(
        events
            .iter()
            .all(|event| (before..=after).contains(&event.mono_ms)),
        "stamped with the agent's clock: {before}..={after}, {events:?}"
    );
    // Oldest first.
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].mono_ms <= pair[1].mono_ms)
    );
    // Drained events are gone.
    assert_eq!(input.drain().expect("the reader works"), []);
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn input_still_arrives_while_a_locker_holds_the_keyboard() {
    // Why the timekeeper drops input from locked time: X11 doesn't hold it back.
    let _one = serial();
    let hands = Hands::new();
    let clock = Arc::new(OsClock::new().expect("a clock"));
    let input = X11Input::start(clock as _).expect("XInput 2.2");
    let (locker, screen) = x11rb::connect(None).expect("an X server");
    let root = locker.setup().roots[screen].root;
    let grab = locker
        .grab_keyboard(
            false,
            root,
            x11rb::CURRENT_TIME,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )
        .expect("request")
        .reply()
        .expect("reply");
    assert_eq!(grab.status, x11rb::protocol::xproto::GrabStatus::SUCCESS);
    drop(input.drain());
    for _ in 0..5 {
        hands.key();
    }
    hands.sync();
    let events = drain_until(&input, 5);
    assert_eq!(count(&events, InputKind::Key), 5);
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn the_reader_stops_promptly_when_dropped() {
    let _one = serial();
    let clock = Arc::new(OsClock::new().expect("a clock"));
    let input = X11Input::start(clock as _).expect("XInput 2.2");
    let started = Instant::now();
    drop(input);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn an_x11_session_gets_the_x11_adapters() {
    let _one = serial();
    let hands = Hands::new();
    let clock: Arc<dyn Clock> = Arc::new(OsClock::new().expect("a clock"));
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
    let sensors = DeviceSensors::start(&clock, &secrets);
    assert!(sensors.problems.is_empty(), "{:?}", sensors.problems);
    assert!(sensors.input_hooks);
    hands.activate(hands.window(Some(("probe", "ProbeApp"))));
    assert!(sensors.window.foreground(false).is_ok());
    assert!(!sensors.capture.monitors().expect("the monitors").is_empty());
    assert!(sensors.idle.idle_for().is_ok());
    drop(sensors.input.drain());
    hands.key();
    hands.sync();
    sleep(Duration::from_millis(200));
    let events = sensors.input.drain().expect("the reader works");
    assert_eq!(count(&events, InputKind::Key), 1);

    // The report: everything an X11 session offers, nothing claimed for browser addresses.
    let report = sensors.report();
    assert_eq!(report.platform, ReportedPlatform::LinuxX11);
    for feature in [
        Feature::Screenshots,
        Feature::Activity,
        Feature::Idle,
        Feature::Apps,
    ] {
        assert_eq!(
            report.get(feature).status(),
            CapabilityStatus::Granted,
            "{report}"
        );
    }
    assert_eq!(
        report.get(Feature::Urls).reasons(),
        [CapabilityReason::NotImplemented]
    );
    // The lock comes from logind or a screen saver, which a test bus may lack: the report
    // follows the reading.
    let lock = report.get(Feature::SessionLock);
    let expected = match sensors.lock.locked() {
        Ok(_) => (CapabilityStatus::Granted, &[][..]),
        Err(SensorError::Unsupported) => (
            CapabilityStatus::Unsupported,
            &[CapabilityReason::DesktopUnsupported][..],
        ),
        Err(_) => (
            CapabilityStatus::Failed,
            &[CapabilityReason::AdapterFailed][..],
        ),
    };
    assert_eq!((lock.status(), lock.reasons()), expected, "{report}");
    // Reading for the report took nothing from the timekeeper's input.
    hands.key();
    hands.sync();
    sleep(Duration::from_millis(200));
    drop(sensors.report());
    let events = sensors.input.drain().expect("the reader works");
    assert_eq!(count(&events, InputKind::Key), 1);
}

/// Applications folders holding `entries`, in a fresh temporary folder.
fn applications(test: &str, entries: &[(&str, &str)]) -> Vec<PathBuf> {
    let dir = std::env::temp_dir().join(format!("worklume-x11-{test}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("a folder");
    for (name, text) in entries {
        std::fs::write(dir.join(name), text).expect("an entry");
    }
    vec![dir]
}

const PROBE: &str =
    "[Desktop Entry]\nType=Application\nName=Probe App\nExec=probe\nStartupWMClass=ProbeApp\n";

fn front(reader: &X11Window, with_title: bool) -> ForegroundApp {
    reader
        .foreground(with_title)
        .expect("a reading")
        .expect("something in front")
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn names_the_app_in_front_and_reads_its_title_only_when_asked() {
    let _one = serial();
    let hands = Hands::new();
    let reader =
        X11Window::connect_with(applications("names", &[("worklume-probe.desktop", PROBE)]))
            .expect("an X server");
    // Chromium puts its profile folder in the instance part; none of it may come out.
    let window = hands.window(Some(("probe (/home/member/secret-profile)", "ProbeApp")));
    hands.set_utf8(window, "_NET_WM_NAME", "Q3 report — draft");
    hands.activate(window);

    let app = front(&reader, false);
    assert_eq!(app.identifier, "worklume-probe");
    assert_eq!(app.name, "Probe App");
    assert_eq!(app.title, None);
    let titled = front(&reader, true);
    assert_eq!(titled.title.as_deref(), Some("Q3 report — draft"));
    assert!(!format!("{app:?}{titled:?}").contains("secret-profile"));
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn without_an_entry_the_class_names_the_app() {
    let _one = serial();
    let hands = Hands::new();
    let reader = X11Window::connect_with(applications("class", &[])).expect("an X server");
    let window = hands.window(Some(("unlisted", "Unlisted")));
    // `WM_NAME` as `STRING` is Latin-1.
    hands.set(
        window,
        AtomEnum::WM_NAME.into(),
        AtomEnum::STRING.into(),
        b"Caf\xe9",
    );
    hands.activate(window);
    let app = front(&reader, true);
    assert_eq!(app.identifier, "unlisted");
    assert_eq!(app.name, "Unlisted");
    assert_eq!(app.title.as_deref(), Some("Café"));
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn the_toolkit_app_id_and_the_executable_name_apps() {
    let _one = serial();
    let hands = Hands::new();
    let exe = std::env::current_exe().expect("this test's executable");
    let exec = format!(
        "[Desktop Entry]\nType=Application\nName=Runner\nExec=\"{}\" --flag\n",
        exe.display()
    );
    let reader = X11Window::connect_with(applications(
        "ids",
        &[
            ("worklume-probe.desktop", PROBE),
            (
                "org.example.Notes.desktop",
                "[Desktop Entry]\nType=Application\nName=Notes\nExec=notes\n",
            ),
            ("runner.desktop", &exec),
        ],
    ))
    .expect("an X server");

    // The toolkit's ID beats the class another entry claims.
    let notes = hands.window(Some(("notes", "ProbeApp")));
    hands.set_utf8(notes, "_GTK_APPLICATION_ID", "org.example.Notes");
    hands.activate(notes);
    assert_eq!(front(&reader, false).identifier, "org.example.Notes");

    // No class: the executable, found through X-Resource, names the app. This test's own
    // process made the window.
    let bare = hands.window(None);
    hands.activate(bare);
    let app = front(&reader, false);
    assert_eq!(
        (app.identifier.as_str(), app.name.as_str()),
        ("runner", "Runner")
    );
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn a_window_that_changes_class_is_named_again() {
    // The server reuses closed windows' IDs, so the app is never taken from the cache alone.
    let _one = serial();
    let hands = Hands::new();
    let reader =
        X11Window::connect_with(applications("reuse", &[("worklume-probe.desktop", PROBE)]))
            .expect("an X server");
    let window = hands.window(Some(("probe", "ProbeApp")));
    hands.activate(window);
    assert_eq!(front(&reader, false).identifier, "worklume-probe");
    hands.set(
        window,
        AtomEnum::WM_CLASS.into(),
        AtomEnum::STRING.into(),
        b"other\0Other\0",
    );
    hands.sync();
    assert_eq!(front(&reader, false).identifier, "other");
}

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn nothing_in_front_differs_from_no_window_manager() {
    let _one = serial();
    let hands = Hands::new();
    let reader = X11Window::connect_with(applications("empty", &[])).expect("an X server");
    hands.activate(0);
    assert_eq!(reader.foreground(true), Ok(None));
    hands.fresh_window_manager();
    assert_eq!(reader.foreground(true), Ok(None));
    hands.no_window_manager();
    assert_eq!(reader.foreground(true), Err(SensorError::Unsupported));
}

/// The pixel at `(x, y)` of a frame, as RGBA.
fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
    let at = usize::try_from((y * frame.monitor.width + x) * 4).expect("an index");
    let bytes = frame.rgba.get(at..at + 4).expect("inside the frame");
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

const RED: [u8; 4] = [0xff, 0, 0, 0xff];
const BLUE: [u8; 4] = [0, 0, 0xff, 0xff];

#[test]
#[ignore = "needs an X server: run under xvfb-run with --ignored"]
fn captures_two_monitors_of_different_sizes() {
    let _one = serial();
    let hands = Hands::new();
    let root = hands
        .conn
        .get_geometry(hands.root)
        .expect("request")
        .reply()
        .expect("reply");
    assert!(root.width >= 1280 && root.height >= 1024, "{root:?}");
    // Two monitors of different sizes over the screen, each showing a colour of its own.
    let left = hands.monitor("WORKLUME-LEFT", 0, 0, 800, 600);
    let right = hands.monitor("WORKLUME-RIGHT", 800, 0, 480, 1024);
    let red = hands.paint(0, 0, 800, 600, 0x00ff_0000);
    let blue = hands.paint(800, 0, 480, 1024, 0x0000_00ff);
    hands.sync();
    sleep(Duration::from_millis(100));

    let capture = X11Capture::connect().expect("a 32-bit TrueColor screen");
    // The screen-wide monitor Xvfb keeps spans both, so it is left out.
    let mut names: Vec<String> = capture
        .monitors()
        .expect("the monitors")
        .into_iter()
        .map(|monitor| monitor.name)
        .collect();
    names.sort();
    assert_eq!(names, ["WORKLUME-LEFT", "WORKLUME-RIGHT"]);

    let mut frames = capture.capture(Monitors::All).expect("frames");
    frames.sort_by(|a, b| a.monitor.name.cmp(&b.monitor.name));
    let [left_frame, right_frame] = frames.as_slice() else {
        panic!("two frames: {frames:?}");
    };
    assert_eq!(
        (left_frame.monitor.width, left_frame.monitor.height),
        (800, 600)
    );
    assert_eq!(left_frame.rgba.len(), 800 * 600 * 4);
    assert_eq!(
        (right_frame.monitor.width, right_frame.monitor.height),
        (480, 1024)
    );
    assert_eq!(right_frame.rgba.len(), 480 * 1024 * 4);
    for (x, y) in [(0, 0), (799, 0), (400, 300), (0, 599), (799, 599)] {
        assert_eq!(pixel(left_frame, x, y), RED, "left at {x},{y}");
    }
    for (x, y) in [(0, 0), (479, 0), (240, 512), (0, 1023), (479, 1023)] {
        assert_eq!(pixel(right_frame, x, y), BLUE, "right at {x},{y}");
    }
    // A frame's Debug never shows pixels.
    assert!(format!("{left_frame:?}").len() < 400);

    // The active monitor is the one under the pointer.
    hands.point_at(1000, 900);
    let active = capture.capture(Monitors::Active).expect("a frame");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].monitor.name, "WORKLUME-RIGHT");
    hands.point_at(100, 100);
    let active = capture.capture(Monitors::Active).expect("a frame");
    assert_eq!(active[0].monitor.name, "WORKLUME-LEFT");
    assert_eq!(pixel(&active[0], 10, 10), RED);

    for window in [red, blue] {
        hands.conn.destroy_window(window).expect("request");
    }
    hands.remove_monitor(left);
    hands.remove_monitor(right);
    hands.sync();
}
