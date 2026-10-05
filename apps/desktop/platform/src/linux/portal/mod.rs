//! Screen capture on Wayland (E13-S03), through the desktop portal's `ScreenCast` interface and
//! `PipeWire`. Wayland lets no app read the screen; the portal asks the member once which screens
//! to share, and a restore token lets later captures skip the question. How each desktop behaves
//! is in `docs/spikes/E13-S01-wayland.md`.
//!
//! - **Not the Screenshot portal.** On GNOME it flashes the screen, plays the shutter sound,
//!   blocks input for half a second and writes a PNG into the member's Pictures folder on every
//!   capture; wlroots' backend writes to a world-readable `/tmp/out.png`.
//! - **One consent.** A capture without a restore token shows the desktop's "Share screen"
//!   dialog, asking for every monitor (`multiple`) and to remember the choice (`persist_mode` 2,
//!   "until revoked"). The token that comes back is kept in the OS credential store, like other
//!   secrets: it lets its holder record the screen without asking. Each session returns a new
//!   token, and the newest is kept.
//! - **Asynchronous consent.** The member may take minutes to answer, or never. A capture waits
//!   up to [`ANSWER_WITHIN`]; after that it reads `PermissionDenied` while the dialog stays up,
//!   and the token arrives for later captures once the member shares. A member who cancels is
//!   not asked again until [`PortalCapture::ask_again`].
//! - **Each capture** opens a session, takes one frame from each shared monitor's `PipeWire`
//!   stream (`frames`), and closes the session. GNOME shows its screen-sharing indicator while
//!   the session is open: a brief blink per capture.
//! - **Monitors** are the streams' positions and sizes. Wayland doesn't say which monitor the
//!   member is using, so `Monitors::Active` takes the first one shared.

mod frames;

use std::collections::HashMap;
use std::fmt;
use std::os::fd::OwnedFd;
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use dbus::Message;
use dbus::arg::{PropMap, RefArg, Variant};
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
use dbus::message::MatchRule;
use zeroize::Zeroizing;

use super::bus::ABSENT;
use crate::{
    Capability, CapabilityProbe, CapabilityReason, CapabilityStatus, Frame, Monitor, Monitors,
    ScreenCapture, SecretStore, SensorError,
};

/// The credential store entry holding the restore token.
pub const TOKEN_SECRET: &str = "screencast-restore-token";

/// How long a capture waits for the portal: enough for a session without a dialog and its
/// frames; beyond it the member is being asked.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(10);
/// How long the dialog may wait for the member before it is closed.
const DIALOG_WITHIN: Duration = Duration::from_mins(10);
/// How long a portal call (not its answer) may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(25);
/// How long the frames may take once the streams are open.
const FRAMES_WITHIN: Duration = Duration::from_secs(5);
/// How long asking whether the portal offers screen casts may take, for the capability report:
/// the first call may start the portal, which first starts its backends.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
const SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
const REQUEST: &str = "org.freedesktop.portal.Request";
const SESSION: &str = "org.freedesktop.portal.Session";

/// Screenshots through the `ScreenCast` portal. See the module documentation.
pub struct PortalCapture {
    shared: Arc<Shared>,
}

struct Shared {
    secrets: Arc<dyn SecretStore>,
    state: Mutex<State>,
    monitors: Mutex<Vec<Monitor>>,
}

#[derive(Default)]
struct State {
    token: Option<Zeroizing<String>>,
    /// The token was read from the credential store.
    loaded: bool,
    /// A session is open, maybe waiting for the member.
    busy: bool,
    /// The member cancelled the dialog.
    declined: bool,
    /// Whether the portal offers screen casts, once known.
    offered: Option<bool>,
}

/// Where the member's consent stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consent {
    /// The desktop asks at the first capture.
    NotAsked,
    /// The dialog is up.
    Asking,
    /// A restore token is held: captures need no dialog.
    Granted,
    /// The member cancelled the dialog; not asked again until [`PortalCapture::ask_again`].
    Declined,
    /// The desktop has no screen-cast portal.
    Unsupported,
}

/// One shared monitor's stream.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stream {
    node: u32,
    monitor: Monitor,
}

/// Why a session ended without frames.
#[derive(Debug)]
enum Failure {
    /// No portal, or no `ScreenCast` in it.
    Unsupported,
    /// The member cancelled, or the dialog went unanswered.
    Declined,
    Failed(String),
}

