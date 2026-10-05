//! The `ext-idle-notify-v1` adapter (E13-S04) against a real compositor: sway, headless with the
//! software renderer, which each test starts in a folder of its own. Input comes through
//! wlroots' virtual pointer. They need sway installed: run with `--ignored`, as CI does on
//! Linux.

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test fails loudly when its compositor is missing"
)]

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};
use worklume_platform::linux::WaylandIdle;
use worklume_platform::{
    ActiveSpan, ActivitySpans, Clock, IDLE_AFTER_MS, IdleSource, OsClock, SensorError,
};

/// How far a notice may land from the moment the test expects, in milliseconds: the
/// compositor's timers, the reader thread and a busy CI machine.
const SLACK_MS: i64 = 150;

/// A headless sway, stopped when dropped.
struct Sway {
    child: Child,
    socket: PathBuf,
    _dir: TempDir,
}

impl Sway {
    fn start() -> Self {
        // A short path: sway's IPC socket name must fit in a socket address.
        let dir = tempfile::Builder::new()
            .prefix("wl")
            .tempdir_in("/tmp")
            .expect("a runtime folder");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("a private runtime folder");
        let config = dir.path().join("config");
        std::fs::write(&config, "output HEADLESS-1 resolution 640x480\n").expect("a config");
        let child = Command::new("sway")
            .arg("--config")
            .arg(&config)
            .env("XDG_RUNTIME_DIR", dir.path())
            .env("WLR_BACKENDS", "headless")
            .env("WLR_RENDERER", "pixman")
            .env("WLR_LIBINPUT_NO_DEVICES", "1")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sway is installed");
        let started = Instant::now();
        let socket = loop {
            let found = std::fs::read_dir(dir.path())
                .expect("the runtime folder")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with("wayland-")
                                && Path::new(name).extension().is_none_or(|ext| ext != "lock")
                        })
                });
            if let Some(socket) = found
                && UnixStream::connect(&socket).is_ok()
            {
                break socket;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "sway didn't start"
            );
            sleep(Duration::from_millis(50));
        };
        Self {
            child,
            socket,
            _dir: dir,
        }
    }

    fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for Sway {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

/// A virtual pointer, as remote-desktop tools use.
struct Hand {
    queue: EventQueue<Nothing>,
    pointer: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    clock: OsClock,
    moves: u32,
}

struct Nothing;

impl Hand {
    fn new(sway: &Sway) -> Self {
        let conn =
            Connection::from_socket(UnixStream::connect(sway.socket()).expect("the compositor"))
                .expect("a connection");
        let (globals, mut queue) = registry_queue_init::<Nothing>(&conn).expect("globals");
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).expect("a seat");
        let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 =
            globals.bind(&qh, 1..=2, ()).expect("virtual pointers");
        let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
        queue
            .roundtrip(&mut Nothing)
            .expect("the compositor answers");
        Self {
            queue,
            pointer,
            clock: OsClock::new().expect("the clock"),
            moves: 0,
        }
    }

    /// Moves the pointer a little, and returns when, once the compositor has it.
    fn moves(&mut self) -> i64 {
        self.moves += 1;
        let step = if self.moves.is_multiple_of(2) {
            3.0
        } else {
            -3.0
        };
        self.pointer.motion(self.moves * 10, step, 0.0);
        self.pointer.frame();
        self.queue
            .roundtrip(&mut Nothing)
            .expect("the compositor answers");
        now(&self.clock)
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Nothing {
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

delegate_noop!(Nothing: ignore wl_seat::WlSeat);
delegate_noop!(Nothing: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
delegate_noop!(Nothing: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

fn now(clock: &dyn Clock) -> i64 {
    clock.read().expect("the clock").mono_ms
}

fn near(actual: i64, expected: i64) -> bool {
    (actual - expected).abs() <= SLACK_MS
}

fn reader(sway: &Sway) -> (WaylandIdle, Arc<OsClock>) {
    let clock = Arc::new(OsClock::new().expect("the clock"));
    let idle = WaylandIdle::connect_to(sway.socket(), Arc::clone(&clock) as _).expect("the reader");
    (idle, clock)
}

#[test]
#[ignore = "needs sway"]
fn a_burst_of_input_is_one_span_to_a_second_after_its_last_input() {
    let sway = Sway::start();
    let (idle, clock) = reader(&sway);
    let mut hand = Hand::new(&sway);
    // Nobody at the keyboard: idle after a second, and that second isn't active time.
    sleep(Duration::from_millis(1_500));
    let idle_for = idle.idle_for().expect("known after a second");
    assert!(idle_for >= Duration::from_millis(300), "idle {idle_for:?}");
    assert_eq!(idle.drain().unwrap(), []);

    let moves: Vec<i64> = (0..5)
        .map(|_| {
            let at = hand.moves();
            sleep(Duration::from_millis(300));
            at
        })
        .collect();
    let (first, last) = (moves[0], moves[4]);
    // Still active: the span is open, and idle time is none.
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
    assert_eq!(idle.drain().unwrap(), []);
    let idle_ms = i64::try_from(idle.idle_for().unwrap().as_millis()).unwrap();
    assert!(near(idle_ms, now(&*clock) - last), "idle {idle_ms} ms");
}

#[test]
#[ignore = "needs sway"]
fn separate_bursts_are_separate_spans() {
    let sway = Sway::start();
    let (idle, _clock) = reader(&sway);
    let mut hand = Hand::new(&sway);
    sleep(Duration::from_millis(1_200));
    let first = hand.moves();
    sleep(Duration::from_millis(1_500));
    let second = hand.moves();
    sleep(Duration::from_millis(1_500));
    let spans = idle.drain().unwrap();
    assert!(
        matches!(
            spans.as_slice(),
            [
                ActiveSpan { start_mono: a, end_mono: Some(a_end) },
                ActiveSpan { start_mono: b, end_mono: Some(b_end) },
            ] if near(*a, first) && near(*a_end, first + IDLE_AFTER_MS)
                && near(*b, second) && near(*b_end, second + IDLE_AFTER_MS)
        ),
        "{spans:?}, inputs at {first} and {second}"
    );
}

#[test]
#[ignore = "needs sway"]
fn a_compositor_that_goes_away_is_reported_and_dropping_the_reader_is_prompt() {
    let mut sway = Sway::start();
    let (idle, _clock) = reader(&sway);
    sleep(Duration::from_millis(300));
    drop(sway.child.kill());
    drop(sway.child.wait());
    sleep(Duration::from_millis(800));
    assert!(matches!(idle.drain(), Err(SensorError::Failed(_))));
    assert!(idle.idle_for().is_err());

    let sway = Sway::start();
    let (idle, _clock) = reader(&sway);
    let dropped = Instant::now();
    drop(idle);
    assert!(dropped.elapsed() < Duration::from_secs(1));
}
