//! The Wayland screen capture (E13-S03) against a stand-in for the desktop portal's `ScreenCast`
//! interface, and a real `PipeWire` with a test video source (`GStreamer`'s `videotestsrc`) as the
//! shared monitor. They need `PipeWire`, `WirePlumber`, `GStreamer`'s `PipeWire` plugin and a session bus
//! of their own: run with `--ignored` under `dbus-run-session`, as CI does on Linux.
//!
//! The real GNOME portal was checked by hand (`docs/conventions/desktop.md`, "Wayland screen
//! capture").

#![cfg(target_os = "linux")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails loudly when its PipeWire or bus is missing"
)]

use std::os::fd::OwnedFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::{MatchRule, Message};
use tempfile::TempDir;
use worklume_platform::fake::MemorySecretStore;
use worklume_platform::linux::{ANSWER_WITHIN, PortalCapture, TOKEN_SECRET};
use worklume_platform::{Monitors, ScreenCapture, SecretStore, SensorError};

const OBJECT: &str = "/org/freedesktop/portal/desktop";
/// The test source's colour, as `videotestsrc` takes it: `ARGB`.
const COLOUR: &str = "0xff2050c0";
const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

/// Tests share `PipeWire` and the portal's bus name, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `PipeWire`, `WirePlumber` and a test video source, for the whole run.
struct Media {
    dir: TempDir,
    node: u32,
    _children: Vec<Child>,
}

fn media() -> &'static Media {
    static MEDIA: OnceLock<Media> = OnceLock::new();
    MEDIA.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("pw")
            .tempdir_in("/tmp")
            .expect("a runtime folder");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let spawn = |program: &str, args: &[&str]| {
            Command::new(program)
                .args(args)
                .env("XDG_RUNTIME_DIR", dir.path())
                .env_remove("PIPEWIRE_REMOTE")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap_or_else(|error| panic!("{program} is installed: {error}"))
        };
        let mut children = vec![spawn("pipewire", &[])];
        wait_for(|| dir.path().join("pipewire-0").exists(), "PipeWire");
        children.push(spawn("wireplumber", &[]));
        sleep(Duration::from_millis(500));
        let caps = format!("video/x-raw,format=BGRx,width={WIDTH},height={HEIGHT},framerate=10/1");
        children.push(spawn(
            "gst-launch-1.0",
            &[
                "-q",
                "videotestsrc",
                "pattern=solid-color",
                &format!("foreground-color={COLOUR}"),
                "!",
                &caps,
                "!",
                "pipewiresink",
                "stream-properties=props,media.class=Video/Source,node.name=worklume-test-screen",
            ],
        ));
        let mut node = None;
        wait_for(
            || {
                node = node_id(dir.path().to_path_buf());
                node.is_some()
            },
            "the test source",
        );
        Media {
            dir,
            node: node.unwrap(),
            _children: children,
        }
    })
}

/// The test source's node, from `pw-dump`.
fn node_id(runtime: PathBuf) -> Option<u32> {
    let output = Command::new("pw-dump")
        .env("XDG_RUNTIME_DIR", runtime)
        .output()
        .ok()?;
    let objects: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    objects.as_array()?.iter().find_map(|object| {
        let name = object.pointer("/info/props/node.name")?.as_str()?;
        (name == "worklume-test-screen")
            .then(|| object.get("id")?.as_u64())
            .flatten()
            .and_then(|id| u32::try_from(id).ok())
    })
}

fn wait_for(mut ready: impl FnMut() -> bool, what: &str) {
    let started = Instant::now();
    while !ready() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{what} didn't start"
        );
        sleep(Duration::from_millis(100));
    }
}

/// How the stand-in's "Share screen" dialog is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    Shares,
    Cancels,
    SharesAfter(Duration),
}

#[derive(Debug)]
struct Portal {
    member: Member,
    /// How many monitors Start shares.
    monitors: u32,
    starts: u32,
    /// The restore token each `SelectSources` carried.
    tokens: Vec<Option<String>>,
    persist_mode: Option<u32>,
    multiple: Option<bool>,
    closed_sessions: u32,
    issued: u32,
    due: Vec<(Instant, Message)>,
}