impl PortalCapture {
    /// Captures through the session bus's portal, keeping the restore token in `secrets`.
    /// Nothing is asked until the first capture.
    #[must_use]
    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            shared: Arc::new(Shared {
                secrets,
                state: Mutex::new(State::default()),
                monitors: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Lets the next capture ask the member again after they cancelled. Returns whether they
    /// had.
    #[must_use]
    pub fn ask_again(&self) -> bool {
        std::mem::take(&mut lock(&self.shared.state).declined)
    }

    /// Whether a restore token is held: captures then need no dialog, unless the member revoked
    /// it or the monitors changed.
    #[must_use]
    pub fn consented(&self) -> bool {
        lock(&self.shared.state).token.is_some()
    }

    /// Where the member's consent stands. Reads the token from the credential store if no
    /// capture has yet, and asks the portal once whether it offers screen casts.
    #[must_use]
    pub fn consent(&self) -> Consent {
        {
            let mut state = lock(&self.shared.state);
            self.shared.load(&mut state);
            if let Some(consent) = state.consent() {
                return consent;
            }
        }
        // Asked without the lock held: a capture must not wait behind it.
        let offered = offers_screencast();
        let mut state = lock(&self.shared.state);
        if state.offered.is_none() {
            state.offered = offered;
        }
        state.consent().unwrap_or(Consent::NotAsked)
    }
}

impl State {
    /// The consent, unless it depends on whether the portal offers screen casts and that isn't
    /// known yet.
    const fn consent(&self) -> Option<Consent> {
        if self.declined {
            Some(Consent::Declined)
        } else if self.token.is_some() {
            Some(Consent::Granted)
        } else if self.busy {
            Some(Consent::Asking)
        } else {
            match self.offered {
                Some(true) => Some(Consent::NotAsked),
                Some(false) => Some(Consent::Unsupported),
                None => None,
            }
        }
    }
}

impl CapabilityProbe for PortalCapture {
    fn capability(&self) -> Capability {
        use CapabilityReason as Why;
        use CapabilityStatus as Status;
        match self.consent() {
            Consent::Granted => Capability::granted(),
            Consent::NotAsked => Capability::limited(Status::Pending, Why::ConsentNotAsked, &[]),
            Consent::Asking => Capability::limited(Status::Pending, Why::ConsentDialogOpen, &[]),
            Consent::Declined => Capability::limited(Status::Denied, Why::ConsentDeclined, &[]),
            Consent::Unsupported => {
                Capability::limited(Status::Unsupported, Why::NoScreencastPortal, &[])
            }
        }
    }
}

/// Whether the session's portal offers screen casts: `None` when it can't be told now.
fn offers_screencast() -> Option<bool> {
    let conn = Connection::new_session().ok()?;
    let version = conn
        .with_proxy(PORTAL, OBJECT, PROBE_TIMEOUT)
        .get::<u32>(SCREENCAST, "version");
    match version {
        Ok(_) => Some(true),
        Err(error) => match error.name() {
            // No portal at all, or one without the interface (its backend lacks it).
            Some(name)
                if ABSENT.contains(&name)
                    || name == "org.freedesktop.DBus.Error.UnknownInterface"
                    || name == "org.freedesktop.DBus.Error.UnknownProperty"
                    || name == "org.freedesktop.DBus.Error.InvalidArgs" =>
            {
                Some(false)
            }
            _ => None,
        },
    }
}

impl ScreenCapture for PortalCapture {
    fn monitors(&self) -> Result<Vec<Monitor>, SensorError> {
        let monitors = lock(&self.shared.monitors).clone();
        if monitors.is_empty() {
            Err(SensorError::PermissionDenied)
        } else {
            Ok(monitors)
        }
    }

    fn capture(&self, which: Monitors) -> Result<Vec<Frame>, SensorError> {
        let token = {
            let mut state = lock(&self.shared.state);
            if state.declined || state.busy {
                return Err(SensorError::PermissionDenied);
            }
            self.shared.load(&mut state);
            state.busy = true;
            state.token.clone()
        };
        let (sender, receiver) = sync_channel(1);
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("worklume-screencast".to_owned())
            .spawn(move || {
                let result = session(token.as_deref().map(String::as_str));
                let result = shared.settle(result);
                // Nobody waits any more when the member took long to answer.
                drop(sender.send(result));
            });
        if let Err(error) = spawned {
            lock(&self.shared.state).busy = false;
            return Err(SensorError::Failed(error.to_string()));
        }
        let mut frames = match receiver.recv_timeout(ANSWER_WITHIN) {
            Ok(result) => result?,
            // The member is being asked; the dialog stays up.
            Err(RecvTimeoutError::Timeout) => return Err(SensorError::PermissionDenied),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(SensorError::Failed("the capture stopped".to_owned()));
            }
        };
        if which == Monitors::Active {
            frames.truncate(1);
        }
        Ok(frames)
    }
}

impl Shared {
    /// Reads the token from the credential store, once. A store that fails reads as no token:
    /// the member is asked again.
    fn load(&self, state: &mut State) {
        if !state.loaded {
            state.token = self.secrets.get(TOKEN_SECRET).ok().flatten();
            state.loaded = true;
        }
    }

