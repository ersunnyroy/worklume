//! The watchdog: restart after a crash within 5 s, stop when the agent quits, restart at once
//! on request, and give up after 5 restarts in 5 minutes (the E09-S11 acceptance criterion).
//!
//! The supervision tests run real processes: this test binary, started again as a stand-in
//! agent that follows a script of exits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use worklume_desktop::watchdog::{
    self, AgentExit, CrashCause, CrashLoopGuard, LaunchMode, Outcome, RESTART_EXIT_CODE,
    RestartPolicy, SUPERVISED_ENV, SupervisorEvent, WATCHDOG_ENV,
};

const SCRIPT_ENV: &str = "WORKLUME_TEST_AGENT_SCRIPT";
const STARTS_ENV: &str = "WORKLUME_TEST_AGENT_STARTS";

/// The stand-in agent. Each run appends its start time to the starts file, then does what the
/// script's line for this run says (the last line repeats). A no-op in a normal test run.
#[test]
fn stand_in_agent() {
    let (Some(script), Some(starts)) = (std::env::var_os(SCRIPT_ENV), std::env::var_os(STARTS_ENV))
    else {
        return;
    };
    let run = fs::read_to_string(&starts).map_or(0, |text| text.lines().count());
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&starts)
        .unwrap();
    writeln!(file, "{millis}").unwrap();
    drop(file);

    let script = fs::read_to_string(script).unwrap();
    let steps: Vec<&str> = script.lines().collect();
    let step = steps.get(run).or(steps.last()).copied().unwrap_or("quit");
    match step {
        "quit" => std::process::exit(0),
        "restart" => std::process::exit(RESTART_EXIT_CODE),
        "abort" => std::process::abort(),
        other => std::process::exit(other.strip_prefix("exit ").unwrap().parse().unwrap()),
    }
}

struct Run {
    outcome: Outcome,
    events: Vec<SupervisorEvent>,
    starts: Vec<u128>,
    elapsed: Duration,
}

fn supervise_script(script: &[&str], policy: RestartPolicy) -> Run {
    let dir = TempDir::new().unwrap();
    let script_file = dir.path().join("script");
    let starts_file = dir.path().join("starts");
    fs::write(&script_file, script.join("\n")).unwrap();
    let executable = std::env::current_exe().unwrap();
    let started = Instant::now();
    let mut events = Vec::new();
    let outcome = watchdog::supervise(
        || spawn_stand_in(&executable, &script_file, &starts_file),
        policy,
        |event| events.push(event.clone()),
    );
    let starts = fs::read_to_string(&starts_file)
        .unwrap_or_default()
        .lines()
        .map(|line| line.parse().unwrap())
        .collect();
    Run {
        outcome,
        events,
        starts,
        elapsed: started.elapsed(),
    }
}

fn spawn_stand_in(
    executable: &PathBuf,
    script: &Path,
    starts: &Path,
) -> std::io::Result<std::process::Child> {
    Command::new(executable)
        .args(["stand_in_agent", "--exact", "--test-threads=1", "--quiet"])
        .env(SCRIPT_ENV, script)
        .env(STARTS_ENV, starts)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn quick_policy() -> RestartPolicy {
    RestartPolicy {
        restart_delay: Duration::from_millis(10),
        ..RestartPolicy::default()
    }
}

#[test]
fn restarts_a_crashed_agent_within_five_seconds_and_stops_when_it_quits() {
    // The real policy, so the timing is the one agents get.
    let run = supervise_script(&["abort", "exit 3", "quit"], RestartPolicy::default());

    assert_eq!(run.outcome, Outcome::AgentQuit);
    assert_eq!(run.starts.len(), 3);
    for pair in run.starts.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(gap < 5_000, "restarted after {gap} ms");
    }
    assert_eq!(run.events.len(), 2);
    let SupervisorEvent::Crashed { cause, delay, .. } = run.events[0] else {
        panic!("expected a crash, got {:?}", run.events[0]);
    };
    assert_eq!(delay, Duration::from_secs(1));
    if cfg!(unix) {
        assert_eq!(cause, CrashCause::Signal(6), "SIGABRT");
    }
    assert!(matches!(
        run.events[1],
        SupervisorEvent::Crashed {
            cause: CrashCause::ExitCode(3),
            recent_restarts: 2,
            ..
        }
    ));
}

#[test]
fn a_requested_restart_is_immediate_and_not_counted() {
    let policy = RestartPolicy {
        max_restarts: 1,
        ..quick_policy()
    };
    let run = supervise_script(&["restart", "restart", "restart", "quit"], policy);
    assert_eq!(run.outcome, Outcome::AgentQuit);
    assert_eq!(run.starts.len(), 4);
    assert_eq!(run.events, vec![SupervisorEvent::RestartRequested; 3]);
}