/// A stand-in for the portal's `ScreenCast` interface.
struct StandIn {
    portal: Arc<Mutex<Portal>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StandIn {
    fn start(member: Member) -> Self {
        let portal = Arc::new(Mutex::new(Portal {
            member,
            monitors: 1,
            starts: 0,
            tokens: Vec::new(),
            persist_mode: None,
            multiple: None,
            closed_sessions: 0,
            issued: 0,
            due: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn({
            let portal = Arc::clone(&portal);
            let stop = Arc::clone(&stop);
            move || {
                let conn = Connection::new_session().expect("a session bus");
                conn.request_name("org.freedesktop.portal.Desktop", false, true, true)
                    .expect("the name is free");
                conn.start_receive(MatchRule::new_method_call(), {
                    let portal = Arc::clone(&portal);
                    Box::new(move |call, conn| {
                        let (reply, signal) = answer(&call, &mut lock(&portal));
                        conn.send(reply).ok();
                        if let Some(signal) = signal {
                            conn.send(signal).ok();
                        }
                        true
                    })
                });
                ready_tx.send(()).expect("the test waits");
                while !stop.load(Ordering::SeqCst) {
                    drop(conn.process(Duration::from_millis(20)));
                    let now = Instant::now();
                    let due: Vec<Message> = {
                        let mut portal = lock(&portal);
                        let (ready, waiting) = std::mem::take(&mut portal.due)
                            .into_iter()
                            .partition(|(at, _)| *at <= now);
                        portal.due = waiting;
                        ready.into_iter().map(|(_, signal)| signal).collect()
                    };
                    for signal in due {
                        conn.send(signal).ok();
                    }
                }
            }
        });
        ready_rx.recv().expect("the stand-in started");
        Self {
            portal,
            stop,
            thread: Some(thread),
        }
    }

    fn portal(&self) -> MutexGuard<'_, Portal> {
        lock(&self.portal)
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

impl Portal {
    /// Records what `SelectSources` asked for.
    fn select(&mut self, options: &PropMap) {
        self.tokens.push(
            options
                .get("restore_token")
                .and_then(|token| token.0.as_str())
                .map(str::to_owned),
        );
        self.persist_mode = options
            .get("persist_mode")
            .and_then(|mode| mode.0.as_u64())
            .and_then(|mode| u32::try_from(mode).ok());
        self.multiple = options
            .get("multiple")
            .and_then(|multiple| multiple.0.as_u64())
            .map(|multiple| multiple != 0);
    }
}

/// The reply to a call, and the Response signal to send now (or queue).
fn answer(call: &Message, portal: &mut Portal) -> (Message, Option<Message>) {
    let member = call
        .member()
        .map(|member| member.to_string())
        .unwrap_or_default();
    let path = call.path().map(|path| path.to_string()).unwrap_or_default();
    if path != OBJECT {
        // Request.Close and Session.Close.
        if path.contains("/session/") && member == "Close" {
            portal.closed_sessions += 1;
        }
        return (call.method_return(), None);
    }
    let sender = call
        .sender()
        .map(|sender| sender.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    let options = last_dict(call).unwrap_or_default();
    let handle = |kind: &str, key: &str| {
        let token = options
            .get(key)
            .and_then(|token| token.0.as_str())
            .unwrap_or("none")
            .to_owned();
        format!("{OBJECT}/{kind}/{sender}/{token}")
    };
    let request = handle("request", "handle_token");
    let response = |code: u32, results: PropMap| {
        Message::new_signal(
            request.clone(),
            "org.freedesktop.portal.Request",
            "Response",
        )
        .unwrap()
        .append2(code, results)
    };
    match member.as_str() {
        "CreateSession" => {
            let mut results = PropMap::new();
            results.insert(
                "session_handle".to_owned(),
                Variant(Box::new(handle("session", "session_handle_token"))),
            );
            let reply = call
                .method_return()
                .append1(dbus::Path::from(request.clone()));
            (reply, Some(response(0, results)))
        }
        "SelectSources" => {
            portal.select(&options);
            let reply = call
                .method_return()
                .append1(dbus::Path::from(request.clone()));
            (reply, Some(response(0, PropMap::new())))
        }
        "Start" => {
            portal.starts += 1;
            let reply = call
                .method_return()
                .append1(dbus::Path::from(request.clone()));
            let restored = portal.tokens.last().cloned().flatten().is_some();
            let signal = if portal.member == Member::Cancels && !restored {
                response(1, PropMap::new())
            } else {
                portal.issued += 1;
                let streams = shared(portal.monitors);
                let mut results = PropMap::new();
                results.insert(
                    "streams".to_owned(),
                    Variant(Box::new(streams) as Box<dyn RefArg>),
                );
                results.insert(
                    "restore_token".to_owned(),
                    Variant(Box::new(format!("token-{}", portal.issued))),
                );
                response(0, results)
            };
            match portal.member {
                Member::SharesAfter(delay) if !restored => {
                    portal.due.push((Instant::now() + delay, signal));
                    (reply, None)
                }
                _ => (reply, Some(signal)),
            }
        }
        "OpenPipeWireRemote" => {
            let socket = UnixStream::connect(media().dir.path().join("pipewire-0"))
                .expect("PipeWire's socket");
            (call.method_return().append1(OwnedFd::from(socket)), None)
        }
        _ => (
            call.error(
                &"org.freedesktop.DBus.Error.UnknownMethod".into(),
                c"no such method",
            ),
            None,
        ),
    }
}

/// Start's streams: `count` monitors side by side, each the test source.
fn shared(count: u32) -> Vec<(u32, PropMap)> {
    (0..count)
        .map(|index| {
            let mut properties = PropMap::new();
            properties.insert("id".to_owned(), Variant(Box::new(index.to_string())));
            let x = i32::try_from(index * WIDTH).unwrap();
            properties.insert("position".to_owned(), Variant(Box::new((x, 0_i32))));
            let size = (
                i32::try_from(WIDTH).unwrap(),
                i32::try_from(HEIGHT).unwrap(),
            );
            properties.insert("size".to_owned(), Variant(Box::new(size)));
            (media().node, properties)
        })
        .collect()
}

/// A call's last argument, read as the options dictionary.
fn last_dict(call: &Message) -> Option<PropMap> {
    let mut items = call.iter_init();
    let mut last = None;
    loop {
        if let Ok(options) = items.read::<PropMap>() {
            last = Some(options);
            continue;
        }
        if !items.next() {
            break;
        }
    }
    last
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn capture(secrets: &Arc<MemorySecretStore>) -> PortalCapture {
    PortalCapture::new(Arc::clone(secrets) as Arc<dyn SecretStore>)
}

fn saved(secrets: &MemorySecretStore) -> Option<String> {
    secrets
        .get(TOKEN_SECRET)
        .unwrap()
        .map(|token| token.as_str().to_owned())
}

#[test]
#[ignore = "needs PipeWire and a session bus"]
fn one_consent_then_captures_without_asking() {
    let _turn = serial();
    let portal = StandIn::start(Member::Shares);
    let secrets = Arc::new(MemorySecretStore::default());
    let screen = capture(&secrets);
    assert_eq!(screen.monitors(), Err(SensorError::PermissionDenied));

    let frames = screen.capture(Monitors::All).expect("frames");
    assert_eq!(frames.len(), 1);
    let frame = &frames[0];
    assert_eq!((frame.monitor.width, frame.monitor.height), (WIDTH, HEIGHT));
    assert_eq!(
        frame.rgba.len(),
        usize::try_from(WIDTH * HEIGHT * 4).unwrap()
    );
    // The source's colour, 0x2050c0, in RGBA.
    assert_eq!(&frame.rgba[..4], [0x20, 0x50, 0xc0, 0xff]);
    assert_eq!(saved(&secrets).as_deref(), Some("token-1"));
    assert_eq!(
        screen.monitors().unwrap(),
        std::slice::from_ref(&frame.monitor)
    );
    {
        let portal = portal.portal();
        // Every monitor, remembered until revoked.
        assert_eq!(
            (portal.persist_mode, portal.multiple),
            (Some(2), Some(true))
        );
        assert_eq!(portal.tokens, [None]);
    }

    // The next capture restores the session from the token, and keeps the new one.
    screen.capture(Monitors::All).expect("frames");
    assert_eq!(portal.portal().tokens, [None, Some("token-1".to_owned())]);
    assert_eq!(saved(&secrets).as_deref(), Some("token-2"));
    // A new run of the agent reads the token from the credential store.
    drop(screen);
    capture(&secrets).capture(Monitors::All).expect("frames");
    assert_eq!(
        portal.portal().tokens.last().cloned().flatten().as_deref(),
        Some("token-2")
    );
    // Every session was closed, which ends the screen sharing.
    assert_eq!(portal.portal().closed_sessions, 3);
}

#[test]
#[ignore = "needs PipeWire and a session bus"]
fn the_active_monitor_is_the_first_one_shared() {
    let _turn = serial();
    let portal = StandIn::start(Member::Shares);
    portal.portal().monitors = 2;
    let secrets = Arc::new(MemorySecretStore::default());
    let screen = capture(&secrets);
    let all = screen.capture(Monitors::All).expect("frames");
    assert_eq!(all.len(), 2);
    assert_eq!((all[0].monitor.x, all[1].monitor.x), (0, 640));
    let active = screen.capture(Monitors::Active).expect("frames");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].monitor, all[0].monitor);
}

#[test]
#[ignore = "needs PipeWire and a session bus"]
fn a_member_who_cancels_is_not_asked_again_until_asked_for() {
    let _turn = serial();
    let portal = StandIn::start(Member::Cancels);
    let secrets = Arc::new(MemorySecretStore::default());
    let screen = capture(&secrets);
    assert!(matches!(
        screen.capture(Monitors::All),
        Err(SensorError::PermissionDenied)
    ));
    assert!(matches!(
        screen.capture(Monitors::All),
        Err(SensorError::PermissionDenied)
    ));
    assert_eq!(portal.portal().starts, 1);
    assert_eq!(saved(&secrets), None);

    portal.portal().member = Member::Shares;
    screen.ask_again();
    assert!(screen.capture(Monitors::All).is_ok());
    assert_eq!(portal.portal().starts, 2);
}

#[test]
#[ignore = "needs PipeWire and a session bus"]
fn a_slow_answer_reads_as_waiting_and_counts_once_it_comes() {
    let _turn = serial();
    let answer_after = ANSWER_WITHIN + Duration::from_secs(2);
    let portal = StandIn::start(Member::SharesAfter(answer_after));
    let secrets = Arc::new(MemorySecretStore::default());
    let screen = capture(&secrets);
    let started = Instant::now();
    assert!(matches!(
        screen.capture(Monitors::All),
        Err(SensorError::PermissionDenied)
    ));
    assert!(started.elapsed() >= ANSWER_WITHIN);
    // Still waiting for the member: nothing new is asked.
    assert!(matches!(
        screen.capture(Monitors::All),
        Err(SensorError::PermissionDenied)
    ));
    assert_eq!(portal.portal().starts, 1);
    // The member shared: the token is kept, and captures need no dialog.
    wait_for(|| saved(&secrets).is_some(), "the token");
    let quick = Instant::now();
    assert!(screen.capture(Monitors::All).is_ok());
    assert!(quick.elapsed() < Duration::from_secs(5));
}

#[test]
#[ignore = "needs PipeWire and a session bus"]
fn without_a_portal_capture_is_unsupported() {
    let _turn = serial();
    let secrets = Arc::new(MemorySecretStore::default());
    assert!(matches!(
        capture(&secrets).capture(Monitors::All),
        Err(SensorError::Unsupported)
    ));
}