    /// Keeps what a session learned, and turns its outcome into the capture's.
    fn settle(
        &self,
        result: Result<(Vec<Frame>, Option<Zeroizing<String>>), Failure>,
    ) -> Result<Vec<Frame>, SensorError> {
        let mut state = lock(&self.state);
        state.busy = false;
        match result {
            Ok((frames, token)) => {
                if let Some(token) = token
                    && state.token.as_deref() != Some(&token)
                {
                    // Kept even if the store refuses: this run still captures without asking.
                    drop(self.secrets.set(TOKEN_SECRET, &token));
                    state.token = Some(token);
                }
                state.offered = Some(true);
                *lock(&self.monitors) = frames.iter().map(|frame| frame.monitor.clone()).collect();
                Ok(frames)
            }
            Err(Failure::Declined) => {
                state.declined = true;
                state.token = None;
                drop(self.secrets.delete(TOKEN_SECRET));
                Err(SensorError::PermissionDenied)
            }
            Err(Failure::Unsupported) => {
                state.offered = Some(false);
                Err(SensorError::Unsupported)
            }
            Err(Failure::Failed(error)) => Err(SensorError::Failed(error)),
        }
    }
}

impl fmt::Debug for PortalCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the token.
        f.debug_struct("PortalCapture")
            .field("consented", &self.consented())
            .finish_non_exhaustive()
    }
}

/// One capture: a session restored from `token`, or asked for; its frames, and the new token.
fn session(token: Option<&str>) -> Result<(Vec<Frame>, Option<Zeroizing<String>>), Failure> {
    let portal = Portal::connect()?;
    let created = portal.request(
        "CreateSession",
        |message| message,
        options([(
            "session_handle_token",
            Variant(Box::new(portal.token()) as Box<dyn RefArg>),
        )]),
        CALL_TIMEOUT,
    )?;
    let session = created
        .get("session_handle")
        .and_then(|handle| handle.0.as_str())
        .ok_or_else(|| Failure::Failed("the portal gave no session".to_owned()))?
        .to_owned();
    let result = portal.capture_in(&session, token);
    // Closing ends the screen sharing, and GNOME's indicator with it.
    drop(portal.call(&session, SESSION, "Close", |message| message));
    result
}

/// A connection to the portal, with its requests' paths.
struct Portal {
    conn: Connection,
    /// The connection's unique name, as request paths spell it.
    sender: String,
    next: std::cell::Cell<u32>,
}

impl Portal {
    fn connect() -> Result<Self, Failure> {
        let conn = Connection::new_session().map_err(|error| failure(&error))?;
        let sender = conn.unique_name().trim_start_matches(':').replace('.', "_");
        Ok(Self {
            conn,
            sender,
            next: std::cell::Cell::new(0),
        })
    }

    /// A fresh token for a request or session handle.
    fn token(&self) -> String {
        self.next.set(self.next.get() + 1);
        format!("worklume{}", self.next.get())
    }

