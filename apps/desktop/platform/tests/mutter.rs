//! The GNOME adapter (E13-S04) against a stand-in for Mutter's `IdleMonitor` on the session bus,
//! which keeps Mutter's rules: an idle watch fires each time idle time reaches its interval, and
//! a "user active" watch fires once, at the next input. They need a session bus of their own:
//! run with `--ignored` under `dbus-run-session`, as CI does on Linux.
//!
//! The real GNOME Shell 46 was checked by hand (`docs/spikes/E13-S01-wayland.md`).

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test fails loudly when its bus is missing"
)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message};
use worklume_platform::linux::MutterIdle;
use worklume_platform::{
    ActiveSpan, ActivitySpans, Clock, IDLE_AFTER_MS, IdleSource, OsClock, SensorError,
};

const SERVICE: &str = "org.gnome.Mutter.IdleMonitor";
const PATH: &str = "/org/gnome/Mutter/IdleMonitor/Core";
/// How far a notice may land from the moment the test expects, in milliseconds.
const SLACK_MS: i64 = 150;

/// Tests share the bus name, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
struct State {
    last_input: Instant,
    /// Input the stand-in hasn't acted on yet.
    input: bool,
    next_id: u32,
    /// Idle watches: id, interval, and whether it fired since the last input.
    idle: Vec<(u32, u64, bool)>,
    active: Vec<u32>,
    removed: Vec<u32>,
}

