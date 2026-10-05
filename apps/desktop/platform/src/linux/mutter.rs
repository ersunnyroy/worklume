//! GNOME (E13-S04): idle time and active spans from Mutter's `IdleMonitor` on the session bus.
//! GNOME offers Wayland clients no idle protocol, but this service answers any unsandboxed
//! client without a prompt, and idle inhibitors (a video playing) don't affect it.
//!
//! - **Two watches.** An idle watch fires once the member has made no input for
//!   [`crate::IDLE_AFTER_MS`], and a "user active" watch fires once, at the next input, and is
//!   set again after each idle notice. So a burst of input costs two signals, and its span counts
//!   what the event meter counts ([`crate::ActivitySpans`]).
//! - **Exact idle time.** On each idle notice, and at the start, `GetIdletime` gives the moment
//!   of the last input. An idle notice whose last input is later than the one known means an
//!   input slipped in before the "user active" watch was set again; it counts its one second.
//! - **Only Mutter's signals count:** each must come from the bus name that owns the service. If
//!   Mutter leaves the bus, the next reading says so.
//! - **The reader** runs on a thread of its own with its own connection, and waits at most half a
//!   second for each message, so dropping it stops it promptly. A failed call or a lost bus is
//!   reported on the next reading. Signals are stamped with the agent's clock as they arrive.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use dbus::blocking::{Connection, Proxy};
use dbus::message::MatchRule;

use crate::spans::Transitions;
use crate::{ActiveSpan, ActivitySpans, Clock, IDLE_AFTER_MS, IdleError, IdleSource, SensorError};

const SERVICE: &str = "org.gnome.Mutter.IdleMonitor";
const PATH: &str = "/org/gnome/Mutter/IdleMonitor/Core";
/// How long a call may wait for Mutter. Only the reader thread waits; at login the Shell can
/// take seconds to answer.
const TIMEOUT: Duration = Duration::from_secs(5);
/// How many times setting the watches is tried when Mutter doesn't answer in time.
const TRIES: u32 = 3;
/// How long the reader waits for a message before checking whether it should stop.
const WAKE_EVERY: Duration = Duration::from_millis(500);
/// The idle watch's interval, as Mutter takes it.
const WATCH_MS: u64 = IDLE_AFTER_MS.unsigned_abs();

/// Idle time and active spans from Mutter. See the module documentation.
pub struct MutterIdle {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    transitions: Mutex<Transitions>,
    /// Watches that fired and when, waiting for the reader.
    fired: Mutex<VecDeque<(u32, i64)>>,
    /// Mutter left the bus: the Shell stopped.
    gone: AtomicBool,
    /// Why the reader stopped, once it has.
    failure: Mutex<Option<String>>,
    stop: AtomicBool,
}

/// The watches the reader has set.
#[derive(Default)]
struct Watches {
    idle: Option<u32>,
    /// The "user active" watch, while one is set.
    active: Option<u32>,
}

impl MutterIdle {
    /// Connects to the session bus and starts watching, stamping notices with `clock`.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] when Mutter's `IdleMonitor` isn't on the bus (not GNOME);
    /// [`SensorError::Failed`] when there is no session bus. Mutter not answering is reported
    /// by the readings, which are unknown until it does.
    pub fn connect(clock: Arc<dyn Clock>) -> Result<Self, SensorError> {
        let conn = Connection::new_session().map_err(|error| failed(&error))?;
        Self::start(conn, clock)
    }

    fn start(conn: Connection, clock: Arc<dyn Clock>) -> Result<Self, SensorError> {
        let bus = conn.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", TIMEOUT);
        let (present,): (bool,) = bus
            .method_call("org.freedesktop.DBus", "NameHasOwner", (SERVICE,))
            .map_err(|error| failed(&error))?;
        if !present {
            return Err(SensorError::Unsupported);
        }
        let (owner,): (String,) = bus
            .method_call("org.freedesktop.DBus", "GetNameOwner", (SERVICE,))
            .map_err(|error| failed(&error))?;
        let shared = Arc::new(Shared::default());
        conn.add_match(
            MatchRule::new_signal("org.freedesktop.DBus", "NameOwnerChanged")
                .with_sender("org.freedesktop.DBus"),
            {
                let shared = Arc::clone(&shared);
                move |(name, _, new_owner): (String, String, String), _, _| {
                    if name == SERVICE && new_owner.is_empty() {
                        shared.gone.store(true, Ordering::Relaxed);
                    }
                    true
                }
            },
        )
        .map_err(|error| failed(&error))?;
        // Signals first, so none fired after a watch is set is missed.
        conn.add_match(
            MatchRule::new_signal(SERVICE, "WatchFired").with_path(PATH),
            {
                let shared = Arc::clone(&shared);
                let clock = Arc::clone(&clock);
                move |(watch,): (u32,), _, message| {
                    if message.sender().as_deref() == Some(owner.as_str())
                        && let Ok(now) = clock.read()
                    {
                        lock(&shared.fired).push_back((watch, now.mono_ms));
                    }
                    true
                }
            },
        )
        .map_err(|error| failed(&error))?;
        let thread = std::thread::Builder::new()
            .name("worklume-mutter-idle".to_owned())
            .spawn({
                let shared = Arc::clone(&shared);
                let clock = Arc::clone(&clock);
                move || {
                    let mut watches = Watches::default();
                    let result = watch(&conn, &*clock, &shared, &mut watches)
                        .and_then(|()| read(&conn, &*clock, &shared, &mut watches));
                    let monitor = conn.with_proxy(SERVICE, PATH, TIMEOUT);
                    for watch in [watches.idle, watches.active].into_iter().flatten() {
                        let removed: Result<(), dbus::Error> =
                            monitor.method_call(SERVICE, "RemoveWatch", (watch,));
                        drop(removed);
                    }
                    if let Err(error) = result {
                        *lock(&shared.failure) = Some(error);
                    }
                }
            })
            .map_err(|error| failed(&error))?;
        Ok(Self {
            shared,
            clock,
            thread: Some(thread),
        })
    }

