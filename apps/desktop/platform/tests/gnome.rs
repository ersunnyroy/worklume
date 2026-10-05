//! The GNOME window adapter (E13-S05) against stand-ins for the Worklume extension and GNOME
//! Shell's extensions service on the session bus. They need a session bus of their own: run
//! with `--ignored` under `dbus-run-session`, as CI does on Linux.
//!
//! The extension itself was checked by hand in GNOME Shell 46 (`docs/conventions/desktop.md`,
//! "GNOME active window").

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test fails loudly when its bus is missing"
)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use dbus::arg::{PropMap, Variant};
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message};
use tempfile::TempDir;
use worklume_platform::linux::{EXTENSION_UUID, ExtensionState, GnomeWindow};
use worklume_platform::{
    ActiveWindow, CapabilityProbe, CapabilityReason, CapabilityStatus, ForegroundApp, SensorError,
};

/// Tests share the bus names, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Default)]
struct State {
    /// What `Get` answers: Shell app ID, class, PID, title. `None`: nothing in front.
    front: Option<(String, String, u32, String)>,
    deny: bool,
    /// The Shell's state for the extension; `None` when it doesn't know it.
    extension: Option<f64>,
    asked_for_titles: u32,
    /// The extension starts answering this long after being turned on, as in GNOME Shell.
    starts_after_enable: Option<Duration>,
    /// When the stand-in takes the extension's name.
    start_at: Option<std::time::Instant>,
}

/// The Worklume extension and GNOME Shell's extensions service, on one connection.
struct Shell {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Shell {
    fn start(extension_running: bool) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn({
            let state = Arc::clone(&state);
            let state_for_loop = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            move || {
                let conn = Connection::new_session().expect("a session bus");
                conn.request_name("org.gnome.Shell", false, true, true)
                    .expect("the name is free");
                if extension_running {
                    conn.request_name("com.worklume.ActiveWindow", false, true, true)
                        .expect("the name is free");
                }
                conn.start_receive(
                    MatchRule::new_method_call(),
                    Box::new(move |call, conn| {
                        conn.send(answer(&call, &mut lock(&state))).ok();
                        true
                    }),
                );
                ready_tx.send(()).expect("the test waits");
                while !stop.load(Ordering::SeqCst) {
                    drop(conn.process(Duration::from_millis(20)));
                    let due = {
                        let mut state = lock(&state_for_loop);
                        let due = state
                            .start_at
                            .is_some_and(|at| at <= std::time::Instant::now());
                        if due {
                            state.start_at = None;
                        }
                        due
                    };
                    if due {
                        conn.request_name("com.worklume.ActiveWindow", false, true, true)
                            .expect("the name is free");
                    }
                }
            }
        });
        ready_rx.recv().expect("the stand-in started");
        Self {
            state,
            stop,
            thread: Some(thread),
        }
    }

