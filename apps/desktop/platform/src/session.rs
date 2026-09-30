//! Whether the member's session is locked (E12-S03).
//!
//! A locked session means the member is away. Input the OS still reports then is the lock
//! screen's: a password being typed. X11 delivers raw input to every client even while a screen
//! locker holds the keyboard (`docs/spikes/E12-S01-x11.md`). The timekeeper therefore pauses the
//! timer (reason `session_locked`) and keeps no input from locked time.
//!
//! The state is read on every tick rather than followed through events, as sleep is read from
//! the clock: a reading can't be missed while the agent is busy or restarting. Each OS answers
//! differently. On Linux, logind's `LockedHint` or the screen saver service (E12-S03); on macOS
//! and Windows, the adapters of E10 and E11. Until those arrive, [`crate::Unsupported`] stands in
//! and the session never reads as locked.

use std::fmt;

use crate::SensorError;

/// Whether the member's session is locked.
pub trait SessionLock: Send + Sync + fmt::Debug {
    /// Whether the session is locked now.
    ///
    /// # Errors
    ///
    /// Fails when this platform cannot tell ([`SensorError::Unsupported`]) or the reading failed
    /// this time. The caller then keeps the state it last knew.
    fn locked(&self) -> Result<bool, SensorError>;
}
