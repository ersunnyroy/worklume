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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use tauri::test::{mock_builder, mock_context, noop_assets};
    use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

    use super::{MAIN_WINDOW, on_run_event};

    /// Closes the main window from inside the running event loop and reports whether the app
    /// was still running afterwards (true) or began exiting (false). The mock runtime cannot
    /// report visibility, so the test observes the part that matters: the agent survives.
    fn app_keeps_running_after_close(with_handler: bool) -> bool {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app builds");
        WebviewWindowBuilder::new(&app, MAIN_WINDOW, WebviewUrl::default())
            .build()
            .expect("window builds");

        let outcome = Rc::new(Cell::new(None));
        let recorded = Rc::clone(&outcome);
        app.run_return(move |handle, event| {
            if with_handler {
                on_run_event(handle, &event);
            }
            if recorded.get().is_some() {
                return;
            }
            match event {
                RunEvent::Ready => {
                    let window = handle
                        .get_webview_window(MAIN_WINDOW)
                        .expect("window exists");
                    window.close().expect("close request is queued");
                }
                // The loop finished an iteration after processing the close: still running.
                // The mock loop only ends when no windows remain, so end it explicitly.
                RunEvent::MainEventsCleared => {
                    recorded.set(Some(true));
                    let window = handle
                        .get_webview_window(MAIN_WINDOW)
                        .expect("window exists");
                    window.destroy().expect("window is destroyed");
                }
                RunEvent::ExitRequested { .. } | RunEvent::Exit => recorded.set(Some(false)),
                _ => {}
            }
        });
        outcome.get().expect("the event loop processed the close")
    }

    #[test]
    fn closing_the_window_keeps_the_agent_running() {
        assert!(app_keeps_running_after_close(true));
    }

    #[test]
    fn without_the_handler_closing_the_window_would_quit() {
        // Guards the test above: it must be able to tell the two behaviours apart.
        assert!(!app_keeps_running_after_close(false));
    }
}
