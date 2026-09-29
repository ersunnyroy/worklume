//! The watchdog (architecture §5 and §13): release builds start as a small supervisor process
//! that runs the agent as a child process and restarts it within seconds of a crash. The
//! journal lets the restarted agent resume where it stopped.
//!
//! The agent's exit status tells the supervisor what happened:
//! - success: the member quit (or a second launch handed over to the running agent), so the
//!   supervisor exits too;
//! - [`RESTART_EXIT_CODE`]: the agent asks to be started again at once, for example after an
//!   update (E09-S10). This is not a crash;
//! - anything else, including being killed: a crash. The agent is restarted after
//!   [`RestartPolicy::restart_delay`], unless it has already been restarted
//!   [`RestartPolicy::max_restarts`] times within [`RestartPolicy::window`]. Then the supervisor
//!   gives up rather than crash-looping.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

/// Set on the agent process the supervisor starts, which then runs the agent itself.
pub const SUPERVISED_ENV: &str = "WORKLUME_SUPERVISED";

/// Debug builds run the agent without a supervisor, so `tauri dev` can restart it freely. Set
/// this to `on` to try the supervisor in a debug build. Release builds ignore it.
pub const WATCHDOG_ENV: &str = "WORKLUME_WATCHDOG";

/// Exit code with which the agent asks to be restarted at once. 75 is `EX_TEMPFAIL`.
pub const RESTART_EXIT_CODE: i32 = 75;

/// What this process should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Agent,
    Supervisor,
}

/// Decides the launch mode from the environment, for this build.
#[must_use]
pub fn launch_mode(var: impl Fn(&str) -> Option<OsString>) -> LaunchMode {
    launch_mode_for(!cfg!(debug_assertions), var)
}

/// Decides the launch mode for a release (`true`) or debug build.
#[must_use]
pub fn launch_mode_for(release: bool, var: impl Fn(&str) -> Option<OsString>) -> LaunchMode {
    if var(SUPERVISED_ENV).is_some() {
        LaunchMode::Agent
    } else if release || var(WATCHDOG_ENV).is_some_and(|value| value == "on") {
        LaunchMode::Supervisor
    } else {
        LaunchMode::Agent
    }
}

/// When to restart the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    /// Pause before a restart. Short enough to restart well within the 5 s the architecture
    /// allows; long enough that a session ending (which also stops the supervisor) wins the race.
    pub restart_delay: Duration,
    /// Restarts allowed within `window` before the supervisor gives up.
    pub max_restarts: usize,
    pub window: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            restart_delay: Duration::from_secs(1),
            max_restarts: 5,
            window: Duration::from_mins(5),
        }
    }
}

/// How the agent ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentExit {
    Quit,
    RestartRequested,
    Crashed(CrashCause),
}

/// Why the agent is considered crashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashCause {
    /// Exited with a failure code (on Windows, also when killed).
    ExitCode(i32),
    /// Killed by a signal (Unix), such as `SIGSEGV`, `SIGABRT` or `SIGKILL`.
    Signal(i32),
    /// The agent could not be started or waited for.
    LaunchFailed,
}

impl std::fmt::Display for CrashCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExitCode(code) => write!(f, "exit code {code}"),
            Self::Signal(signal) => write!(f, "signal {signal}"),
            Self::LaunchFailed => f.write_str("launch failed"),
        }
    }
}

/// Classifies how the agent process ended.
#[must_use]
pub fn classify(status: ExitStatus) -> AgentExit {
    if status.success() {
        return AgentExit::Quit;
    }
    match status.code() {
        Some(RESTART_EXIT_CODE) => AgentExit::RestartRequested,
        Some(code) => AgentExit::Crashed(CrashCause::ExitCode(code)),
        None => AgentExit::Crashed(signal_cause(status)),
    }
}

#[cfg(unix)]
fn signal_cause(status: ExitStatus) -> CrashCause {
    use std::os::unix::process::ExitStatusExt as _;
    status
        .signal()
        .map_or(CrashCause::LaunchFailed, CrashCause::Signal)
}

#[cfg(not(unix))]
fn signal_cause(_status: ExitStatus) -> CrashCause {
    // Windows always reports an exit code, so this is unreachable there in practice.
    CrashCause::LaunchFailed
}

/// Counts recent restarts and decides whether another one is allowed.
#[derive(Debug)]
pub struct CrashLoopGuard {
    policy: RestartPolicy,
    restarts: VecDeque<Instant>,
}

impl CrashLoopGuard {
    #[must_use]
    pub fn new(policy: RestartPolicy) -> Self {
        Self {
            policy,
            restarts: VecDeque::with_capacity(policy.max_restarts + 1),
        }
    }

    /// Records a crash at `now` and returns whether the agent may be restarted: it may unless
    /// `max_restarts` restarts already happened within the window before `now`.
    pub fn allow_restart(&mut self, now: Instant) -> bool {
        while self
            .restarts
            .front()
            .is_some_and(|&restart| now.saturating_duration_since(restart) >= self.policy.window)
        {
            self.restarts.pop_front();
        }
        if self.restarts.len() >= self.policy.max_restarts {
            return false;
        }
        self.restarts.push_back(now);
        true
    }

    /// Restarts within the window, as of the last call.
    #[must_use]
    pub fn recent_restarts(&self) -> usize {
        self.restarts.len()
    }
}

/// What the supervisor observed, for logging and crash reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorEvent {
    /// The agent crashed and will be restarted after `delay`.
    Crashed {
        cause: CrashCause,
        uptime: Duration,
        recent_restarts: usize,
        delay: Duration,
    },
    /// The agent asked to be restarted.
    RestartRequested,
    /// The agent crashed again after too many recent restarts; the supervisor stops.
    GaveUp {
        cause: CrashCause,
        uptime: Duration,
        restarts: usize,
    },
}

/// How supervision ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The agent quit normally.
    AgentQuit,
    /// The agent kept crashing and was not restarted again.
    CrashLoop,
}

/// Runs the agent until it quits or crash-loops. `spawn` starts one agent process; `report`
/// receives every crash and restart.
pub fn supervise(
    mut spawn: impl FnMut() -> io::Result<Child>,
    policy: RestartPolicy,
    mut report: impl FnMut(&SupervisorEvent),
) -> Outcome {
    let mut guard = CrashLoopGuard::new(policy);
    loop {
        let started = Instant::now();
        let exit = match spawn().and_then(|mut child| child.wait()) {
            Ok(status) => classify(status),
            Err(_) => AgentExit::Crashed(CrashCause::LaunchFailed),
        };
        let cause = match exit {
            AgentExit::Quit => return Outcome::AgentQuit,
            AgentExit::RestartRequested => {
                report(&SupervisorEvent::RestartRequested);
                continue;
            }
            AgentExit::Crashed(cause) => cause,
        };
        let uptime = started.elapsed();
        if !guard.allow_restart(Instant::now()) {
            report(&SupervisorEvent::GaveUp {
                cause,
                uptime,
                restarts: guard.recent_restarts(),
            });
            return Outcome::CrashLoop;
        }
        report(&SupervisorEvent::Crashed {
            cause,
            uptime,
            recent_restarts: guard.recent_restarts(),
            delay: policy.restart_delay,
        });
        std::thread::sleep(policy.restart_delay);
    }
}
