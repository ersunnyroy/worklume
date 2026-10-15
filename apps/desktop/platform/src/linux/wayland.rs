//! Wayland (E13-S04): idle time and active spans from `ext-idle-notify-v1`, which KDE, wlroots
//! compositors such as sway, and others offer to any client without a prompt. GNOME
//! doesn't; see [`super::MutterIdle`]. What each compositor gives is in
//! `docs/spikes/E13-S01-wayland.md`.
//!
//! - **One notification, one second.** It reports "idle" once the member has made no input for
//!   [`crate::IDLE_AFTER_MS`] and "resumed" at the next input, so its spans count what the event
//!   meter counts. [`crate::ActivitySpans`] explains the spans.
//! - **Idle inhibitors.** Version 2 of the protocol has a notification that ignores them, and it
//!   is used when the compositor offers it. With version 1, a window that inhibits idling, such
//!   as a video playing, keeps the member active for as long as it is shown
//!   ([`WaylandIdle::ignores_inhibitors`]).
//! - **Startup.** The compositor says nothing until the member has been idle for a second, so
//!   idle time is unknown until then; with no "idle" by then, the member is active.
//! - **The reader** runs on a thread of its own with its own connection, and waits on the socket
//!   with a half-second timeout, so dropping it stops it promptly. A compositor that goes away
//!   is reported on the next reading. Notices are stamped with the agent's clock as they arrive.

use std::fmt;
use std::io::ErrorKind;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use wayland_client::backend::WaylandError;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1, ext_idle_notifier_v1,
};

use crate::spans::Transitions;
use crate::{ActiveSpan, ActivitySpans, Clock, IDLE_AFTER_MS, IdleError, IdleSource, SensorError};

/// The notification's timeout, as the protocol takes it.
const TIMEOUT_MS: u32 = 1_000;
const _: () = assert!(TIMEOUT_MS as i64 == IDLE_AFTER_MS);

/// How long the reader waits for the compositor before checking whether it should stop.
const WAKE_EVERY: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 500_000_000,
};

/// Idle time and active spans from `ext-idle-notify-v1`. See the module documentation.
pub struct WaylandIdle {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
    ignores_inhibitors: bool,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    transitions: Mutex<Transitions>,
    /// Why the reader stopped, once it has.
    failure: Mutex<Option<String>>,
    stop: AtomicBool,
}

/// The reader thread's state, which the event handlers see.
struct Reader {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
}

impl WaylandIdle {
    /// Connects to the compositor named by `WAYLAND_DISPLAY` and starts watching, stamping
    /// notices with `clock`.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] when the compositor has no `ext-idle-notify-v1` or no seat;
    /// [`SensorError::Failed`] when there is no compositor to connect to.
    pub fn connect(clock: Arc<dyn Clock>) -> Result<Self, SensorError> {
        let conn = Connection::connect_to_env().map_err(|error| {
            SensorError::Failed(format!("connecting to the Wayland compositor: {error}"))
        })?;
        Self::start(&conn, clock)
    }

    /// Connects to the compositor listening at `socket`, as [`WaylandIdle::connect`] does.
    ///
    /// # Errors
    ///
    /// As [`WaylandIdle::connect`].
    pub fn connect_to(socket: &Path, clock: Arc<dyn Clock>) -> Result<Self, SensorError> {
        let stream = UnixStream::connect(socket).map_err(failed)?;
        let conn = Connection::from_socket(stream).map_err(failed)?;
        Self::start(&conn, clock)
    }

