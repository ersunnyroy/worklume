//! What the member has in front of them: the foreground application and, in a browser, the
//! active tab.
//!
//! The per-OS readers arrive with the OS stories:
//!
//! - macOS reads the front app through `NSWorkspace` (no permission), and titles and URLs
//!   through Accessibility and Automation.
//! - Windows uses `GetForegroundWindow`, with UI Automation for browser addresses.
//! - X11 uses EWMH `_NET_ACTIVE_WINDOW`, with AT-SPI or the browser extension for addresses.
//! - Wayland uses a GNOME Shell extension or a `KWin` script, with the browser extension.
//!
//! Until then only the test doubles implement these traits. Business rules, such as what the
//! organization lets the agent keep, live in the core, which asks only for what it may keep.

use std::fmt;

/// The foreground application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundApp {
    /// A stable identifier per OS: the bundle ID on macOS, the executable name on Windows, the
    /// desktop-file ID on Linux.
    pub identifier: String,
    /// The name people know it by.
    pub name: String,
    /// The focused window's title, only when asked for and the OS gives it.
    pub title: Option<String>,
}

/// The active tab of a browser in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTab {
    /// Which browser, in lower case: `chrome`, `edge`, `firefox`, `safari` and so on.
    pub browser: String,
    /// The address as the browser has it. The core reduces it to what the policy allows before
    /// anything is kept.
    pub url: String,
    /// The tab is in a private (incognito) window.
    pub private: bool,
}

/// Why a reading is unavailable. Unavailable is never read as "nothing in front".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SensorError {
    /// This session cannot read it, for example the active window on Wayland without the shell
    /// extension. The capability report says so (E09-S06).
    #[error("not available in this session")]
    Unsupported,
    /// The OS needs a permission the member hasn't granted, such as Accessibility on macOS.
    #[error("permission not granted")]
    PermissionDenied,
    /// The reading failed this time.
    #[error("reading failed: {0}")]
    Failed(String),
}

/// The foreground application.
pub trait ActiveWindow: Send + Sync + fmt::Debug {
    /// The application in front, or `None` when nothing is (the desktop, a lock screen). The
    /// window title is read only when `with_title` is set, since some OSes need a permission
    /// for it and the policy may not keep it.
    ///
    /// # Errors
    ///
    /// Fails when the OS cannot say.
    fn foreground(&self, with_title: bool) -> Result<Option<ForegroundApp>, SensorError>;
}

/// The active tab of a browser.
pub trait BrowserUrl: Send + Sync + fmt::Debug {
    /// The active tab of `app`, or `None` when `app` is not a browser this reader knows.
    ///
    /// # Errors
    ///
    /// Fails when `app` is a browser but its address cannot be read.
    fn active_tab(&self, app: &ForegroundApp) -> Result<Option<BrowserTab>, SensorError>;
}
