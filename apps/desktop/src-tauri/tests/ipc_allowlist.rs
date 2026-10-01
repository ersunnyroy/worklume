//! The webview can call only what capabilities/main.json grants, and only from the main window.

// Shared helpers are not #[test] functions, so clippy.toml's test exemption does not reach them.
#![allow(clippy::expect_used, reason = "test setup failures should fail loudly")]

use std::sync::Arc;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tempfile::TempDir;
use uuid::Uuid;
use worklume_core::catalog::{Catalog, Project};
use worklume_core::timekeeper::Sensors;
use worklume_desktop::MAIN_WINDOW;
use worklume_desktop::agent::Agent;
use worklume_platform::fake::{
    FakeBrowser, FakeClock, FakeIdle, FakeInput, FakeWindow, MemorySecretStore,
};

const PROJECT: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0001);

/// The real app context (config, capabilities and ACL) on Tauri's mock runtime, with an agent
/// on fake sensors in `dir`.
fn app(dir: &TempDir) -> App<MockRuntime> {
    let clock = Arc::new(FakeClock::new(1_789_999_380_000));
    let agent = Agent::new(Box::new(|| 1_789_948_800_000));
    agent
        .open(
            dir.path(),
            &MemorySecretStore::default(),
            Sensors {
                clock: Arc::clone(&clock) as _,
                idle: Arc::new(FakeIdle::new(clock)),
                window: Arc::new(FakeWindow::default()),
                browser: Arc::new(FakeBrowser::default()),
                input: Arc::new(FakeInput::default()),
            },
        )
        .expect("the agent opens");
    agent
        .set_catalog(Catalog {
            projects: vec![Project {
                id: PROJECT,
                name: "Website".to_owned(),
                tasks: Vec::new(),
            }],
            break_policies: Vec::new(),
        })
        .expect("the agent is ready");
    let commands = worklume_desktop::ipc::builder::<MockRuntime>();
    let app = mock_builder()
        .manage(agent)
        .invoke_handler(commands.invoke_handler())
        .build(tauri::generate_context!(test = true))
        .expect("app builds with the real context");
    commands.mount_events(&app);
    app
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .expect("window builds")
}

/// The origin Tauri serves the app from: Windows (`WebView2`) and Android use an HTTP workaround.
fn local_origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    }
}

fn invoke(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
) -> Result<InvokeResponseBody, serde_json::Value> {
    invoke_with(window, command, &json!({}))
}

fn invoke_with(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    arguments: &Value,
) -> Result<InvokeResponseBody, serde_json::Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: local_origin().parse().expect("valid URL"),
            body: InvokeBody::Json(arguments.clone()),
            headers: tauri::http::HeaderMap::default(),
            invoke_key: INVOKE_KEY.to_owned(),
        },
    )
}

fn json_of(response: &InvokeResponseBody) -> Value {
    response.clone().deserialize().expect("JSON response")
}

#[test]
fn the_main_window_may_read_the_agent_status() {
    let dir = TempDir::new().expect("temporary folder");
    let app = app(&dir);
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
fn the_main_window_may_run_the_timer_through_the_agent() {
    let dir = TempDir::new().expect("temporary folder");
    let app = app(&dir);
    let main = window(&app, MAIN_WINDOW);

    let state = json_of(&invoke(&main, "agent_state").expect("agent_state is allowed"));
    assert_eq!(state["kind"], "ready");
    assert_eq!(state["timer"]["status"]["kind"], "stopped");
    let catalog = json_of(&invoke(&main, "work_catalog").expect("work_catalog is allowed"));
    assert_eq!(catalog["projects"][0]["name"], "Website");

    // IDs from the webview are parsed, then checked against the catalog.
    for (project_id, expected) in [
        ("not-an-id", "invalid_id"),
        ("0192f000-0000-7000-8000-000000000999", "unknown_project"),
    ] {
        let refused = invoke_with(
            &main,
            "timer_start",
            &json!({"projectId": project_id, "taskId": null}),
        )
        .expect_err("refused");
        assert_eq!(refused, json!(expected));
    }
    let started = invoke_with(
        &main,
        "timer_start",
        &json!({"projectId": PROJECT.to_string(), "taskId": null}),
    )
    .expect("a catalogued project starts");
    assert_eq!(json_of(&started)["status"]["kind"], "running");
    assert_eq!(
        invoke(&main, "break_end").expect_err("not on a break"),
        json!("not_now")
    );
    let answer = json!({"answer": {"kind": "reassign", "project_id": "0192f000-0000-7000-8000-000000000999"}});
    assert_eq!(
        invoke_with(&main, "idle_answer", &answer).expect_err("refused"),
        json!("unknown_project")
    );
    let stopped = invoke(&main, "timer_stop").expect("stops");
    assert_eq!(json_of(&stopped)["status"]["kind"], "stopped");
}

/// Asserts the ACL let the call through: it succeeded, or failed for another reason.
fn assert_allowed(window: &WebviewWindow<MockRuntime>, command: &str) {
    if let Err(error) = invoke(window, command) {
        let message = error.as_str().unwrap_or_default();
        assert!(
            !message.contains("not allowed"),
            "{command} was refused by the ACL: {message}"
        );
    }
}

#[test]
fn the_main_window_may_listen_to_events_but_not_emit_them() {
    let dir = TempDir::new().expect("temporary folder");
    let app = app(&dir);
    let main = window(&app, MAIN_WINDOW);
    assert_allowed(&main, "plugin:event|listen");
    assert_allowed(&main, "plugin:event|unlisten");
    assert_refused(&main, "plugin:event|emit");
    assert_refused(&main, "plugin:event|emit_to");
}

#[test]
fn commands_outside_the_allowlist_are_refused() {
    let dir = TempDir::new().expect("temporary folder");
    let app = app(&dir);
    let main = window(&app, MAIN_WINDOW);

    // Core commands, including the read-only ones that `core:default` would grant.
    for command in [
        "plugin:window|close",
        "plugin:window|is_visible",
        "plugin:webview|create_webview_window",
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
    let dir = TempDir::new().expect("temporary folder");
    let app = app(&dir);
    let other = window(&app, "other");

    for command in [
        "agent_status",
        "agent_state",
        "work_catalog",
        "timer_start",
        "timer_switch",
        "timer_stop",
        "timer_resume",
        "break_start",
        "break_end",
        "idle_answer",
        "plugin:event|listen",
    ] {
        assert_refused(&other, command);
    }
}