    fn start(conn: &Connection, clock: Arc<dyn Clock>) -> Result<Self, SensorError> {
        let (globals, mut queue) = registry_queue_init::<Reader>(conn).map_err(failed)?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| SensorError::Unsupported)?;
        let notifier: ext_idle_notifier_v1::ExtIdleNotifierV1 = globals
            .bind(&qh, 1..=2, ())
            .map_err(|_| SensorError::Unsupported)?;
        let ignores_inhibitors = notifier.version() >= 2;
        let notification = if ignores_inhibitors {
            notifier.get_input_idle_notification(TIMEOUT_MS, &seat, &qh, ())
        } else {
            notifier.get_idle_notification(TIMEOUT_MS, &seat, &qh, ())
        };
        let shared = Arc::new(Shared::default());
        let mut reader = Reader {
            shared: Arc::clone(&shared),
            clock: Arc::clone(&clock),
        };
        let now = clock.read().map_err(failed)?.mono_ms;
        lock(&shared.transitions).watching(now);
        // The compositor has the request, or refused it.
        queue.roundtrip(&mut reader).map_err(failed)?;
        let thread = std::thread::Builder::new()
            .name("worklume-wayland-idle".to_owned())
            .spawn({
                let conn = conn.clone();
                move || {
                    let shared = Arc::clone(&reader.shared);
                    let result = pump(&conn, &mut queue, &mut reader, &shared.stop);
                    notification.destroy();
                    notifier.destroy();
                    drop(conn.flush());
                    if let Err(error) = result {
                        *lock(&reader.shared.failure) = Some(error);
                    }
                }
            })
            .map_err(failed)?;
        Ok(Self {
            shared,
            clock,
            ignores_inhibitors,
            thread: Some(thread),
        })
    }

    /// Whether idle inhibitors (a video playing) leave idle time alone. When they don't, the
    /// capability report says activity can read high while a video plays (E13-S02).
    #[must_use]
    pub const fn ignores_inhibitors(&self) -> bool {
        self.ignores_inhibitors
    }

    fn failure(&self) -> Option<String> {
        lock(&self.shared.failure).clone()
    }
}

impl IdleSource for WaylandIdle {
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

impl ActivitySpans for WaylandIdle {
    fn drain(&self) -> Result<Vec<ActiveSpan>, SensorError> {
        if let Some(error) = self.failure() {
            return Err(SensorError::Failed(error));
        }
        let now = self.clock.read().map_err(failed)?;
        Ok(lock(&self.shared.transitions).drain(now.mono_ms))
    }
}

impl Drop for WaylandIdle {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // The reader wakes within `WAKE_EVERY` and sees the flag.
            drop(thread.join());
        }
    }
}

impl fmt::Debug for WaylandIdle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WaylandIdle")
            .field("ignores_inhibitors", &self.ignores_inhibitors)
            .finish_non_exhaustive()
    }
}

/// A reader thread's loop: handles what the compositor has sent to `state`, then waits for more,
/// until `stop` is set. Shared with the Plasma window reader (`super::plasma`).
pub(super) fn pump<D: 'static>(
    conn: &Connection,
    queue: &mut EventQueue<D>,
    state: &mut D,
    stop: &AtomicBool,
) -> Result<(), String> {
    let lost = |error: &dyn fmt::Display| format!("the Wayland connection failed: {error}");
    while !stop.load(Ordering::Relaxed) {
        queue
            .dispatch_pending(state)
            .map_err(|error| lost(&error))?;
        conn.flush().map_err(|error| lost(&error))?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let ready = {
            let fd = guard.connection_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            match poll(&mut fds, Some(&WAKE_EVERY)) {
                Ok(ready) => ready > 0,
                Err(Errno::INTR) => false,
                Err(error) => return Err(format!("waiting for the compositor: {error}")),
            }
        };
        if ready {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(error)) if error.kind() == ErrorKind::WouldBlock => {}
                Err(error) => return Err(lost(&error)),
            }
        }
    }
    Ok(())
}

impl Dispatch<ext_idle_notification_v1::ExtIdleNotificationV1, ()> for Reader {
    fn event(
        reader: &mut Self,
        _: &ext_idle_notification_v1::ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // A notice the clock can't date is dropped rather than dated wrongly.
        let Ok(now) = reader.clock.read() else { return };
        let mut transitions = lock(&reader.shared.transitions);
        match event {
            ext_idle_notification_v1::Event::Idled => {
                transitions.idle_since(now.mono_ms.saturating_sub(IDLE_AFTER_MS));
            }
            ext_idle_notification_v1::Event::Resumed => transitions.active(now.mono_ms),
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Reader {
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

delegate_noop!(Reader: ignore wl_seat::WlSeat);
delegate_noop!(Reader: ext_idle_notifier_v1::ExtIdleNotifierV1);

fn failed(error: impl fmt::Display) -> SensorError {
    SensorError::Failed(error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
