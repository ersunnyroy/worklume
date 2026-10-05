//! The sensors this session offers: each an OS adapter where one exists and starts, otherwise
//! [`Unsupported`], so a missing capability is reported rather than guessed.
//!
//! - Linux X11: idle time and input events (E12-S03), the window in front (E12-S04) and screen
//!   capture (E12-S02); the session lock from logind or the screen saver (E12-S03, shared with
//!   Wayland).
//! - Everything else waits for its story: browser addresses (E12-S04 with E35, E10-S04,
//!   E11-S04), the macOS and Windows front app (E10-S04, E11-S04), and their input and locks
//!   (E10-S03, E11-S03).

use std::sync::Arc;

use crate::{
    ActiveWindow, BrowserUrl, Clock, IdleSource, InputActivity, ScreenCapture, SessionLock,
    Unsupported,
};

/// The sensors of this session, and what couldn't start.
pub struct DeviceSensors {
    pub idle: Arc<dyn IdleSource>,
    pub window: Arc<dyn ActiveWindow>,
    pub browser: Arc<dyn BrowserUrl>,
    pub input: Arc<dyn InputActivity>,
    pub lock: Arc<dyn SessionLock>,
    /// Screenshots, for the screenshot pipeline (E09-S07).
    pub capture: Arc<dyn ScreenCapture>,
    /// Input is read event by event (hooks) rather than from idle time alone.
    pub input_hooks: bool,
    /// An adapter that exists here but didn't start, and why: for the log.
    pub problems: Vec<String>,
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
            lock: Arc::new(Unsupported),
            capture: Arc::new(Unsupported),
            input_hooks: false,
            problems: Vec::new(),
        };
        sensors.start_os(clock);
        sensors
    }

    #[cfg(target_os = "linux")]
    fn start_os(&mut self, clock: &Arc<dyn Clock>) {
        use crate::linux::{LinuxSessionLock, X11Capture, X11Idle, X11Input, X11Window};
        use crate::{LinuxSession, PlatformInfo};

        self.lock = Arc::new(LinuxSessionLock::new());
        if PlatformInfo::current().linux_session != Some(LinuxSession::X11) {
            return;
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
            .finish_non_exhaustive()
    }
}
