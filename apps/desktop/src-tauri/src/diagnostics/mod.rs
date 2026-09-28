//! Diagnostics: structured log files and crash reports (E09-S11). Both follow the agent's
//! privacy rule: failures are recorded without personal data. Paths under the member's home
//! folder are shortened to `~`, and nothing identifies the machine or the member.

pub mod crash_reports;
pub mod logging;
mod redact;

use std::path::PathBuf;

pub use redact::{HomeRedactor, RedactingMakeWriter};

/// The app identifier from `tauri.conf.json`. The supervisor needs it before any Tauri context
/// exists; a test keeps the two equal.
pub const APP_IDENTIFIER: &str = "com.worklume.agent";

/// Which process is writing: the supervisor and the agent keep separate log files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessKind {
    Supervisor,
    Agent,
}

impl ProcessKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supervisor => "supervisor",
            Self::Agent => "agent",
        }
    }
}

/// The folder Tauri's `app_log_dir` resolves to, computed without a Tauri context: on macOS
/// `~/Library/Logs/<identifier>`, elsewhere `<local data>/<identifier>/logs`.
#[must_use]
pub fn log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| home.join("Library/Logs").join(APP_IDENTIFIER))
    } else {
        dirs::data_local_dir().map(|dir| dir.join(APP_IDENTIFIER).join("logs"))
    }
}
