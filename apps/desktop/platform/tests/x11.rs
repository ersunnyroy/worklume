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

use worklume_platform::linux::{X11Idle, X11Input};
use worklume_platform::{
    Clock, DeviceSensors, IdleSource, InputActivity, InputEvent, InputKind, OsClock,
};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, GrabMode, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

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
    let sensors = DeviceSensors::start(&clock);
    assert!(sensors.problems.is_empty(), "{:?}", sensors.problems);
    assert!(sensors.input_hooks);
    assert!(sensors.idle.idle_for().is_ok());
    drop(sensors.input.drain());
    hands.key();
    hands.sync();
    sleep(Duration::from_millis(200));
    let events = sensors.input.drain().expect("the reader works");
    assert_eq!(count(&events, InputKind::Key), 1);
}
