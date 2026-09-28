//! Process start: diagnostics first, then either the watchdog supervisor or the agent.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode};

use tracing::{error, info, warn};

use crate::diagnostics::{self, ProcessKind, crash_reports, logging};
use crate::watchdog::{self, LaunchMode, Outcome, RestartPolicy, SupervisorEvent};

/// Runs this process as the supervisor or the agent, as [`watchdog::launch_mode`] decides.
#[must_use]
pub fn start() -> ExitCode {
    let mode = watchdog::launch_mode(|name| std::env::var_os(name));
    let process = match mode {
        LaunchMode::Supervisor => ProcessKind::Supervisor,
        LaunchMode::Agent => ProcessKind::Agent,
    };
    // Crash reporting installs its panic hook first, so the logging hook runs before it.
    let reporting = crash_reports::init(process);
    let log_dir = diagnostics::log_dir();
    start_logging(log_dir.as_deref(), process);
    logging::log_panics();
    info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        process = process.as_str(),
        crash_reports = reporting.is_some(),
        "starting"
    );
    match mode {
        LaunchMode::Supervisor => run_supervisor(),
        LaunchMode::Agent => run_agent(log_dir.as_deref()),
    }
}

fn start_logging(log_dir: Option<&Path>, process: ProcessKind) {
    let result = log_dir
        .ok_or_else(|| "the log folder could not be determined".to_owned())
        .and_then(|dir| {
            logging::init(&logging::LogConfig::for_process(dir.to_path_buf(), process))
                .map_err(|error| error.to_string())
        });
    if let Err(reason) = result {
        #[expect(
            clippy::print_stderr,
            reason = "without a log file, stderr is the only place left to say why"
        )]
        {
            eprintln!("Worklume could not start logging: {reason}");
        }
    }
}

fn run_agent(log_dir: Option<&Path>) -> ExitCode {
    if let Some(dir) = log_dir
        && let Err(error) = worklume_storage::route_sqlcipher_log(Some(&dir.join("sqlcipher.log")))
    {
        warn!(%error, "SQLCipher's log stays off");
        let _ = worklume_storage::route_sqlcipher_log(None);
    }
    match crate::run() {
        Ok(()) => {
            info!("the agent quit");
            ExitCode::SUCCESS
        }
        Err(error) => {
            error!(%error, "the agent failed to start");
            ExitCode::FAILURE
        }
    }
}

fn run_supervisor() -> ExitCode {
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            error!(%error, "the supervisor cannot find its own executable");
            return ExitCode::FAILURE;
        }
    };
    // Arguments pass through unchanged; the environment variable is what marks the agent.
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let outcome = watchdog::supervise(
        || {
            Command::new(&executable)
                .args(&arguments)
                .env(watchdog::SUPERVISED_ENV, "1")
                .spawn()
        },
        RestartPolicy::default(),
        |event| {
            log_supervisor_event(event);
            crash_reports::report_supervisor_event(event);
        },
    );
    match outcome {
        Outcome::AgentQuit => {
            info!("the agent quit; the supervisor stops");
            ExitCode::SUCCESS
        }
        Outcome::CrashLoop => ExitCode::FAILURE,
    }
}

fn log_supervisor_event(event: &SupervisorEvent) {
    match event {
        SupervisorEvent::Crashed {
            cause,
            uptime,
            recent_restarts,
            delay,
        } => warn!(
            %cause,
            uptime_ms = millis(*uptime),
            recent_restarts,
            delay_ms = millis(*delay),
            "the agent crashed; restarting it"
        ),
        SupervisorEvent::RestartRequested => info!("the agent asked to be restarted"),
        SupervisorEvent::GaveUp {
            cause,
            uptime,
            restarts,
        } => error!(
            %cause,
            uptime_ms = millis(*uptime),
            restarts,
            "the agent kept crashing; the watchdog stopped restarting it"
        ),
    }
}

/// Milliseconds as a number: tracing writes `u128` values as strings.
fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