    fn failure(&self) -> Option<String> {
        lock(&self.shared.failure).clone()
    }
}

/// Sets the watches and reads the idle time, trying again while Mutter is too busy to answer.
fn watch(
    conn: &Connection,
    clock: &dyn Clock,
    shared: &Shared,
    watches: &mut Watches,
) -> Result<(), String> {
    let monitor = conn.with_proxy(SERVICE, PATH, TIMEOUT);
    let mut tries = 0;
    loop {
        tries += 1;
        let result = (|| {
            if watches.idle.is_none() {
                let (idle,): (u32,) = monitor.method_call(SERVICE, "AddIdleWatch", (WATCH_MS,))?;
                watches.idle = Some(idle);
            }
            if watches.active.is_none() {
                let (active,): (u32,) = monitor.method_call(SERVICE, "AddUserActiveWatch", ())?;
                watches.active = Some(active);
            }
            last_input(&monitor, clock)
        })();
        match result {
            Ok(input_at) => {
                let now = clock.read().map_err(|error| error.to_string())?.mono_ms;
                let mut transitions = lock(&shared.transitions);
                if now - input_at < IDLE_AFTER_MS {
                    transitions.active(input_at);
                } else {
                    transitions.idle_since(input_at);
                }
                return Ok(());
            }
            Err(error)
                if error.name() == Some("org.freedesktop.DBus.Error.NoReply")
                    && tries < TRIES
                    && !shared.stop.load(Ordering::Relaxed) => {}
            Err(error) => {
                return Err(format!(
                    "Mutter's idle monitor didn't answer: {}",
                    error.message().unwrap_or("unknown error")
                ));
            }
        }
    }
}

/// The reader's loop: waits for a signal, then acts on the watches that fired.
fn read(
    conn: &Connection,
    clock: &dyn Clock,
    shared: &Shared,
    watches: &mut Watches,
) -> Result<(), String> {
    let monitor = conn.with_proxy(SERVICE, PATH, TIMEOUT);
    let lost = |error: dbus::Error| {
        format!(
            "Mutter's idle monitor failed: {}",
            error.message().unwrap_or("unknown error")
        )
    };
    while !shared.stop.load(Ordering::Relaxed) {
        conn.process(WAKE_EVERY).map_err(lost)?;
        if shared.gone.load(Ordering::Relaxed) {
            return Err("Mutter's idle monitor left the bus".to_owned());
        }
        loop {
            let Some((watch, at)) = lock(&shared.fired).pop_front() else {
                break;
            };
            if Some(watch) == watches.idle {
                let input_at = last_input(&monitor, clock).map_err(lost)?;
                lock(&shared.transitions).idle_since(input_at);
                if watches.active.is_none() {
                    let (active,): (u32,) = monitor
                        .method_call(SERVICE, "AddUserActiveWatch", ())
                        .map_err(lost)?;
                    watches.active = Some(active);
                }
            } else if Some(watch) == watches.active {
                // A "user active" watch fires once.
                watches.active = None;
                lock(&shared.transitions).active(at);
            }
        }
    }
    Ok(())
}

/// The moment of the member's last input, on `clock`, from Mutter's idle time.
fn last_input(monitor: &Proxy<'_, &Connection>, clock: &dyn Clock) -> Result<i64, dbus::Error> {
    let (idle_ms,): (u64,) = monitor.method_call(SERVICE, "GetIdletime", ())?;
    let now = clock
        .read()
        .map_err(|error| dbus::Error::new_failed(&error.to_string()))?;
    Ok(now
        .mono_ms
        .saturating_sub(i64::try_from(idle_ms).unwrap_or(i64::MAX)))
}

impl IdleSource for MutterIdle {
    fn idle_for(&self) -> Result<Duration, IdleError> {
        if let Some(error) = self.failure() {
            return Err(IdleError::Failed(error));
        }
        let now = self
            .clock
            .read()
            .map_err(|error| IdleError::Failed(error.to_string()))?;
        lock(&self.shared.transitions).idle_for(now.mono_ms)
    }
}

impl ActivitySpans for MutterIdle {
    fn drain(&self) -> Result<Vec<ActiveSpan>, SensorError> {
        if let Some(error) = self.failure() {
            return Err(SensorError::Failed(error));
        }
        let now = self.clock.read().map_err(|error| failed(&error))?;
        Ok(lock(&self.shared.transitions).drain(now.mono_ms))
    }
}

impl Drop for MutterIdle {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // The reader wakes within `WAKE_EVERY` and sees the flag.
            drop(thread.join());
        }
    }
}

impl fmt::Debug for MutterIdle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MutterIdle").finish_non_exhaustive()
    }
}

fn failed(error: &dyn fmt::Display) -> SensorError {
    SensorError::Failed(error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
