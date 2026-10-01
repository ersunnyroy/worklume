//! The tray icon and its menu (E09-S01). On Linux it is a `StatusNotifierItem` through
//! `libayatana-appindicator`, which stock Ubuntu shows through its `AppIndicator` extension
//! (E12-S05).

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Runtime};

use crate::window;

/// Commands offered by the tray menu. Menu item IDs round-trip through [`TrayAction::id`] and
/// [`TrayAction::from_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    Quit,
}

impl TrayAction {
    /// Every action, in menu order.
    pub const ALL: [Self; 2] = [Self::Open, Self::Quit];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Quit => "quit",
        }
    }

    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    /// Menu text, in English like the window: both are localised together when a second
    /// language arrives.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Open => "Open Worklume",
            Self::Quit => "Quit Worklume",
        }
    }
}

/// Adds the Worklume icon and menu to the system tray or menu bar.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = menu_item(app, TrayAction::Open)?;
    let quit = menu_item(app, TrayAction::Quit)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut tray = TrayIconBuilder::with_id("worklume")
        .tooltip("Worklume")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_menu_event(app, &event));
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    #[cfg(target_os = "linux")]
    if let Some(dir) = linux_icon_dir() {
        tray = tray.temp_dir_path(dir);
    }
    tray.build(app)?;
    Ok(())
}

/// Where the tray icon's image goes on Linux, whose trays take a file: a folder only the member
/// can open. Without `XDG_RUNTIME_DIR` the tray library falls back to a shared
/// `/tmp/tray-icon`, which another user may own, or hold a planted link in.
#[cfg(target_os = "linux")]
fn linux_icon_dir() -> Option<std::path::PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let dir = dirs::runtime_dir()
        .or_else(dirs::cache_dir)?
        .join(crate::diagnostics::APP_IDENTIFIER)
        .join("tray");
    let private = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .and_then(|()| std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)));
    match private {
        Ok(()) => Some(dir),
        Err(error) => {
            tracing::warn!(%error, "no private folder for the tray icon; using the library's");
            None
        }
    }
}

fn menu_item<R: Runtime>(app: &AppHandle<R>, action: TrayAction) -> tauri::Result<MenuItem<R>> {
    MenuItem::with_id(app, action.id(), action.label(), true, None::<&str>)
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    match TrayAction::from_id(event.id().as_ref()) {
        Some(TrayAction::Open) => window::show_main(app),
        Some(TrayAction::Quit) => app.exit(0),
        None => {}
    }
}
