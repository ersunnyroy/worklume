//! The Linux tray and a launch at login, end to end (E12-S05): the real app against a stand-in
//! `StatusNotifierWatcher`, the service stock Ubuntu's `AppIndicator` extension provides. It needs
//! an X server and a D-Bus session: run with `--ignored` under `xvfb-run -a dbus-run-session`,
//! as CI does on Linux.

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "the test fails loudly when its environment is missing"
)]

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::MatchRule;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, MapState};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WAIT: Duration = Duration::from_secs(30);

/// The stand-in watcher: owns the name, says a host is there, and records registrations as
/// (sender, service).
struct Watcher {
    items: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    fn start() -> Self {
        let items = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn({
            let items = Arc::clone(&items);
            let stop = Arc::clone(&stop);
            move || {
                let conn = Connection::new_session().expect("a session bus");
                conn.request_name(WATCHER, false, true, true)
                    .expect("the name is free");
                conn.start_receive(
                    MatchRule::new_method_call(),
                    Box::new(move |call, conn| {
                        let member = call.member().map(|member| member.to_string());
                        let reply = match member.as_deref() {
                            Some("RegisterStatusNotifierItem") => {
                                let service: String = call.read1().unwrap_or_default();
                                let sender = call
                                    .sender()
                                    .map(|sender| sender.to_string())
                                    .unwrap_or_default();
                                items
                                    .lock()
                                    .unwrap_or_else(PoisonError::into_inner)
                                    .push((sender, service));
                                call.method_return()
                            }
                            Some("RegisterStatusNotifierHost") => call.method_return(),
                            Some("Get") => {
                                let (_, property): (String, String) =
                                    call.read2().unwrap_or_default();
                                match property.as_str() {
                                    "IsStatusNotifierHostRegistered" => {
                                        call.method_return().append1(Variant(true))
                                    }
                                    "RegisteredStatusNotifierItems" => {
                                        call.method_return().append1(Variant(Vec::<String>::new()))
                                    }
                                    _ => call.method_return().append1(Variant(0_i32)),
                                }
                            }
                            Some("GetAll") => {
                                let mut all = PropMap::new();
                                all.insert(
                                    "IsStatusNotifierHostRegistered".to_owned(),
                                    Variant(Box::new(true)),
                                );
                                all.insert("ProtocolVersion".to_owned(), Variant(Box::new(0_i32)));
                                call.method_return().append1(all)
                            }
                            _ => call.error(
                                &"org.freedesktop.DBus.Error.UnknownMethod".into(),
                                c"no such method",
                            ),
                        };
                        conn.send(reply).ok();
                        true
                    }),
                );
                ready_tx.send(()).expect("the test waits");
                while !stop.load(Ordering::SeqCst) {
                    conn.process(Duration::from_millis(50)).ok();
                }
            }
        });
        ready_rx.recv().expect("the watcher started");
        Self {
            items,
            stop,
            thread: Some(thread),
        }
    }

    /// The first item registered, as (bus name, object path), waiting for it.
    fn first_item(&self) -> (String, String) {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some((sender, service)) = self
                .items
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .first()
                .cloned()
            {
                // An item registers its object path, or a bus name at the standard path.
                return if service.starts_with('/') {
                    (sender, service)
                } else {
                    (service, "/StatusNotifierItem".to_owned())
                };
            }
            assert!(Instant::now() < deadline, "no tray item registered");
            sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

/// The app with its own home, config, data, cache and runtime folders.
struct Home {
    root: tempfile::TempDir,
}

impl Home {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("a temporary folder"),
        }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.root.path().join(name);
        std::fs::create_dir_all(&dir).expect("a folder");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("mode");
        dir
    }

    fn launch(&self, args: &[&str]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_worklume-desktop"))
            .args(args)
            .env("HOME", self.dir("home"))
            .env("XDG_CONFIG_HOME", self.dir("config"))
            .env("XDG_DATA_HOME", self.dir("data"))
            .env("XDG_CACHE_HOME", self.dir("cache"))
            .env("XDG_RUNTIME_DIR", self.dir("runtime"))
            .spawn()
            .expect("the app starts")
    }
}

fn wait_for_exit(child: &mut Child, within: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("the process") {
            return Some(status);
        }
        sleep(Duration::from_millis(100));
    }
    None
}

