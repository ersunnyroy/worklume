//! Worklume desktop agent shell: a tray-resident Tauri application.
//!
//! The window starts hidden and closing it only hides it, so the agent keeps running from the
//! tray. A second launch focuses the running agent instead of starting another one. Tracking,
//! storage and sync live in the `worklume-core`, `worklume-storage` and `worklume-sync` crates;
//! OS access lives in `worklume-platform`.

pub mod ipc;
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
    let commands = ipc::builder::<tauri::Wry>();
    let app = tauri::Builder::default()
        // Must be the first plugin, so a second launch exits before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            window::show_main(app);
        }))
        .invoke_handler(commands.invoke_handler())
        .setup(|app| {
            tray::install(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|handle, event| window::on_run_event(handle, &event));
    Ok(())
}
