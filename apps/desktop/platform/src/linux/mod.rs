//! Linux adapters (E12): idle time, input events, the window in front and screen capture on X11
//! (`x11`), which
//! installed app a window belongs to (`desktop`), and whether the session is locked, from logind
//! or the screen saver service (`lock`, shared with Wayland); and the XDG autostart entry
//! (`autostart`).
//!
//! What each API gives and costs is in `docs/spikes/E12-S01-x11.md`.

mod autostart;
mod desktop;
mod lock;
mod x11;

pub use autostart::{ENTRY_NAME, XdgAutostart};
pub use desktop::{App, DesktopFiles, Hints};
pub use lock::LinuxSessionLock;
pub use x11::{X11Capture, X11Idle, X11Input, X11Window};
