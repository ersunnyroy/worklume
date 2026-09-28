//! The webview can call only what capabilities/main.json grants, and only from the main window.

// Shared helpers are not #[test] functions, so clippy.toml's test exemption does not reach them.
#![allow(clippy::expect_used, reason = "test setup failures should fail loudly")]

use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use worklume_desktop::MAIN_WINDOW;

/// The real app context (config, capabilities and ACL) on Tauri's mock runtime.
fn app() -> App<MockRuntime> {
    mock_builder()
        .invoke_handler(worklume_desktop::ipc::builder::<MockRuntime>().invoke_handler())
        .build(tauri::generate_context!(test = true))
        .expect("app builds with the real context")
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .expect("window builds")
}

fn invoke(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
) -> Result<InvokeResponseBody, serde_json::Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("valid URL"),
            body: InvokeBody::default(),
            headers: tauri::http::HeaderMap::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        },
    )
}

#[test]
fn the_main_window_may_read_the_agent_status() {
    let app = app();
    let main = window(&app, MAIN_WINDOW);

    let response = invoke(&main, "agent_status").expect("agent_status is allowed");
    let status: serde_json::Value = response.deserialize().expect("JSON response");

    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert!(status["platform"]["os"].is_string());
}

/// Asserts the ACL refused the call, as opposed to a call that was allowed but failed later
/// (for example on missing arguments), which would hide a too-broad grant.
fn assert_refused(window: &WebviewWindow<MockRuntime>, command: &str) {
    let error = invoke(window, command).expect_err("the call should be refused");
    let message = error.as_str().unwrap_or_default();
    assert!(
        message.contains("not allowed"),
        "{command} was not refused by the ACL: {message}"
    );
}

#[test]
fn commands_outside_the_allowlist_are_refused() {
    let app = app();
    let main = window(&app, MAIN_WINDOW);

    // Core commands, including the read-only ones that `core:default` would grant.
    for command in [
        "plugin:window|close",
        "plugin:window|is_visible",
        "plugin:webview|create_webview_window",
        "plugin:event|listen",
        "plugin:event|emit",
        "plugin:path|resolve_directory",
        "plugin:app|version",
        "not_a_command",
    ] {
        assert_refused(&main, command);
    }
}

#[test]
fn other_windows_may_not_call_anything() {
    let app = app();
    let other = window(&app, "other");

    assert_refused(&other, "agent_status");
}
