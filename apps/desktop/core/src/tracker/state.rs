//! The tracker's states and the values they carry.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::time::Moment;

/// What the timer runs on. The IDs come from the server, which checks that the member may track
/// time on them (the agent never trusts its own copy of project assignments).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkTarget {
    pub project_id: Uuid,
    pub task_id: Option<Uuid>,
}

/// An open time entry: one continuous interval on one target. Its ID becomes the server's time
/// entry ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub time_entry_id: Uuid,
    pub target: WorkTarget,
    pub started: Moment,
}

/// An idle period waiting for the member's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdlePeriod {
    pub idle_period_id: Uuid,
    /// The entry that was running when idle began; the server adjusts it on resolution.
    pub time_entry_id: Uuid,
    pub since: Moment,
}

/// Why tracking paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    Sleep,
    SessionLocked,
}

/// Who decided about an idle period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecidedBy {
    /// The member, in the idle dialog.
    Member,
    /// The organization's keep-idle policy (`always` or `never`), without asking.
    Policy,
}

impl DecidedBy {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Policy => "policy",
        }
    }
}

/// The decision about an idle period (the idle dialog, or the keep-idle policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleResolution {
    /// Count the idle time as worked.
    Keep,
    /// Remove the idle time; the timer keeps running.
    Discard,
    /// Move the idle time to another project; the timer keeps running on its own.
    Reassign { project_id: Uuid },
    /// Remove the idle time and stop the timer where idle began.
    Stop,
}

impl IdleResolution {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Discard => "discard",
            Self::Reassign { .. } => "reassign",
            Self::Stop => "stop",
        }
    }
}

/// The tracker's state machine (E09-S04): stopped, running, idle-pending, paused and on-break.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TrackerState {
    Stopped,
    Running {
        entry: Entry,
    },
    /// Idle was detected. Time still counts, as idle, until the member decides.
    IdlePending {
        entry: Entry,
        idle: IdlePeriod,
    },
    /// The device slept or the session locked. The server's time entry is closed; `resume`
    /// opens a new one on the same target. If idle time was awaiting a decision when the pause
    /// began, it still does: the idle period ended with the entry, and `resume` waits for the
    /// decision.
    Paused {
        target: WorkTarget,
        reason: PauseReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle: Option<IdlePeriod>,
    },
    /// On a break. Break time is not tracked; ending the break resumes `resume_to`, if the timer
    /// was running.
    OnBreak {
        break_id: Uuid,
        break_policy_id: Uuid,
        resume_to: Option<WorkTarget>,
    },
}

impl TrackerState {
    /// Idle time awaiting a decision, while tracking or paused.
    #[must_use]
    pub fn pending_idle(&self) -> Option<&IdlePeriod> {
        match self {
            Self::IdlePending { idle, .. }
            | Self::Paused {
                idle: Some(idle), ..
            } => Some(idle),
            _ => None,
        }
    }

    /// The open entry, while time counts.
    #[must_use]
    pub fn entry(&self) -> Option<&Entry> {
        match self {
            Self::Running { entry } | Self::IdlePending { entry, .. } => Some(entry),
            _ => None,
        }
    }

    /// Whether time counts right now.
    #[must_use]
    pub fn is_tracking(&self) -> bool {
        self.entry().is_some()
    }

    #[must_use]
    pub const fn name(&self) -> StateName {
        match self {
            Self::Stopped => StateName::Stopped,
            Self::Running { .. } => StateName::Running,
            Self::IdlePending { .. } => StateName::IdlePending,
            Self::Paused { .. } => StateName::Paused,
            Self::OnBreak { .. } => StateName::OnBreak,
        }
    }
}

/// A state, without its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateName {
    Stopped,
    Running,
    IdlePending,
    Paused,
    OnBreak,
}

impl fmt::Display for StateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stopped => "stopped",
            Self::Running => "running",
            Self::IdlePending => "waiting for an idle decision",
            Self::Paused => "paused",
            Self::OnBreak => "on a break",
        })
    }
}

/// What was asked of the tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Switch,
    Stop,
    ResolveIdle,
    Pause,
    Resume,
    StartBreak,
    EndBreak,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Start => "starting the timer",
            Self::Switch => "switching projects",
            Self::Stop => "stopping the timer",
            Self::ResolveIdle => "resolving idle time",
            Self::Pause => "pausing",
            Self::Resume => "resuming",
            Self::StartBreak => "starting a break",
            Self::EndBreak => "ending a break",
        })
    }
}