/// Whether a top-level window titled "Worklume" is on screen.
fn window_shown() -> bool {
    let (conn, screen) = x11rb::connect(None).expect("an X server");
    let root = conn.setup().roots[screen].root;
    let tree = conn
        .query_tree(root)
        .expect("request")
        .reply()
        .expect("reply");
    tree.children.iter().any(|&window| {
        let viewable = conn
            .get_window_attributes(window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|attributes| attributes.map_state == MapState::VIEWABLE);
        let titled = conn
            .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::ANY, 0, 64)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.value == b"Worklume");
        viewable && titled
    })
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting: {what}");
        sleep(Duration::from_millis(100));
    }
}

/// A `com.canonical.dbusmenu` layout node: ID, properties and children.
type Layout = (i32, PropMap, Vec<Variant<Box<dyn RefArg>>>);

/// The menu's items as (id, label).
fn menu_items(conn: &Connection, name: &str, menu: &str) -> Vec<(i32, String)> {
    let proxy = conn.with_proxy(name, menu, Duration::from_secs(5));
    let (_, (_, _, children)): (u32, Layout) = proxy
        .method_call(
            "com.canonical.dbusmenu",
            "GetLayout",
            (0_i32, -1_i32, Vec::<String>::new()),
        )
        .expect("the menu's layout");
    children
        .iter()
        .filter_map(|child| {
            let mut fields = child.0.as_iter()?;
            let id = i32::try_from(fields.next()?.as_i64()?).ok()?;
            let properties = fields.next()?;
            let mut pairs = properties.as_iter()?;
            let mut label = None;
            while let (Some(key), Some(value)) = (pairs.next(), pairs.next()) {
                if key.as_str() == Some("label") {
                    label = value.as_str().map(str::to_owned);
                }
            }
            Some((id, label?))
        })
        .collect()
}

fn click(conn: &Connection, name: &str, menu: &str, id: i32) {
    let proxy = conn.with_proxy(name, menu, Duration::from_secs(5));
    let () = proxy
        .method_call(
            "com.canonical.dbusmenu",
            "Event",
            (id, "clicked", Variant(String::new()), 0_u32),
        )
        .expect("the click is delivered");
}

fn string(properties: &HashMap<String, Variant<Box<dyn RefArg>>>, key: &str) -> String {
    properties
        .get(key)
        .and_then(|value| value.0.as_str())
        .unwrap_or_default()
        .to_owned()
}

#[test]
#[ignore = "needs an X server and a D-Bus session: run under xvfb-run -a dbus-run-session"]
fn the_tray_shows_on_a_status_notifier_host_and_a_login_launch_stays_hidden() {
    let watcher = Watcher::start();
    let home = Home::new();
    let mut app = home.launch(&["--autostart"]);
    let (name, path) = watcher.first_item();
    let conn = Connection::new_session().expect("a session bus");

    // The item a panel draws: active, with an icon and a menu.
    let item = conn.with_proxy(name.as_str(), path.as_str(), Duration::from_secs(5));
    let properties = item
        .get_all("org.kde.StatusNotifierItem")
        .expect("the item's properties");
    assert_eq!(string(&properties, "Status"), "Active");
    let icon = PathBuf::from(string(&properties, "IconName"));
    assert!(icon.is_file(), "{icon:?}");
    // In the member's private runtime folder, never a shared /tmp one.
    let private = home.dir("runtime").join("com.worklume.agent/tray");
    assert_eq!(icon.parent(), Some(private.as_path()));
    let mode = std::fs::metadata(&private)
        .expect("the folder")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    let menu = string(&properties, "Menu");
    let items = menu_items(&conn, &name, &menu);
    let labels: Vec<&str> = items.iter().map(|(_, label)| label.as_str()).collect();
    assert_eq!(labels, ["Open Worklume", "Quit Worklume"]);

    // Started at login: nothing on screen, and a second launch at login shows nothing either.
    sleep(Duration::from_secs(1));
    assert!(!window_shown());
    let mut again = home.launch(&["--autostart"]);
    let status = wait_for_exit(&mut again, WAIT).expect("the second launch exits");
    assert!(status.success());
    sleep(Duration::from_secs(1));
    assert!(!window_shown());

    // The tray's menu opens the window and quits the app.
    let id = |label: &str| {
        items
            .iter()
            .find(|(_, item)| item == label)
            .map(|(id, _)| *id)
            .expect("the item")
    };
    click(&conn, &name, &menu, id("Open Worklume"));
    wait_until("the window shows", window_shown);
    click(&conn, &name, &menu, id("Quit Worklume"));
    let status = wait_for_exit(&mut app, WAIT).expect("the app quits");
    assert!(status.success());
}