/// A stand-in for Mutter's idle monitor.
struct Mutter {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Mutter {
    /// On the bus, with the last input `idle` ago.
    fn start(idle: Duration) -> Self {
        let state = Arc::new(Mutex::new(State {
            last_input: Instant::now().checked_sub(idle).unwrap(),
            input: false,
            next_id: 1,
            idle: Vec::new(),
            active: Vec::new(),
            removed: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn({
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            move || {
                let conn = Connection::new_session().expect("a session bus");
                conn.request_name(SERVICE, false, true, true)
                    .expect("the name is free");
                conn.start_receive(MatchRule::new_method_call(), {
                    let state = Arc::clone(&state);
                    Box::new(move |call, conn| {
                        let reply = answer(&call, &mut lock(&state));
                        conn.send(reply).ok();
                        true
                    })
                });
                ready_tx.send(()).expect("the test waits");
                while !stop.load(Ordering::SeqCst) {
                    drop(conn.process(Duration::from_millis(10)));
                    for watch in fire(&mut lock(&state)) {
                        let signal = Message::new_signal(PATH, SERVICE, "WatchFired")
                            .unwrap()
                            .append1(watch);
                        conn.send(signal).ok();
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

    /// The member touches the keyboard or mouse now.
    fn input(&self) -> Instant {
        let mut state = lock(&self.state);
        state.last_input = Instant::now();
        state.input = true;
        state.last_input
    }

    fn removed(&self) -> Vec<u32> {
        lock(&self.state).removed.clone()
    }
}

impl Drop for Mutter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn answer(call: &Message, state: &mut State) -> Message {
    let mut new_id = || {
        state.next_id += 1;
        state.next_id
    };
    match call.member().as_deref() {
        _ if call.path().as_deref() != Some(PATH) => call.error(
            &"org.freedesktop.DBus.Error.UnknownObject".into(),
            c"no such object",
        ),
        Some("AddIdleWatch") => {
            let interval: u64 = call.read1().unwrap();
            let id = new_id();
            state.idle.push((id, interval, false));
            call.method_return().append1(id)
        }
        Some("AddUserActiveWatch") => {
            let id = new_id();
            state.active.push(id);
            call.method_return().append1(id)
        }
        Some("RemoveWatch") => {
            let id: u32 = call.read1().unwrap();
            state.idle.retain(|watch| watch.0 != id);
            state.active.retain(|watch| *watch != id);
            state.removed.push(id);
            call.method_return()
        }
        Some("GetIdletime") => {
            let idle = u64::try_from(state.last_input.elapsed().as_millis()).unwrap();
            call.method_return().append1(idle)
        }
        _ => call.error(
            &"org.freedesktop.DBus.Error.UnknownMethod".into(),
            c"no such method",
        ),
    }
}

/// The watches due to fire now, as Mutter fires them.
fn fire(state: &mut State) -> Vec<u32> {
    let mut due = Vec::new();
    if state.input {
        state.input = false;
        due.append(&mut state.active);
        for watch in &mut state.idle {
            watch.2 = false;
        }
    }
    let idle = u64::try_from(state.last_input.elapsed().as_millis()).unwrap();
    for watch in &mut state.idle {
        if !watch.2 && idle >= watch.1 {
            watch.2 = true;
            due.push(watch.0);
        }
    }
    due
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now(clock: &dyn Clock) -> i64 {
    clock.read().expect("the clock").mono_ms
}

fn near(actual: i64, expected: i64) -> bool {
    (actual - expected).abs() <= SLACK_MS
}

/// A reader, once it has set its watches: idle time is unknown until then.
fn reader() -> (MutterIdle, Arc<OsClock>) {
    let clock = Arc::new(OsClock::new().expect("the clock"));
    let idle = MutterIdle::connect(Arc::clone(&clock) as _).expect("the reader");
    let started = Instant::now();
    while idle.idle_for().is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the reader never set its watches"
        );
        sleep(Duration::from_millis(10));
    }
    (idle, clock)
}

fn idle_ms(idle: &MutterIdle) -> i64 {
    i64::try_from(idle.idle_for().unwrap().as_millis()).unwrap()
}

#[test]
#[ignore = "needs a session bus"]
fn spans_follow_mutters_watches() {
    let _turn = serial();
    let mutter = Mutter::start(Duration::from_secs(10));
    let (idle, clock) = reader();
    // Exact idle time from the start, and no active time.
    assert!(near(idle_ms(&idle), 10_000), "idle {:?}", idle.idle_for());
    sleep(Duration::from_millis(200));
    assert_eq!(idle.drain().unwrap(), []);

    let mut inputs = Vec::new();
    for _ in 0..5 {
        mutter.input();
        inputs.push(now(&*clock));
        sleep(Duration::from_millis(300));
    }
    let (first, last) = (inputs[0], inputs[4]);
    let spans = idle.drain().unwrap();
    assert!(
        matches!(spans.as_slice(), [ActiveSpan { start_mono, end_mono: None }] if near(*start_mono, first)),
        "{spans:?}, first input at {first}"
    );
    assert_eq!(idle.idle_for().unwrap(), Duration::ZERO);

    sleep(Duration::from_millis(1_500));
    let spans = idle.drain().unwrap();
    assert!(
        matches!(
            spans.as_slice(),
            [ActiveSpan { start_mono, end_mono: Some(end) }]
                if near(*start_mono, first) && near(*end, last + IDLE_AFTER_MS)
        ),
        "{spans:?}, inputs from {first} to {last}"
    );
    assert!(near(idle_ms(&idle), now(&*clock) - last));

    // A second burst begins at its first input: the "user active" watch was set again after
    // the idle notice.
    sleep(Duration::from_millis(500));
    let mut again = Vec::new();
    for _ in 0..4 {
        mutter.input();
        again.push(now(&*clock));
        sleep(Duration::from_millis(300));
    }
    sleep(Duration::from_millis(1_500));
    let spans = idle.drain().unwrap();
    assert!(
        matches!(
            spans.as_slice(),
            [ActiveSpan { start_mono, end_mono: Some(end) }]
                if near(*start_mono, again[0]) && near(*end, again[3] + IDLE_AFTER_MS)
        ),
        "{spans:?}, inputs from {} to {}",
        again[0],
        again[3]
    );
}

#[test]
#[ignore = "needs a session bus"]
fn active_at_the_start_is_an_open_span() {
    let _turn = serial();
    let _mutter = Mutter::start(Duration::from_millis(200));
    let (idle, clock) = reader();
    let started = now(&*clock);
    let spans = idle.drain().unwrap();
    assert!(
        matches!(spans.as_slice(), [ActiveSpan { start_mono, end_mono: None }] if near(*start_mono, started - 200)),
        "{spans:?}, last input at {}",
        started - 200
    );
    sleep(Duration::from_millis(1_200));
    assert!(matches!(
        idle.drain().unwrap().as_slice(),
        [ActiveSpan {
            end_mono: Some(_),
            ..
        }]
    ));
}

#[test]
#[ignore = "needs a session bus"]
fn only_mutters_signals_count() {
    let _turn = serial();
    let _mutter = Mutter::start(Duration::from_secs(10));
    let (idle, _clock) = reader();
    // Another client sends "fired" for every watch the reader may hold.
    let other = Connection::new_session().expect("a session bus");
    for watch in 0..20_u32 {
        let signal = Message::new_signal(PATH, SERVICE, "WatchFired")
            .unwrap()
            .append1(watch);
        other.send(signal).unwrap();
    }
    sleep(Duration::from_millis(500));
    assert_eq!(idle.drain().unwrap(), []);
    assert!(idle_ms(&idle) >= 10_000);
}

#[test]
#[ignore = "needs a session bus"]
fn without_mutter_the_reader_is_unsupported_and_mutter_leaving_is_reported() {
    let _turn = serial();
    assert!(matches!(
        MutterIdle::connect(Arc::new(OsClock::new().unwrap())),
        Err(SensorError::Unsupported)
    ));
    let mutter = Mutter::start(Duration::from_secs(10));
    let (idle, _clock) = reader();
    drop(mutter);
    sleep(Duration::from_millis(800));
    assert!(matches!(idle.drain(), Err(SensorError::Failed(_))));
    assert!(idle.idle_for().is_err());
}

#[test]
#[ignore = "needs a session bus"]
fn dropping_the_reader_removes_its_watches_promptly() {
    let _turn = serial();
    let mutter = Mutter::start(Duration::from_secs(10));
    let (idle, _clock) = reader();
    let dropped = Instant::now();
    drop(idle);
    assert!(dropped.elapsed() < Duration::from_secs(1));
    assert_eq!(mutter.removed().len(), 2, "{:?}", mutter.removed());
}
