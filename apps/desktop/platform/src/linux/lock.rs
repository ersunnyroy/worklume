//! Whether the session is locked, on Linux (E12-S03): the same on X11 and Wayland.
//!
//! **logind** knows first. Screen lockers set `LockedHint` on the member's session (GNOME, KDE
//! and others do). The session is `/org/freedesktop/login1/session/auto`: the caller's own
//! session, or the member's graphical one when the agent runs outside it, as a systemd user
//! service does.
//!
//! **The screen saver service** on the session bus answers where logind can't
//! (`org.freedesktop.ScreenSaver`, then GNOME's `org.gnome.ScreenSaver`). Active means locked or
//! blanked; either way the member is away. A service that isn't running is never started by a
//! reading.
//!
//! Each reading is one D-Bus call. A call that fails drops its connection, so the next reading
//! connects again.

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use dbus::Message;
use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;

use crate::{SensorError, SessionLock};

/// How long a reading may wait for an answer. A tick must not stall behind a busy bus.
const TIMEOUT: Duration = Duration::from_millis(250);

const LOGIND: &str = "org.freedesktop.login1";
const LOGIND_SESSION: &str = "/org/freedesktop/login1/session/auto";
const SCREEN_SAVERS: [(&str, &str); 2] = [
    (
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
    ),
    ("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver"),
];
/// D-Bus errors meaning the service isn't there, rather than that it failed.
const ABSENT: [&str; 4] = [
    "org.freedesktop.DBus.Error.ServiceUnknown",
    "org.freedesktop.DBus.Error.NameHasNoOwner",
    "org.freedesktop.DBus.Error.UnknownObject",
    "org.freedesktop.DBus.Error.UnknownMethod",
];

type Connect = Box<dyn Fn() -> Result<Connection, dbus::Error> + Send>;

/// A bus connected on first use, and again after a failure.
struct Bus {
    connect: Connect,
    conn: Option<Connection>,
}

impl Bus {
    fn new(connect: Connect) -> Self {
        Self {
            connect,
            conn: None,
        }
    }

    /// Runs `call` on the connection; a failed call drops it.
    fn call<T>(
        &mut self,
        call: impl FnOnce(&Connection) -> Result<T, dbus::Error>,
    ) -> Result<T, dbus::Error> {
        if self.conn.is_none() {
            self.conn = Some((self.connect)()?);
        }
        let result = self
            .conn
            .as_ref()
            .map_or_else(|| Err(dbus::Error::new_failed("not connected")), call);
        if result.is_err() {
            self.conn = None;
        }
        result
    }
}

/// Whether the member's Linux session is locked. See the module documentation.
pub struct LinuxSessionLock {
    buses: Mutex<(Bus, Bus)>,
}

impl LinuxSessionLock {
    /// Reads logind on the system bus and the screen saver on the session bus. Nothing connects
    /// until the first reading.
    #[must_use]
    pub fn new() -> Self {
        Self::on(
            Box::new(Connection::new_system),
            Box::new(Connection::new_session),
        )
    }

    /// Reads logind through `system` and the screen saver through `session`.
    fn on(system: Connect, session: Connect) -> Self {
        Self {
            buses: Mutex::new((Bus::new(system), Bus::new(session))),
        }
    }
}

