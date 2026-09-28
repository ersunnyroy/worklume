//! Worklume desktop agent shell: a tray-resident Tauri application.
//!
//! The window starts hidden and closing it only hides it, so the agent keeps running from the
//! tray. Tracking, storage and sync arrive as separate crates in E09.

mod tray;
mod window;

pub use tray::TrayAction;
pub use window::{MAIN_WINDOW, on_run_event};

/// Builds and runs the application until the user quits from the tray.
///
/// # Errors
///
/// Returns an error when the webview, the tray or the event loop cannot be created.
pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .setup(|app| {
            tray::install(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|handle, event| window::on_run_event(handle, &event));
    Ok(())
}
