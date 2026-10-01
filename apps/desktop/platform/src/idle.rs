//! How long the member has been away from the keyboard and mouse.
//!
//! Each OS answers the same question, "how long since the last input?", with no special
//! permission: `CGEventSourceSecondsSinceLastEventType` on macOS, `GetLastInputInfo` on Windows,
//! the `XScreenSaver` extension on X11, and `ext-idle-notify-v1` or Mutter's `IdleMonitor` on
//! Wayland. X11's arrived with E12-S03 (`linux::X11Idle`); the others come with
//! E10-S03, E11-S03 and E13-S04.

use std::fmt;
use std::time::Duration;

/// The time since the member's last keyboard or mouse input.
pub trait IdleSource: Send + Sync + fmt::Debug {
    /// How long ago the last input happened.
    ///
    /// # Errors
    ///
    /// Fails when the OS cannot say. The caller must then treat idle time as unknown, never as
    /// zero or as growing.
    fn idle_for(&self) -> Result<Duration, IdleError>;
}

/// Why idle time is unknown.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdleError {
    /// This session cannot measure idle time, for example a Wayland compositor without an idle
    /// protocol. The capability report says so (E09-S06).
    #[error("idle time cannot be measured in this session")]
    Unsupported,
    /// The measurement failed this time.
    #[error("reading idle time failed: {0}")]
    Failed(String),
}
