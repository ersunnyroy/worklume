//! Crash reports: what is sent for panics and for crashes the watchdog sees, and what is
//! removed before anything leaves the device. Events are captured in-process; nothing is sent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sentry::integrations::panic::PanicIntegration;
use sentry::protocol::{
    Breadcrumb, Context, DeviceContext, Event, Exception, Frame, Level, Stacktrace, User, Value,
};
use worklume_desktop::diagnostics::HomeRedactor;
use worklume_desktop::diagnostics::crash_reports::{self, options, scrub, supervisor_event};
use worklume_desktop::watchdog::{CrashCause, SupervisorEvent};

fn alex() -> HomeRedactor {
    HomeRedactor::new(Some(Path::new("/Users/alex")))
}

#[test]
fn nothing_is_sent_without_a_dsn() {
    assert!(options(None, alex()).dsn.is_none());
    assert!(options(Some("not a dsn"), alex()).dsn.is_none());
    assert!(
        options(Some("https://public@o0.ingest.sentry.io/0"), alex())
            .dsn
            .is_some()
    );
    if crash_reports::BUILD_DSN.is_none() {
        assert!(crash_reports::init(worklume_desktop::diagnostics::ProcessKind::Agent).is_none());
    }
}

#[test]
fn options_leave_out_personal_data() {
    let options = options(None, alex());
    assert!(!options.send_default_pii);
    assert_eq!(options.max_breadcrumbs, 0);
    assert_eq!(
        options.release.as_deref(),
        Some(concat!("worklume-desktop@", env!("CARGO_PKG_VERSION")))
    );
    assert!(options.before_send.is_some());
}

#[test]
fn scrubbing_removes_identity_and_home_paths() {
    let mut event = Event {
        server_name: Some("alexs-macbook-pro".into()),
        user: Some(User {
            username: Some("alex".to_owned()),
            ..User::default()
        }),
        message: Some("failed to open /Users/alex/Library/x".to_owned()),
        ..Event::default()
    };
    event.breadcrumbs.values.push(Breadcrumb {
        message: Some("opened /Users/alex/secret".to_owned()),
        ..Breadcrumb::default()
    });
    event.contexts.insert(
        "device".to_owned(),
        Context::Device(Box::new(DeviceContext {
            name: Some("Alex's MacBook Pro".to_owned()),
            arch: Some("arm64".to_owned()),
            ..DeviceContext::default()
        })),
    );
    event
        .tags
        .insert("folder".to_owned(), "/Users/alex/Desktop".to_owned());
    event
        .extra
        .insert("file".to_owned(), Value::from("/Users/alex/Desktop/a.txt"));
    event.exception.values.push(Exception {
        ty: "panic".to_owned(),
        value: Some("read /Users/alex/Documents/report.txt".to_owned()),
        stacktrace: Some(Stacktrace {
            frames: vec![Frame {
                abs_path: Some("/Users/alex/.cargo/registry/src/crate/lib.rs".to_owned()),
                filename: Some("/Users/alex/src/main.rs".to_owned()),
                ..Frame::default()
            }],
            ..Stacktrace::default()
        }),
        ..Exception::default()
    });

    let event = scrub(event, &alex());
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains("alex"), "{json}");
    assert!(!json.contains("Alex"), "{json}");
    assert_eq!(event.server_name, None);
    assert_eq!(event.user, None);
    assert!(event.breadcrumbs.values.is_empty());
    let Some(Context::Device(device)) = event.contexts.get("device") else {
        panic!("the device context stays, without its name");
    };
    assert_eq!(device.arch.as_deref(), Some("arm64"));
    assert_eq!(event.message.as_deref(), Some("failed to open ~/Library/x"));
    let frame = &event.exception.values[0]
        .stacktrace
        .as_ref()
        .unwrap()
        .frames[0];
    assert_eq!(frame.filename.as_deref(), Some("~/src/main.rs"));
}

#[test]
fn the_watchdog_reports_crashes_and_crash_loops() {
    let crashed = supervisor_event(&SupervisorEvent::Crashed {
        cause: CrashCause::Signal(11),
        uptime: Duration::from_secs(42),
        recent_restarts: 2,
        delay: Duration::from_secs(1),
    })
    .unwrap();
    assert_eq!(crashed.level, Level::Error);
    assert_eq!(
        crashed.tags,
        BTreeMap::from([
            ("crash_cause".to_owned(), "signal".to_owned()),
            ("crash_code".to_owned(), "11".to_owned()),
        ])
    );
    assert_eq!(crashed.extra["uptime_ms"], Value::from(42_000));
    assert_eq!(crashed.extra["recent_restarts"], Value::from(2));
    assert_eq!(
        crashed.fingerprint.as_ref(),
        ["agent-crash", "signal", "11"]
    );

    let gave_up = supervisor_event(&SupervisorEvent::GaveUp {
        cause: CrashCause::ExitCode(1),
        uptime: Duration::from_millis(300),
        restarts: 5,
    })
    .unwrap();
    assert_eq!(gave_up.level, Level::Fatal);
    assert_eq!(gave_up.tags["crash_cause"], "exit_code");

    assert!(supervisor_event(&SupervisorEvent::RestartRequested).is_none());
}

#[test]
fn a_panic_is_captured_and_scrubbed() {
    let mut options = options(None, alex());
    options
        .integrations
        .push(Arc::new(PanicIntegration::default()));
    let events = sentry::test::with_captured_events_options(
        || {
            let result = std::panic::catch_unwind(|| {
                panic!("could not read /Users/alex/Documents/report.txt");
            });
            assert!(result.is_err());
        },
        options,
    );
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.level, Level::Fatal);
    assert_eq!(
        event.exception.values[0].value.as_deref(),
        Some("could not read ~/Documents/report.txt")
    );
    assert_eq!(event.server_name, None);
}

#[test]
fn a_reported_crash_goes_through_the_same_scrubbing() {
    let events = sentry::test::with_captured_events_options(
        || {
            crash_reports::report_supervisor_event(&SupervisorEvent::GaveUp {
                cause: CrashCause::ExitCode(101),
                uptime: Duration::from_secs(3),
                restarts: 5,
            });
            crash_reports::report_supervisor_event(&SupervisorEvent::RestartRequested);
        },
        options(None, alex()),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tags["crash_code"], "101");
    assert_eq!(events[0].server_name, None);
}