    fn set(&self, change: impl FnOnce(&mut State)) {
        change(&mut lock(&self.state));
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn answer(call: &Message, state: &mut State) -> Message {
    let path = call.path().map(|path| path.to_string()).unwrap_or_default();
    match (path.as_str(), call.member().as_deref()) {
        ("/com/worklume/ActiveWindow", Some("Get")) => {
            if state.deny {
                return call.error(
                    &"org.freedesktop.DBus.Error.AccessDenied".into(),
                    c"Only the Worklume agent may ask",
                );
            }
            let with_title: bool = call.read1().unwrap();
            state.asked_for_titles += u32::from(with_title);
            let (id, class, pid, title) = state.front.clone().unwrap_or_default();
            let title = if with_title { title } else { String::new() };
            call.method_return().append3(id, class, pid).append1(title)
        }
        ("/org/gnome/Shell", Some("GetExtensionInfo")) => {
            let uuid: String = call.read1().unwrap();
            let mut info: PropMap = HashMap::new();
            if let (true, Some(extension)) = (uuid == EXTENSION_UUID, state.extension) {
                info.insert("state".to_owned(), Variant(Box::new(extension)));
            }
            call.method_return().append1(info)
        }
        ("/org/gnome/Shell", Some("EnableExtension")) => {
            let known = state.extension.is_some();
            if known {
                state.extension = Some(1.0);
                state.start_at = state
                    .starts_after_enable
                    .map(|delay| std::time::Instant::now() + delay);
            }
            call.method_return().append1(known)
        }
        _ => call.error(
            &"org.freedesktop.DBus.Error.UnknownMethod".into(),
            c"no such method",
        ),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An applications folder with an editor and an entry for this test's own executable.
fn entries() -> TempDir {
    let dir = TempDir::new().expect("a folder");
    let write = |name: &str, body: &str| {
        std::fs::write(dir.path().join(name), body).expect("an entry");
    };
    write(
        "org.example.Editor.desktop",
        "[Desktop Entry]\nType=Application\nName=Example Editor\nExec=example-editor %F\n",
    );
    let program = std::env::current_exe().unwrap();
    let program = program.file_name().unwrap().to_string_lossy();
    write(
        "org.example.Tests.desktop",
        &format!("[Desktop Entry]\nType=Application\nName=The Tests\nExec={program}\n"),
    );
    dir
}

fn reader(dir: &Path) -> GnomeWindow {
    GnomeWindow::with_dirs(vec![dir.to_owned()])
}

fn app(identifier: &str, name: &str, title: Option<&str>) -> ForegroundApp {
    ForegroundApp {
        identifier: identifier.to_owned(),
        name: name.to_owned(),
        title: title.map(str::to_owned),
    }
}

#[test]
#[ignore = "needs a session bus"]
fn the_app_in_front_comes_from_its_desktop_entry() {
    let _turn = serial();
    let shell = Shell::start(true);
    let dir = entries();
    let window = reader(dir.path());
    // The Shell's own app ID.
    shell.set(|state| {
        state.front = Some((
            "org.example.Editor.desktop".into(),
            "whatever".into(),
            0,
            "notes.txt".into(),
        ));
    });
    assert_eq!(
        window.foreground(false).unwrap(),
        Some(app("org.example.Editor", "Example Editor", None))
    );
    // A Wayland app ID with no Shell app: it is the desktop-file ID.
    shell.set(|state| {
        state.front = Some((String::new(), "org.example.Editor".into(), 0, String::new()));
    });
    assert_eq!(
        window.foreground(false).unwrap(),
        Some(app("org.example.Editor", "Example Editor", None))
    );
    // Neither, but the executable has an entry.
    shell.set(|state| {
        state.front = Some((
            String::new(),
            "Unknown-class".into(),
            std::process::id(),
            String::new(),
        ));
    });
    assert_eq!(
        window.foreground(false).unwrap(),
        Some(app("org.example.Tests", "The Tests", None))
    );
    // No entry at all: named by its class.
    shell.set(|state| state.front = Some((String::new(), "Kitty".into(), 0, String::new())));
    assert_eq!(
        window.foreground(false).unwrap(),
        Some(app("kitty", "Kitty", None))
    );
}

#[test]
#[ignore = "needs a session bus"]
fn the_title_is_read_only_when_asked_for() {
    let _turn = serial();
    let shell = Shell::start(true);
    let dir = entries();
    let window = reader(dir.path());
    shell.set(|state| {
        state.front = Some((
            String::new(),
            "org.example.Editor".into(),
            0,
            "plan.md".into(),
        ));
    });
    assert_eq!(window.foreground(false).unwrap().unwrap().title, None);
    assert_eq!(lock(&shell.state).asked_for_titles, 0);
    assert_eq!(
        window.foreground(true).unwrap().unwrap().title.as_deref(),
        Some("plan.md")
    );
}

#[test]
#[ignore = "needs a session bus"]
fn nothing_in_front_and_an_extension_not_running_or_refusing() {
    let _turn = serial();
    let dir = entries();
    {
        let _shell = Shell::start(true);
        assert_eq!(reader(dir.path()).foreground(false), Ok(None));
    }
    {
        let _shell = Shell::start(false);
        assert_eq!(
            reader(dir.path()).foreground(false),
            Err(SensorError::Unsupported)
        );
    }
    let shell = Shell::start(true);
    shell.set(|state| state.deny = true);
    assert_eq!(
        reader(dir.path()).foreground(false),
        Err(SensorError::PermissionDenied)
    );
}

#[test]
#[ignore = "needs a session bus"]
fn the_extension_is_turned_on_once_the_shell_knows_it() {
    let _turn = serial();
    let dir = entries();
    let shell = Shell::start(false);
    let window = reader(dir.path());
    // Installed since login: the Shell doesn't know it, and can't turn it on.
    assert_eq!(window.extension(), Ok(ExtensionState::NotLoaded));
    assert!(window.enable_extension().is_err());
    // Loaded but never turned on, or turned off.
    shell.set(|state| state.extension = Some(6.0));
    assert_eq!(window.extension(), Ok(ExtensionState::Disabled));
    shell.set(|state| state.extension = Some(2.0));
    assert_eq!(window.extension(), Ok(ExtensionState::Disabled));
    window.enable_extension().unwrap();
    assert_eq!(window.extension(), Ok(ExtensionState::Enabled));
    shell.set(|state| state.extension = Some(4.0));
    assert_eq!(window.extension(), Ok(ExtensionState::Failed));
    drop(shell);
    assert_eq!(window.extension(), Err(SensorError::Unsupported));
}

#[test]
#[ignore = "needs a session bus"]
fn a_reading_turns_the_extension_on_once_the_shell_knows_it() {
    let _turn = serial();
    let dir = entries();
    let shell = Shell::start(false);
    shell.set(|state| state.extension = Some(2.0));
    let window = reader(dir.path());
    assert_eq!(window.foreground(false), Err(SensorError::Unsupported));
    assert_eq!(lock(&shell.state).extension, Some(1.0));
    // The member turns it off again: it stays off for this run.
    shell.set(|state| state.extension = Some(2.0));
    assert_eq!(window.foreground(false), Err(SensorError::Unsupported));
    assert_eq!(lock(&shell.state).extension, Some(2.0));
}

#[test]
#[ignore = "needs a session bus"]
fn the_report_says_why_apps_are_missing() {
    let _turn = serial();
    let dir = entries();
    let report = |window: &GnomeWindow| {
        let capability = window.capability();
        (capability.status(), capability.reasons().to_vec())
    };
    {
        let _shell = Shell::start(true);
        assert_eq!(
            report(&reader(dir.path())),
            (CapabilityStatus::Granted, vec![])
        );
    }
    {
        // Installed since login.
        let _shell = Shell::start(false);
        assert_eq!(
            report(&reader(dir.path())),
            (
                CapabilityStatus::Pending,
                vec![CapabilityReason::GnomeExtensionNextLogin]
            )
        );
    }
    {
        // Turned off: the report's reading turns it on, but it doesn't answer.
        let shell = Shell::start(false);
        shell.set(|state| state.extension = Some(2.0));
        let window = reader(dir.path());
        assert_eq!(
            report(&window),
            (
                CapabilityStatus::Failed,
                vec![CapabilityReason::GnomeExtensionFailed]
            )
        );
        assert_eq!(lock(&shell.state).extension, Some(1.0));
        // The member turns it off again.
        shell.set(|state| state.extension = Some(2.0));
        assert_eq!(
            report(&window),
            (
                CapabilityStatus::Denied,
                vec![CapabilityReason::GnomeExtensionDisabled]
            )
        );
        shell.set(|state| state.extension = Some(4.0));
        assert_eq!(
            report(&window),
            (
                CapabilityStatus::Failed,
                vec![CapabilityReason::GnomeExtensionFailed]
            )
        );
        drop(shell);
        // No GNOME Shell any more.
        assert_eq!(
            report(&window),
            (
                CapabilityStatus::Unsupported,
                vec![CapabilityReason::DesktopUnsupported]
            )
        );
    }
    {
        // Turned off at login: the report's reading turns it on, and it answers a moment later.
        let shell = Shell::start(false);
        shell.set(|state| {
            state.extension = Some(2.0);
            state.starts_after_enable = Some(Duration::from_millis(400));
        });
        assert_eq!(
            report(&reader(dir.path())),
            (CapabilityStatus::Granted, vec![])
        );
    }
    let shell = Shell::start(true);
    shell.set(|state| state.deny = true);
    assert_eq!(
        report(&reader(dir.path())),
        (
            CapabilityStatus::Denied,
            vec![CapabilityReason::PermissionDenied]
        )
    );
}
