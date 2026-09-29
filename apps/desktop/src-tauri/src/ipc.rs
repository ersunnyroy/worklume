//! Commands the webview may call.
//!
//! Every command is listed three times, and Tauri's ACL refuses anything missing from any of
//! them: in [`builder`] (registration and TypeScript bindings), in `build.rs` (the app manifest)
//! and in `capabilities/main.json` (the permission granted to the main window).

use serde::Serialize;
use specta::Type;
use tauri::Runtime;
use tauri_specta::{Builder, collect_commands};
use worklume_platform::{LinuxSession, OperatingSystem, PlatformInfo};

/// Registers the commands; also used to generate `frontend/src/ipc/bindings.gen.ts`.
#[must_use]
pub fn builder<R: Runtime>() -> Builder<R> {
    Builder::<R>::new().commands(collect_commands![agent_status])
}

/// What the agent is and where it runs, for the UI's about section and support requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AgentStatus {
    pub version: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Platform {
    pub os: Os,
    /// Present on Linux only.
    pub linux_session: Option<DisplaySession>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DisplaySession {
    X11,
    Wayland,
    Unknown,
}

impl From<PlatformInfo> for Platform {
    fn from(info: PlatformInfo) -> Self {
        Self {
            os: match info.os {
                OperatingSystem::MacOs => Os::Macos,
                OperatingSystem::Windows => Os::Windows,
                OperatingSystem::Linux => Os::Linux,
            },
            linux_session: info.linux_session.map(|session| match session {
                LinuxSession::X11 => DisplaySession::X11,
                LinuxSession::Wayland => DisplaySession::Wayland,
                LinuxSession::Unknown => DisplaySession::Unknown,
            }),
        }
    }
}

/// The agent's version and platform.
#[tauri::command]
#[specta::specta]
#[must_use]
pub fn agent_status() -> AgentStatus {
    AgentStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: PlatformInfo::current().into(),
    }
}
