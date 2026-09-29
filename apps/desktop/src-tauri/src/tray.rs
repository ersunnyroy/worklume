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

    /// Menu text. Localisation of the agent UI arrives with E09-S09.
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
    tray.build(app)?;
    Ok(())
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