#[test]
fn gives_up_after_five_restarts_in_five_minutes() {
    let run = supervise_script(&["abort"], quick_policy());

    assert_eq!(run.outcome, Outcome::CrashLoop);
    assert_eq!(run.starts.len(), 6, "the first run and five restarts");
    let crashes = run
        .events
        .iter()
        .filter(|event| matches!(event, SupervisorEvent::Crashed { .. }))
        .count();
    assert_eq!(crashes, 5);
    assert!(matches!(
        run.events.last(),
        Some(SupervisorEvent::GaveUp { restarts: 5, .. })
    ));
    assert!(run.elapsed < Duration::from_mins(1));
}

#[test]
fn a_failed_launch_counts_as_a_crash() {
    let mut attempts = 0;
    let mut events = Vec::new();
    let outcome = watchdog::supervise(
        || {
            attempts += 1;
            Err(std::io::Error::other("missing executable"))
        },
        quick_policy(),
        |event| events.push(event.clone()),
    );
    assert_eq!(outcome, Outcome::CrashLoop);
    assert_eq!(attempts, 6);
    assert!(matches!(
        events.last(),
        Some(SupervisorEvent::GaveUp {
            cause: CrashCause::LaunchFailed,
            ..
        })
    ));
}

#[test]
fn the_guard_counts_restarts_within_the_window_only() {
    let policy = RestartPolicy::default();
    let start = Instant::now();
    let at = |seconds: u64| start + Duration::from_secs(seconds);

    let mut guard = CrashLoopGuard::new(policy);
    for second in 0..5 {
        assert!(guard.allow_restart(at(second * 10)), "restart {second}");
    }
    assert!(
        !guard.allow_restart(at(60)),
        "a sixth crash within 5 minutes"
    );

    // The window slides: once the oldest restart is 5 minutes old, a restart is allowed again.
    let mut guard = CrashLoopGuard::new(policy);
    for second in 0..5 {
        assert!(guard.allow_restart(at(second)));
    }
    assert!(!guard.allow_restart(at(299)));
    assert!(guard.allow_restart(at(300)));

    // Crashes spaced more than a minute apart never trip the guard.
    let mut guard = CrashLoopGuard::new(policy);
    for crash in 0..50 {
        assert!(guard.allow_restart(at(crash * 61)), "crash {crash}");
    }
}

#[test]
fn the_default_policy_matches_the_story() {
    let policy = RestartPolicy::default();
    assert!(policy.restart_delay < Duration::from_secs(5));
    assert_eq!(policy.max_restarts, 5);
    assert_eq!(policy.window, Duration::from_mins(5));
}

#[cfg(unix)]
#[test]
fn classifies_exit_statuses() {
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::ExitStatus;
    assert_eq!(watchdog::classify(ExitStatus::from_raw(0)), AgentExit::Quit);
    assert_eq!(
        watchdog::classify(ExitStatus::from_raw(RESTART_EXIT_CODE << 8)),
        AgentExit::RestartRequested
    );
    assert_eq!(
        watchdog::classify(ExitStatus::from_raw(1 << 8)),
        AgentExit::Crashed(CrashCause::ExitCode(1))
    );
    assert_eq!(
        watchdog::classify(ExitStatus::from_raw(9)),
        AgentExit::Crashed(CrashCause::Signal(9))
    );
}

#[cfg(windows)]
#[test]
fn classifies_exit_statuses() {
    use std::os::windows::process::ExitStatusExt as _;
    use std::process::ExitStatus;
    assert_eq!(watchdog::classify(ExitStatus::from_raw(0)), AgentExit::Quit);
    assert_eq!(
        watchdog::classify(ExitStatus::from_raw(75)),
        AgentExit::RestartRequested
    );
    assert_eq!(
        watchdog::classify(ExitStatus::from_raw(1)),
        AgentExit::Crashed(CrashCause::ExitCode(1))
    );
}

#[test]
fn launch_mode_follows_the_build_and_the_environment() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    };
    // Release builds supervise, unless this process is the supervised agent.
    assert_eq!(
        watchdog::launch_mode_for(true, env(&[])),
        LaunchMode::Supervisor
    );
    assert_eq!(
        watchdog::launch_mode_for(true, env(&[(WATCHDOG_ENV, "off")])),
        LaunchMode::Supervisor,
        "release builds cannot switch the watchdog off"
    );
    assert_eq!(
        watchdog::launch_mode_for(true, env(&[(SUPERVISED_ENV, "1")])),
        LaunchMode::Agent
    );
    // Debug builds run the agent directly unless asked.
    assert_eq!(
        watchdog::launch_mode_for(false, env(&[])),
        LaunchMode::Agent
    );
    assert_eq!(
        watchdog::launch_mode_for(false, env(&[(WATCHDOG_ENV, "on")])),
        LaunchMode::Supervisor
    );
    assert_eq!(
        watchdog::launch_mode_for(false, env(&[(WATCHDOG_ENV, "on"), (SUPERVISED_ENV, "1")])),
        LaunchMode::Agent
    );
}
