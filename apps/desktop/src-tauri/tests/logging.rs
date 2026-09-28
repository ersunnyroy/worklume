//! Log files: where they live, their format, the filter, redaction of home paths, and panics.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use tauri::Manager as _;
use tauri::test::mock_builder;
use tempfile::TempDir;
use worklume_desktop::diagnostics::logging::{self, LogConfig};
use worklume_desktop::diagnostics::{self, APP_IDENTIFIER, HomeRedactor, ProcessKind};

#[test]
fn the_identifier_matches_the_tauri_config() {
    let context: tauri::Context<tauri::test::MockRuntime> = tauri::generate_context!(test = true);
    assert_eq!(context.config().identifier, APP_IDENTIFIER);
}

#[test]
fn the_log_folder_is_the_one_tauri_uses() {
    let app = mock_builder()
        .build(tauri::generate_context!(test = true))
        .expect("app builds");
    assert_eq!(
        diagnostics::log_dir(),
        Some(
            app.path()
                .app_log_dir()
                .expect("Tauri resolves the log folder")
        )
    );
}

fn config(dir: &Path, home: &str) -> LogConfig {
    LogConfig {
        dir: dir.to_path_buf(),
        process: ProcessKind::Agent,
        redactor: HomeRedactor::new(Some(Path::new(home))),
        filter: None,
        echo_to_stderr: false,
    }
}

/// Every JSON line written to the agent's log files in `dir`.
fn lines(dir: &Path) -> Vec<Value> {
    let mut lines = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        assert!(
            name.starts_with("agent.")
                && path.extension().is_some_and(|extension| extension == "log"),
            "{name}"
        );
        for line in fs::read_to_string(&path).unwrap().lines() {
            lines.push(serde_json::from_str(line).expect("each line is one JSON object"));
        }
    }
    lines
}

#[test]
fn writes_json_lines_filtered_and_redacted() {
    let dir = TempDir::new().unwrap();
    let subscriber = logging::subscriber(&config(dir.path(), "/Users/alex")).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            target: "worklume_desktop::test",
            path = "/Users/alex/Library/Application Support/x",
            attempts = 3,
            "opened /Users/alex/store.db"
        );
        tracing::debug!(target: "worklume_desktop::test", "too detailed by default");
        tracing::info!(target: "tao::event_loop", "a dependency's routine message");
        tracing::warn!(target: "tao::event_loop", "a dependency's warning");
    });

    let lines = lines(dir.path());
    let text = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(lines.len(), 2, "{text}");
    assert!(!text.contains("alex"), "{text}");

    let ours = &lines[0];
    assert_eq!(ours["level"], "INFO");
    assert_eq!(ours["target"], "worklume_desktop::test");
    assert_eq!(ours["message"], "opened ~/store.db");
    assert_eq!(ours["path"], "~/Library/Application Support/x");
    assert_eq!(ours["attempts"], 3);
    assert!(
        ours["timestamp"].as_str().unwrap().ends_with('Z'),
        "UTC timestamps"
    );

    assert_eq!(lines[1]["level"], "WARN");
    assert_eq!(lines[1]["target"], "tao::event_loop");
}

#[test]
fn the_filter_can_be_raised() {
    let dir = TempDir::new().unwrap();
    let mut config = config(dir.path(), "/Users/alex");
    config.filter = Some("debug".to_owned());
    let subscriber = logging::subscriber(&config).unwrap();
    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!(target: "worklume_desktop::test", "now visible");
    });
    assert_eq!(lines(dir.path())[0]["message"], "now visible");
}

#[test]
fn windows_home_paths_are_redacted_as_written_and_as_json_escapes_them() {
    let redactor = HomeRedactor::new(Some(Path::new(r"C:\Users\alex")));
    assert_eq!(redactor.redact(r"C:\Users\alex\AppData\x"), r"~\AppData\x");
    assert_eq!(
        redactor.redact(r#"{"path":"C:\\Users\\alex\\AppData"}"#),
        r#"{"path":"~\\AppData"}"#
    );
    // A trailing separator on the home folder does not matter.
    let redactor = HomeRedactor::new(Some(Path::new("/home/alex/")));
    assert_eq!(redactor.redact("/home/alex/.local"), "~/.local");
    // A root "home" would redact everything; it redacts nothing instead.
    assert_eq!(
        HomeRedactor::new(Some(Path::new("/"))).redact("/usr/bin"),
        "/usr/bin"
    );
    assert_eq!(HomeRedactor::new(None).redact("/home/alex"), "/home/alex");
}

#[test]
fn panics_are_logged_with_their_location() {
    let dir = TempDir::new().unwrap();
    let subscriber = logging::subscriber(&config(dir.path(), "/Users/alex")).unwrap();
    logging::log_panics();
    tracing::subscriber::with_default(subscriber, || {
        let result = std::panic::catch_unwind(|| {
            panic!("could not read /Users/alex/Documents/report.txt");
        });
        assert!(result.is_err());
    });

    let lines = lines(dir.path());
    let panic = lines
        .iter()
        .find(|line| line["target"] == "worklume_desktop::panic")
        .expect("the panic was logged");
    assert_eq!(panic["level"], "ERROR");
    assert_eq!(
        panic["message"],
        "panic: could not read ~/Documents/report.txt"
    );
    assert!(panic["location"].as_str().unwrap().contains("logging.rs:"));
}

#[cfg(unix)]
#[test]
fn the_log_folder_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt as _;
    let parent = TempDir::new().unwrap();
    let dir = parent.path().join("logs");
    let _subscriber = logging::subscriber(&config(&dir, "/Users/alex")).unwrap();
    let mode = fs::metadata(&dir).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
}
