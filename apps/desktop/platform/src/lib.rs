//! Operating-system integration for the Worklume agent.
//!
//! Everything that talks to the OS lives here, behind traits the rest of the agent depends on:
//! screen capture, input activity, idle time, the active window, browser URLs, the tray,
//! autostart, notifications, secret storage, power events and network identity. Each OS gets an
//! adapter module (macOS, Windows, Linux X11, Linux Wayland), added by the per-OS epics.
//! Business rules never live here.
//!
//! So far the crate identifies the platform the agent runs on, which decides the adapter, keeps
//! secrets in the OS credential store, and reads the device's clocks. It also defines the idle
//! source, whose adapters come with the per-OS input stories.

mod clock;
mod detect;
#[cfg(feature = "fake")]
pub mod fake;
mod idle;
mod secret;

pub use clock::{Clock, ClockError, ClockReading, OsClock};
pub use detect::{LinuxSession, OperatingSystem, PlatformInfo, linux_session_from_env};
pub use idle::{IdleError, IdleSource};
pub use secret::{OsSecretStore, Secret, SecretStore, SecretStoreError, SecretStoreErrorKind};
