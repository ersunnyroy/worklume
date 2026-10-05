//! The sensors this session offers: each an OS adapter where one exists and starts, otherwise
//! [`Unsupported`], so a missing capability is reported rather than guessed.
//!
//! - Linux X11: idle time and input events (E12-S03), the window in front (E12-S04) and screen
//!   capture (E12-S02); the session lock from logind or the screen saver (E12-S03, shared with
//!   Wayland).
//! - Linux Wayland: idle time and active spans (E13-S04), from Mutter's `IdleMonitor` on GNOME,
//!   else from `ext-idle-notify-v1`; on GNOME, the window in front from the Worklume extension
//!   (E13-S05), which the agent turns on. Never the X11 adapters, though `DISPLAY` is set for
//!   Xwayland: through it they would see only X11 apps.
//! - Everything else waits for its story: browser addresses (E12-S04 with E35, E10-S04,
//!   E11-S04), the macOS and Windows front app (E10-S04, E11-S04), and their input and locks
//!   (E10-S03, E11-S03).

use std::sync::Arc;

use crate::{
    ActiveWindow, ActivitySpans, BrowserUrl, Clock, IdleSource, InputActivity, ScreenCapture,
    SessionLock, Unsupported,
};

/// The sensors of this session, and what couldn't start.
pub struct DeviceSensors {
    pub idle: Arc<dyn IdleSource>,
    pub window: Arc<dyn ActiveWindow>,
    pub browser: Arc<dyn BrowserUrl>,
    pub input: Arc<dyn InputActivity>,
    /// Active spans, where there are no input events (Wayland).
    pub spans: Arc<dyn ActivitySpans>,
    pub lock: Arc<dyn SessionLock>,
    /// Screenshots, for the screenshot pipeline (E09-S07).
    pub capture: Arc<dyn ScreenCapture>,
    /// Input is read event by event (hooks) rather than from idle time alone.
    pub input_hooks: bool,
    /// Activity comes from active spans (a compositor's idle notifications).
    pub activity_spans: bool,
    /// An adapter that exists here but didn't start, and why: for the log.
    pub problems: Vec<String>,
    /// What an adapter that started can't measure well: for the log, and the capability report
    /// (E13-S02).
    pub limits: Vec<String>,
}

impl DeviceSensors {
    /// Starts this session's adapters. Input events are stamped with `clock`.
    #[must_use]
    pub fn start(clock: &Arc<dyn Clock>) -> Self {
        let mut sensors = Self {
            idle: Arc::new(Unsupported),
            window: Arc::new(Unsupported),
            browser: Arc::new(Unsupported),
            input: Arc::new(Unsupported),
            spans: Arc::new(Unsupported),
            lock: Arc::new(Unsupported),
            capture: Arc::new(Unsupported),
            input_hooks: false,
            activity_spans: false,
            problems: Vec::new(),
            limits: Vec::new(),
        };
        sensors.start_os(clock);
        sensors
    }

    #[cfg(target_os = "linux")]
    fn start_os(&mut self, clock: &Arc<dyn Clock>) {
        use crate::linux::{LinuxSessionLock, X11Capture, X11Idle, X11Input, X11Window};
        use crate::{LinuxSession, PlatformInfo};

        self.lock = Arc::new(LinuxSessionLock::new());
        match PlatformInfo::current().linux_session {
            Some(LinuxSession::X11) => {}
            Some(LinuxSession::Wayland) => return self.start_wayland(clock),
            _ => return,
        }
        match X11Idle::connect() {
            Ok(idle) => self.idle = Arc::new(idle),
            Err(error) => self.problems.push(format!("idle time: {error}")),
        }
        match X11Window::connect() {
            Ok(window) => self.window = Arc::new(window),
            Err(error) => self.problems.push(format!("the window in front: {error}")),
        }
        match X11Capture::connect() {
            Ok(capture) => self.capture = Arc::new(capture),
            Err(error) => self.problems.push(format!("screen capture: {error}")),
        }
        match X11Input::start(Arc::clone(clock)) {
            Ok(input) => {
                self.input = Arc::new(input);
                self.input_hooks = true;
            }
            Err(error) => self.problems.push(format!("input events: {error}")),
        }
    }

    /// GNOME's Mutter first, which idle inhibitors don't affect; then the Wayland protocol.
    #[cfg(target_os = "linux")]
    fn start_wayland(&mut self, clock: &Arc<dyn Clock>) {
        use crate::SensorError;
        use crate::linux::{MutterIdle, WaylandIdle};

        let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| {
            desktops
                .split(':')
                .any(|desktop| desktop.eq_ignore_ascii_case("GNOME"))
        });
        let mutter = match MutterIdle::connect(Arc::clone(clock), gnome) {
            Ok(mutter) => Some(Arc::new(mutter)),
            Err(SensorError::Unsupported) => None,
            Err(error) => {
                self.problems
                    .push(format!("idle time from Mutter: {error}"));
                None
            }
        };
        if let Some(mutter) = mutter {
            self.idle = Arc::clone(&mutter) as _;
            self.spans = mutter;
            self.activity_spans = true;
            self.start_gnome_window();
            return;
        }
        match WaylandIdle::connect(Arc::clone(clock)) {
            Ok(wayland) => {
                if !wayland.ignores_inhibitors() {
                    self.limits.push(
                        "activity: the compositor's idle notices stop while a window inhibits \
                         idling, so a video playing counts as active"
                            .to_owned(),
                    );
                }
                let wayland = Arc::new(wayland);
                self.idle = Arc::clone(&wayland) as _;
                self.spans = wayland;
                self.activity_spans = true;
            }
            Err(error) => self.problems.push(format!("idle time on Wayland: {error}")),
        }
    }

    /// GNOME's window in front, from the Worklume extension, which the reader turns on once
    /// GNOME Shell knows it (see `linux::GnomeWindow`).
    #[cfg(target_os = "linux")]
    fn start_gnome_window(&mut self) {
        self.window = Arc::new(crate::linux::GnomeWindow::new());
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(
        clippy::unused_self,
        clippy::missing_const_for_fn,
        reason = "the macOS and Windows adapters arrive with E10 and E11"
    )]
    fn start_os(&mut self, _clock: &Arc<dyn Clock>) {}
}

impl std::fmt::Debug for DeviceSensors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSensors")
            .field("input_hooks", &self.input_hooks)
            .field("problems", &self.problems)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
