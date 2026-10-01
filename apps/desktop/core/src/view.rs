//! What the member sees of the timer (E09-S09), for the agent's UI.
//!
//! The UI only shows a [`TimerView`]: what the timer is doing and on what, today's total, the
//! question being asked, and which actions are possible now. The rules behind it (what may be
//! done in which state, how idle time counts) stay here, so the UI holds no tracking state and
//! makes no decisions of its own.

use std::time::Duration;

use uuid::Uuid;

use crate::catalog::Catalog;
use crate::timekeeper::{Prompt, Timekeeper};
use crate::tracker::{PauseReason, TrackerState, WorkTarget};

/// The timer as the member sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerView {
    pub status: TimerStatus,
    /// What the timer runs on, or resumes on after a pause or a break.
    pub work: Option<WorkView>,
    /// Tracked today: since the start of the day the caller names, idle time awaiting a decision
    /// included.
    pub today: Duration,
    /// The question the member is being asked, if any.
    pub question: Option<Question>,
    pub allowed: Allowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerStatus {
    Stopped,
    Running,
    /// Idle was detected; time still counts until the member decides.
    Idle,
    Paused(PauseReason),
    /// On a break under the named policy, when the catalog knows it.
    OnBreak {
        policy: Option<String>,
    },
}

/// A project and task, with their names when the catalog knows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkView {
    pub project_id: Uuid,
    pub project_name: Option<String>,
    pub task_id: Option<Uuid>,
    pub task_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Idle time awaits a decision: keep, discard, reassign or stop. When `paused`, the device
    /// slept or the session was locked meanwhile, and any answer but stop also resumes the
    /// timer.
    Idle {
        /// Where inactivity began, in the tracker's wall time.
        since_wall_ms: i64,
        /// How long ago that was, sleep included; unknown after a reboot.
        idle_for: Option<Duration>,
        paused: bool,
    },
    /// The device slept, or the session was locked, for longer than the idle timeout: resume,
    /// or stay stopped.
    Resume,
}

/// What the member may do now. Answering a question is allowed exactly while it is asked.
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per action the UI offers, nothing to combine"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allowed {
    pub start: bool,
    pub switch: bool,
    pub stop: bool,
    pub start_break: bool,
    pub end_break: bool,
}

/// The timer as the member sees it now, with names from `catalog` and today counted from
/// `day_start_wall_ms` (the member's local midnight).
#[must_use]
pub fn timer_view(keeper: &Timekeeper, catalog: &Catalog, day_start_wall_ms: i64) -> TimerView {
    let state = keeper.state();
    let has_projects = !catalog.projects.is_empty();
    let has_breaks = !catalog.break_policies.is_empty();
    let (status, target) = match state {
        TrackerState::Stopped => (TimerStatus::Stopped, None),
        TrackerState::Running { entry } => (TimerStatus::Running, Some(entry.target)),
        TrackerState::IdlePending { entry, .. } => (TimerStatus::Idle, Some(entry.target)),
        TrackerState::Paused { target, reason, .. } => {
            (TimerStatus::Paused(*reason), Some(*target))
        }
        TrackerState::OnBreak {
            break_policy_id,
            resume_to,
            ..
        } => (
            TimerStatus::OnBreak {
                policy: catalog
                    .break_policy(*break_policy_id)
                    .ok()
                    .map(|policy| policy.name.clone()),
            },
            *resume_to,
        ),
    };
    let running = matches!(state, TrackerState::Running { .. });
    let stopped = matches!(state, TrackerState::Stopped);
    TimerView {
        status,
        work: target.map(|target| work_view(catalog, target)),
        today: keeper.tracked_since(day_start_wall_ms),
        question: keeper.prompt().map(|prompt| question(keeper, &prompt)),
        allowed: Allowed {
            start: stopped && has_projects,
            switch: running && has_projects,
            stop: running || matches!(state, TrackerState::Paused { idle: None, .. }),
            start_break: (running || stopped) && has_breaks,
            end_break: matches!(state, TrackerState::OnBreak { .. }),
        },
    }
}

fn work_view(catalog: &Catalog, target: WorkTarget) -> WorkView {
    let project = catalog.project(target.project_id);
    WorkView {
        project_id: target.project_id,
        project_name: project.map(|project| project.name.clone()),
        task_id: target.task_id,
        task_name: target
            .task_id
            .and_then(|task_id| project.and_then(|project| project.task(task_id)))
            .map(|task| task.name.clone()),
    }
}

fn question(keeper: &Timekeeper, prompt: &Prompt) -> Question {
    match prompt {
        Prompt::Idle { idle, paused } => {
            let now = keeper.last_reading();
            let idle_for = (now.boot_id == idle.since.boot_id)
                .then(|| u64::try_from(now.mono_ms - idle.since.mono_ms).unwrap_or(0))
                .map(Duration::from_millis);
            Question::Idle {
                since_wall_ms: idle.since.wall_ms,
                idle_for,
                paused: *paused,
            }
        }
        Prompt::Resume { .. } => Question::Resume,
    }
}