    fn capture_in(
        &self,
        session: &str,
        token: Option<&str>,
    ) -> Result<(Vec<Frame>, Option<Zeroizing<String>>), Failure> {
        let mut select = options([
            ("types", Variant(Box::new(1_u32) as Box<dyn RefArg>)),
            ("multiple", Variant(Box::new(true))),
            ("persist_mode", Variant(Box::new(2_u32))),
        ]);
        if let Some(token) = token {
            select.insert(
                "restore_token".to_owned(),
                Variant(Box::new(token.to_owned())),
            );
        }
        self.request(
            "SelectSources",
            |message| message.append1(dbus::Path::from(session.to_owned())),
            select,
            CALL_TIMEOUT,
        )?;
        let started = self.request(
            "Start",
            |message| message.append2(dbus::Path::from(session.to_owned()), ""),
            PropMap::new(),
            DIALOG_WITHIN,
        )?;
        let new_token = started
            .get("restore_token")
            .and_then(|token| token.0.as_str())
            .map(|token| Zeroizing::new(token.to_owned()));
        let streams = streams(&started);
        if streams.is_empty() {
            return Err(Failure::Failed("the portal shared no monitor".to_owned()));
        }
        let (fd,): (OwnedFd,) = self
            .conn
            .with_proxy(PORTAL, OBJECT, CALL_TIMEOUT)
            .method_call(
                SCREENCAST,
                "OpenPipeWireRemote",
                (dbus::Path::from(session.to_owned()), PropMap::new()),
            )
            .map_err(|error| failure(&error))?;
        let frames = frames::grab(fd, &streams, FRAMES_WITHIN).map_err(Failure::Failed)?;
        Ok((frames, new_token))
    }

    /// Calls a portal method that answers through a request, and waits up to `within` for the
    /// answer. Past it the request is closed, which dismisses its dialog.
    fn request(
        &self,
        method: &str,
        arguments: impl FnOnce(Message) -> Message,
        mut options: PropMap,
        within: Duration,
    ) -> Result<PropMap, Failure> {
        let token = self.token();
        let path = format!("{OBJECT}/request/{}/{token}", self.sender);
        options.insert("handle_token".to_owned(), Variant(Box::new(token)));
        let answer: Arc<Mutex<Option<(u32, PropMap)>>> = Arc::new(Mutex::new(None));
        let rule = MatchRule::new_signal(REQUEST, "Response").with_path(path.clone());
        let matched = self
            .conn
            .add_match(rule, {
                let answer = Arc::clone(&answer);
                move |(code, results): (u32, PropMap), _, _| {
                    *lock(&answer) = Some((code, results));
                    true
                }
            })
            .map_err(|error| failure(&error))?;
        let call = Message::new_method_call(PORTAL, OBJECT, SCREENCAST, method)
            .map_err(Failure::Failed)?;
        let call = arguments(call).append1(options);
        let sent = self
            .conn
            .channel()
            .send_with_reply_and_block(call, CALL_TIMEOUT)
            .map_err(|error| failure(&error));
        let result = sent.and_then(|_| self.wait(&answer, &path, within));
        drop(self.conn.remove_match(matched));
        result
    }

    fn wait(
        &self,
        answer: &Mutex<Option<(u32, PropMap)>>,
        path: &str,
        within: Duration,
    ) -> Result<PropMap, Failure> {
        let deadline = Instant::now() + within;
        loop {
            if let Some((code, results)) = lock(answer).take() {
                return match code {
                    0 => Ok(results),
                    1 => Err(Failure::Declined),
                    _ => Err(Failure::Failed("the portal ended the request".to_owned())),
                };
            }
            if Instant::now() >= deadline {
                drop(self.call(path, REQUEST, "Close", |message| message));
                return Err(Failure::Declined);
            }
            self.conn
                .process(Duration::from_millis(100))
                .map_err(|error| failure(&error))?;
        }
    }

    /// Calls a method on one of the portal's objects, without an answer.
    fn call(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        arguments: impl FnOnce(Message) -> Message,
    ) -> Result<(), Failure> {
        let call =
            Message::new_method_call(PORTAL, path, interface, method).map_err(Failure::Failed)?;
        self.conn
            .channel()
            .send_with_reply_and_block(arguments(call), CALL_TIMEOUT)
            .map(drop)
            .map_err(|error| failure(&error))
    }
}

