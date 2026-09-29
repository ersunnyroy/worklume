//! Worklume desktop agent shell: a tray-resident Tauri application.
//!
//! The window starts hidden and closing it only hides it, so the agent keeps running from the
//! tray. A second launch focuses the running agent instead of starting another one. In release
//! builds a watchdog process restarts the agent after a crash. Tracking, storage and sync live
//! in the `worklume-core`, `worklume-storage` and `worklume-sync` crates; OS access lives in
//! `worklume-platform`. [`agent`] runs the timekeeper on a background thread for the timer UI.

pub mod agent;
pub mod diagnostics;
pub mod ipc;
mod startup;
mod ticker;
mod tray;
pub mod watchdog;
mod window;

pub use startup::start;
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
        .manage(agent::Agent::new(Box::new(agent::local_day_start)))
        .invoke_handler(commands.invoke_handler())
        .setup(move |app| {
            commands.mount_events(app);
            tray::install(app.handle())?;
            ticker::spawn(app.handle().clone())?;
            Ok(())
        })
        .build(tauri::generate_context!())?;
    app.run(|handle, event| window::on_run_event(handle, &event));
    Ok(())
}
