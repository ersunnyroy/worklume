use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};

/// Label of the agent's only window, as declared in `tauri.conf.json`.
pub const MAIN_WINDOW: &str = "main";

/// Keeps the agent running when its window is closed: the close is cancelled and the window
/// hidden. Quitting happens only from the tray menu.
///
/// This runs from the app's event loop callback rather than a per-window listener so it behaves
/// the same under the real runtime and Tauri's mock runtime used in tests.
pub fn on_run_event<R: Runtime>(app: &AppHandle<R>, event: &RunEvent) {
    if let RunEvent::WindowEvent {
        label,
        event: WindowEvent::CloseRequested { api, .. },
        ..
    } = event
    {
        if label != MAIN_WINDOW {
            return;
        }
        api.prevent_close();
        if let Some(window) = app.get_webview_window(label) {
            // Hiding can only fail if the window is already gone, which leaves nothing to do.
            // Failure logging arrives with E09-S11.
            window.hide().ok();
        }
    }
}

/// Brings the main window to the front, restoring it if it was hidden or minimised.
pub fn show_main<R: Runtime, M: Manager<R>>(manager: &M) {
    if let Some(window) = manager.get_webview_window(MAIN_WINDOW) {
        // Each step is best effort: a window that cannot be focused is still shown.
        window.unminimize().ok();
        window.show().ok();
        window.set_focus().ok();
    }
}
