//! Crash reports through Sentry (architecture: Sentry for desktop errors).
//!
//! Two kinds of report are sent:
//! - Rust panics in either process, with their stack trace;
//! - the watchdog's reports of an agent that died, with how (exit code or signal) and how long
//!   it ran. Native crashes, in the webview or OS libraries, show up here.
//!
//! Memory dumps are not collected: an employee-monitoring agent's memory can hold screenshots,
//! URLs and window titles, so dumps wait for a privacy decision.
//!
//! Nothing identifies the member or the machine: no user, no hostname, no device name, no list
//! of loaded libraries, no breadcrumbs, and home folder paths are shortened to `~`.
//!
//! The DSN comes from the build environment (`WORKLUME_SENTRY_DSN`). Without it, nothing is sent.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use sentry::protocol::{Context, DebugMeta, Event, Level, Value};
use sentry::transports::UreqHttpTransportOptions;
use sentry::{ClientInitGuard, ClientOptions, Transport, TransportFactory, TransportOptions};

use super::{HomeRedactor, ProcessKind};
use crate::watchdog::{CrashCause, SupervisorEvent};

/// The DSN compiled into this build, if any.
pub const BUILD_DSN: Option<&str> = option_env!("WORKLUME_SENTRY_DSN");

/// Client options. With no usable DSN the client sends nothing.
pub fn options(dsn: Option<&str>, redactor: HomeRedactor) -> ClientOptions {
    let mut options = ClientOptions::default();
    options.dsn = dsn.and_then(|dsn| dsn.parse().ok());
    options.release = Some(Cow::Owned(format!(
        "worklume-desktop@{}",
        env!("CARGO_PKG_VERSION")
    )));
    options.environment = Some(Cow::Borrowed(if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    }));
    options.send_default_pii = false;
    // Log lines would travel as breadcrumbs; the log files stay on the device.
    options.max_breadcrumbs = 0;
    options.before_send = Some(Arc::new(move |event| Some(scrub(event, &redactor))));
    options.shutdown_timeout = Duration::from_secs(2);
    options.transport = Some(Arc::new(OsTrustTransport));
    options
}

/// Sends reports through the agent's HTTPS client, which trusts the OS certificate store. The
/// SDK's own client would trust only a bundled list of roots.
struct OsTrustTransport;

impl TransportFactory for OsTrustTransport {
    fn create_transport_with_options(&self, options: TransportOptions) -> Arc<dyn Transport> {
        Arc::new(
            UreqHttpTransportOptions::from(options)
                .with_agent(worklume_sync::http::agent())
                .build(),
        )
    }
}

/// Starts crash reporting for this process, if the build has a DSN. Keep the guard alive: it
/// flushes pending reports when dropped.
#[must_use]
pub fn init(process: ProcessKind) -> Option<ClientInitGuard> {
    let dsn = BUILD_DSN.filter(|dsn| !dsn.is_empty())?;
    let guard = sentry::init(options(Some(dsn), HomeRedactor::current()));
    if !guard.is_enabled() {
        return None;
    }
    sentry::configure_scope(|scope| scope.set_tag("process", process.as_str()));
    Some(guard)
}

/// Reports what the watchdog saw. A requested restart is not a crash and is not reported.
pub fn report_supervisor_event(event: &SupervisorEvent) {
    if let Some(event) = supervisor_event(event) {
        sentry::capture_event(event);
    }
}

/// The Sentry event for a watchdog observation.
#[must_use]
pub fn supervisor_event(event: &SupervisorEvent) -> Option<Event<'static>> {
    let (level, message, cause, uptime, restarts) = match *event {
        SupervisorEvent::Crashed {
            cause,
            uptime,
            recent_restarts,
            ..
        } => (
            Level::Error,
            "The agent exited unexpectedly and was restarted",
            cause,
            uptime,
            recent_restarts,
        ),
        SupervisorEvent::GaveUp {
            cause,
            uptime,
            restarts,
        } => (
            Level::Fatal,
            "The agent kept crashing, so the watchdog stopped restarting it",
            cause,
            uptime,
            restarts,
        ),
        SupervisorEvent::RestartRequested => return None,
    };
    let (kind, code) = match cause {
        CrashCause::ExitCode(code) => ("exit_code", code.to_string()),
        CrashCause::Signal(signal) => ("signal", signal.to_string()),
        CrashCause::LaunchFailed => ("launch_failed", String::new()),
    };
    let mut report = Event {
        level,
        message: Some(message.to_owned()),
        // One issue per way of dying, not one per occurrence.
        fingerprint: Cow::Owned(vec![
            Cow::Borrowed("agent-crash"),
            Cow::Borrowed(kind),
            Cow::Owned(code.clone()),
        ]),
        ..Event::default()
    };
    report
        .tags
        .insert("crash_cause".to_owned(), kind.to_owned());
    if !code.is_empty() {
        report.tags.insert("crash_code".to_owned(), code);
    }
    report.extra.insert(
        "uptime_ms".to_owned(),
        Value::from(u64::try_from(uptime.as_millis()).unwrap_or(u64::MAX)),
    );
    report
        .extra
        .insert("recent_restarts".to_owned(), Value::from(restarts));
    Some(report)
}

/// Removes what could identify the member or the machine.
#[must_use]
pub fn scrub(mut event: Event<'static>, redactor: &HomeRedactor) -> Event<'static> {
    event.server_name = None;
    event.user = None;
    event.request = None;
    event.breadcrumbs.values.clear();
    event.modules.clear();
    event.debug_meta = Cow::Owned(DebugMeta::default());
    if let Some(Context::Device(device)) = event.contexts.get_mut("device") {
        device.name = None;
    }
    if let Some(message) = event.message.as_mut() {
        *message = redactor.redact(message);
    }
    if let Some(entry) = event.logentry.as_mut() {
        entry.message = redactor.redact(&entry.message);
    }
    for value in event.tags.values_mut() {
        *value = redactor.redact(value);
    }
    for value in event.extra.values_mut() {
        if let Value::String(text) = value {
            *text = redactor.redact(text);
        }
    }
    let stacktraces = event
        .exception
        .values
        .iter_mut()
        .filter_map(|exception| {
            if let Some(value) = exception.value.as_mut() {
                *value = redactor.redact(value);
            }
            exception.stacktrace.as_mut()
        })
        .chain(
            event
                .threads
                .values
                .iter_mut()
                .filter_map(|thread| thread.stacktrace.as_mut()),
        );
    for stacktrace in stacktraces {
        for frame in &mut stacktrace.frames {
            for path in [frame.filename.as_mut(), frame.abs_path.as_mut()]
                .into_iter()
                .flatten()
            {
                *path = redactor.redact(path);
            }
        }
    }
    event
}
