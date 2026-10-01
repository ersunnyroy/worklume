//! Linux adapters (E12): idle time, input events and the window in front on X11 (`x11`), which
//! installed app a window belongs to (`desktop`), and whether the session is locked, from logind
//! or the screen saver service (`lock`, shared with Wayland).
//!
//! What each API gives and costs is in `docs/spikes/E12-S01-x11.md`.

mod desktop;
mod lock;
mod x11;

pub use desktop::{App, DesktopFiles, Hints};
pub use lock::LinuxSessionLock;
pub use x11::{X11Idle, X11Input, X11Window};