fn options<const N: usize>(entries: [(&str, Variant<Box<dyn RefArg>>); N]) -> PropMap {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

/// The streams in Start's answer: `a(ua{sv})`, each a node and its monitor's place.
fn streams(results: &PropMap) -> Vec<Stream> {
    let Some(streams) = results
        .get("streams")
        .and_then(|streams| streams.0.as_iter())
    else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for stream in streams {
        let Some(mut parts) = stream.as_iter() else {
            continue;
        };
        let (Some(node), Some(properties)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(node) = node.as_u64().and_then(|node| u32::try_from(node).ok()) else {
            continue;
        };
        let properties = dict(properties);
        let pair = |key: &str| {
            properties.get(key).and_then(|value| {
                let mut values = value.as_iter()?;
                let first = values.next()?.as_i64()?;
                let second = values.next()?.as_i64()?;
                Some((first, second))
            })
        };
        let (x, y) = pair("position").unwrap_or((0, 0));
        let Some((width, height)) = pair("size") else {
            continue;
        };
        let name = properties
            .get("id")
            .and_then(|id| id.as_str())
            .map_or_else(|| format!("stream {node}"), |id| format!("monitor {id}"));
        let fits = |value: i64| i32::try_from(value).ok();
        let size = |value: i64| u32::try_from(value).ok().filter(|value| *value > 0);
        if let (Some(x), Some(y), Some(width), Some(height)) =
            (fits(x), fits(y), size(width), size(height))
        {
            found.push(Stream {
                node,
                monitor: Monitor {
                    name,
                    x,
                    y,
                    width,
                    height,
                    primary: found.is_empty(),
                },
            });
        }
    }
    found
}

/// A dictionary argument's entries, by key; each value without its variant wrapper.
fn dict(arg: &dyn RefArg) -> HashMap<String, &dyn RefArg> {
    let mut entries = HashMap::new();
    let Some(mut items) = arg.as_iter() else {
        return entries;
    };
    while let (Some(key), Some(value)) = (items.next(), items.next()) {
        if let Some(key) = key.as_str() {
            let value = value
                .as_iter()
                .and_then(|mut inner| inner.next())
                .unwrap_or(value);
            entries.insert(key.to_owned(), value);
        }
    }
    entries
}

fn failure(error: &dbus::Error) -> Failure {
    match error.name() {
        Some(name) if ABSENT.contains(&name) => Failure::Unsupported,
        _ => Failure::Failed(format!(
            "the screen-sharing portal: {}",
            error.message().unwrap_or("unknown error")
        )),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use dbus::arg::{PropMap, RefArg, Variant};

    use super::{Stream, streams};
    use crate::Monitor;

    fn stream(node: u32, id: &str, position: (i32, i32), size: (i32, i32)) -> (u32, PropMap) {
        let mut properties = PropMap::new();
        properties.insert("id".to_owned(), Variant(Box::new(id.to_owned())));
        properties.insert("position".to_owned(), Variant(Box::new(position)));
        properties.insert("size".to_owned(), Variant(Box::new(size)));
        properties.insert("source_type".to_owned(), Variant(Box::new(1_u32)));
        (node, properties)
    }

    /// Start's answer as it arrives: through a D-Bus message, so the values are D-Bus's own.
    fn answer(streams: Vec<(u32, PropMap)>) -> PropMap {
        let mut results = PropMap::new();
        results.insert(
            "streams".to_owned(),
            Variant(Box::new(streams) as Box<dyn RefArg>),
        );
        let message = dbus::Message::new_signal("/a", "a.b", "C")
            .unwrap()
            .append2(0_u32, results);
        let (_, results): (u32, PropMap) = message.read2().unwrap();
        results
    }

    #[test]
    fn each_stream_gives_its_monitor() {
        let results = answer(vec![
            stream(42, "0", (0, 0), (1920, 1080)),
            stream(43, "1", (1920, -200), (1080, 1920)),
        ]);
        let found = streams(&results);
        let monitor = |name: &str, x, y, width, height, primary| Monitor {
            name: name.to_owned(),
            x,
            y,
            width,
            height,
            primary,
        };
        assert_eq!(
            found,
            [
                Stream {
                    node: 42,
                    monitor: monitor("monitor 0", 0, 0, 1920, 1080, true),
                },
                Stream {
                    node: 43,
                    monitor: monitor("monitor 1", 1920, -200, 1080, 1920, false),
                },
            ]
        );
    }

    #[test]
    fn streams_without_a_size_are_skipped() {
        let mut nameless = PropMap::new();
        nameless.insert("position".to_owned(), Variant(Box::new((0_i32, 0_i32))));
        let results = answer(vec![(7, nameless), stream(8, "x", (0, 0), (0, 10))]);
        assert!(streams(&results).is_empty());
        assert!(streams(&PropMap::new()).is_empty());
    }
}