impl Default for LinuxSessionLock {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLock for LinuxSessionLock {
    fn locked(&self) -> Result<bool, SensorError> {
        let mut buses = lock(&self.buses);
        let (system, session) = &mut *buses;
        let logind = system.call(|conn| {
            conn.with_proxy(LOGIND, LOGIND_SESSION, TIMEOUT)
                .get::<bool>("org.freedesktop.login1.Session", "LockedHint")
        });
        let mut errors = match logind {
            Ok(locked) => return Ok(locked),
            Err(error) => vec![error],
        };
        for (service, path) in SCREEN_SAVERS {
            let active = session.call(|conn| {
                let mut call = Message::new_method_call(service, path, service, "GetActive")
                    .map_err(|error| dbus::Error::new_failed(&error))?;
                // A reading never starts a screen saver that isn't running: its service file may
                // be installed (GNOME Shell ships one) on a desktop that doesn't use it.
                call.set_auto_start(false);
                let reply = conn.channel().send_with_reply_and_block(call, TIMEOUT)?;
                Ok(reply.read1::<bool>()?)
            });
            match active {
                Ok(active) => return Ok(active),
                Err(error) => errors.push(error),
            }
        }
        if errors
            .iter()
            .all(|error| error.name().is_none_or(|name| ABSENT.contains(&name)))
        {
            Err(SensorError::Unsupported)
        } else {
            let detail = errors
                .iter()
                .map(|error| error.message().unwrap_or("unknown error"))
                .collect::<Vec<_>>()
                .join("; ");
            Err(SensorError::Failed(format!(
                "reading the session lock: {detail}"
            )))
        }
    }
}

impl fmt::Debug for LinuxSessionLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinuxSessionLock").finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    //! Against stand-in services on the session bus. They need one: run with `--ignored` under
    //! `dbus-run-session`, as CI does on Linux.

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use dbus::arg::Variant;
    use dbus::blocking::Connection;
    use dbus::channel::{MatchingReceiver, Sender};
    use dbus::message::MatchRule;

    use super::{LOGIND, LOGIND_SESSION, LinuxSessionLock};
    use crate::{SensorError, SessionLock};

    /// A service owning `name` on the session bus that answers whether the session is locked,
    /// as logind (`LockedHint`) or as a screen saver (`GetActive`), until dropped.
    struct StandIn {
        locked: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl StandIn {
        fn start(name: &'static str, path: &'static str) -> Self {
            let locked = Arc::new(AtomicBool::new(false));
            let stop = Arc::new(AtomicBool::new(false));
            let (ready_tx, ready_rx) = std::sync::mpsc::channel();
            let thread = std::thread::spawn({
                let locked = Arc::clone(&locked);
                let stop = Arc::clone(&stop);
                move || {
                    let conn = Connection::new_session().expect("a session bus");
                    conn.request_name(name, false, true, true)
                        .expect("the name is free");
                    conn.start_receive(
                        MatchRule::new_method_call(),
                        Box::new(move |call, conn| {
                            let is_locked = locked.load(Ordering::SeqCst);
                            let reply = match call.member().as_deref() {
                                _ if call.path().as_deref() != Some(path) => call.error(
                                    &"org.freedesktop.DBus.Error.UnknownObject".into(),
                                    c"no such object",
                                ),
                                Some("Get") => call.method_return().append1(Variant(is_locked)),
                                Some("GetActive") => call.method_return().append1(is_locked),
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
                        drop(conn.process(Duration::from_millis(50)));
                    }
                }
            });
            ready_rx.recv().expect("the stand-in started");
            Self {
                locked,
                stop,
                thread: Some(thread),
            }
        }

        fn set(&self, locked: bool) {
            self.locked.store(locked, Ordering::SeqCst);
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

    /// Reads logind and the screen saver both on the session bus.
    fn reader() -> LinuxSessionLock {
        LinuxSessionLock::on(
            Box::new(Connection::new_session),
            Box::new(Connection::new_session),
        )
    }

    #[test]
    #[ignore = "needs a D-Bus session bus: run under dbus-run-session with --ignored"]
    fn follows_logind_and_falls_back_to_the_screen_saver() {
        let lock = reader();
        // Nothing answers yet.
        assert_eq!(lock.locked(), Err(SensorError::Unsupported));

        let logind = StandIn::start(LOGIND, LOGIND_SESSION);
        assert_eq!(lock.locked(), Ok(false));
        logind.set(true);
        assert_eq!(lock.locked(), Ok(true));
        logind.set(false);
        assert_eq!(lock.locked(), Ok(false));
        drop(logind);

        // Without logind, a screen saver answers; the reader reconnects after the failure.
        let saver = StandIn::start("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver");
        saver.set(true);
        assert_eq!(lock.locked(), Ok(true));
        saver.set(false);
        assert_eq!(lock.locked(), Ok(false));
    }
}
