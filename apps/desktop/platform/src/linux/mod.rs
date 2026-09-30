//! Linux adapters (E12): idle time and input events on X11 (`x11`), and whether the session is
//! locked, from logind or the screen saver service (`lock`, shared with Wayland).
//!
//! What each API gives and costs is in `docs/spikes/E12-S01-x11.md`.

mod lock;
mod x11;

pub use lock::LinuxSessionLock;
pub use x11::{X11Idle, X11Input};
