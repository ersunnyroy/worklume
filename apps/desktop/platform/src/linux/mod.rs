//! Linux adapters (E12, E13).
//!
//! - X11 (`x11`): idle time, input events, the window in front and screen capture.
//! - Wayland: idle time and active spans from `ext-idle-notify-v1` (`wayland`: KDE, wlroots
//!   and others) or from Mutter's `IdleMonitor` (`mutter`: GNOME); on GNOME, the window in front
//!   from the Worklume Shell extension (`gnome`); screen capture through the desktop portal's
//!   screen casts (`portal`).
//! - Both: which installed app a window belongs to (`desktop`); whether the session is locked,
//!   from logind or the screen saver service (`lock`); and the XDG autostart entry
//!   (`autostart`).
//!
//! What each API gives and costs is in `docs/spikes/E12-S01-x11.md` and
//! `docs/spikes/E13-S01-wayland.md`.

mod autostart;
mod bus;
mod desktop;
mod gnome;
mod lock;
mod mutter;
mod portal;
mod wayland;
mod x11;

pub use autostart::{ENTRY_NAME, XdgAutostart};
pub use desktop::{App, DesktopFiles, Hints};
pub use gnome::{EXTENSION_UUID, ExtensionState, GnomeWindow};
pub use lock::LinuxSessionLock;
pub use mutter::MutterIdle;
pub use portal::{ANSWER_WITHIN, Consent, PortalCapture, TOKEN_SECRET};
pub use wayland::WaylandIdle;
pub use x11::{X11Capture, X11Idle, X11Input, X11Window};
