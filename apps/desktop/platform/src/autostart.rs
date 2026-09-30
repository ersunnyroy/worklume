//! Starting the agent when the member logs in (E12-S05, E10-S07, E11-S05).
//!
//! Each OS keeps its own switch: an XDG autostart entry on Linux (E12-S05), a login item on
//! macOS (E10-S07), the Run key on Windows (E11-S05). Whether the agent should start at login is
//! the organization's `auto-start` policy, delivered with the configuration (E09-S06); the member
//! may change it where the policy allows (the settings screen, E09-S09 part 2). Until an OS's
//! adapter arrives, [`crate::Unsupported`] stands in.

use std::fmt;

/// The argument a launch at login carries, on every OS: such a launch never shows the window,
/// even when the agent is already running.
pub const LAUNCHED_AT_LOGIN: &str = "--autostart";

/// Whether the agent starts at login.
pub trait Autostart: Send + Sync + fmt::Debug {
    /// Whether the agent will start at the member's next login.
    ///
    /// # Errors
    ///
    /// Fails when the OS can't say.
    fn enabled(&self) -> Result<bool, AutostartError>;

    /// Makes the agent start at login, or not.
    ///
    /// # Errors
    ///
    /// Fails when the OS refuses the change; nothing changed then.
    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError>;
}

/// Why starting at login couldn't be read or changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AutostartError {
    /// This platform's adapter hasn't arrived yet.
    #[error("starting at login can't be managed here")]
    Unsupported,
    #[error("starting at login: {0}")]
    Failed(String),
}
