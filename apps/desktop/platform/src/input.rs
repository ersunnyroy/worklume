//! Input events, as timing only: when an input happened and what kind it was (E09-S13).
//!
//! The per-OS hook readers arrive with the input stories: a `CGEvent` tap on macOS (Input
//! Monitoring) and Raw Input on Windows (E10-S03, E11-S03); X11's `XInput2` reader arrived with
//! E12-S03 (`linux::X11Input`). Wayland has no global input events, so its reader reports
//! [`SensorError::Unsupported`]. A lock screen's input still reaches a reader on X11; the
//! timekeeper drops input from locked time. A reader keeps
//! the kind and the monotonic time of each event, never which key, what text, or where the
//! pointer is, and buffers them until the agent drains them each tick.
//!
//! A reader stamps each event with the agent's clock ([`crate::Clock`]) when it arrives, not
//! with the OS event's own timestamp: those count on other clocks (macOS event times stop
//! during sleep). Its buffer is bounded, dropping the oldest events if the agent stops draining
//! it.

use std::fmt;

use crate::SensorError;

/// What kind of input happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputKind {
    /// A key went down. Which key is never recorded, and a key held down (auto-repeat) is not
    /// a new press.
    Key,
    /// The pointer moved. A moving mouse reports many times a second; each report is one event.
    Pointer,
    /// A mouse button or trackpad click.
    Click,
    /// A scroll wheel notch or a trackpad scroll report.
    Scroll,
}

/// One input event: its kind and when it happened, in the clock's monotonic milliseconds
/// (`worklume_platform::ClockReading::mono_ms`) of the current boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    pub mono_ms: i64,
    pub kind: InputKind,
}

/// The input events since the previous drain.
pub trait InputActivity: Send + Sync + fmt::Debug {
    /// The events since the previous call, oldest first. Events are kept only until then, so a
    /// reader holds at most one tick's worth.
    ///
    /// # Errors
    ///
    /// Fails when this session cannot observe input events, or the reader failed.
    fn drain(&self) -> Result<Vec<InputEvent>, SensorError>;
}
