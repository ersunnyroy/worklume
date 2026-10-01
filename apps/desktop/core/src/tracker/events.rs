//! The events the tracker adds to the outbox. Their shapes are the `DesktopEvent` schemas in
//! `packages/api-contract/openapi.yaml`; a fixture test keeps the two in step.
//!
//! The envelope time of every event is the moment the change took effect, which is not always
//! when it was recorded: `idle.detected` is dated when idle began, a restart's `timer.stopped`
//! when the last snapshot was taken, and `slot.closed` at the end of its window, as are the
//! slot's `app_usage.batch` and `url_usage.batch`.

use serde_json::{Value, json};
use uuid::Uuid;
use worklume_storage::{EventId, EventType, NewEvent, Payload};

use super::TrackerError;
use super::state::{DecidedBy, Entry, IdlePeriod, IdleResolution, PauseReason, WorkTarget};
use crate::time::Moment;

/// Why a time entry ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    User,
    Switch,
    Pause(PauseReason),
    Break,
    /// No tick for longer than `max_tick_gap`: the agent was frozen or the device slept unseen.
    Gap,
    /// The agent restarted; time since the last snapshot is unaccounted.
    AgentRestart,
}

impl StopReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Switch => "switch",
            Self::Pause(PauseReason::Sleep) => "sleep",
            Self::Pause(PauseReason::SessionLocked) => "session_locked",
            Self::Break => "break",
            Self::Gap => "gap",
            Self::AgentRestart => "agent_restart",
        }
    }
}

/// Why a time entry began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartTrigger {
    User,
    Switch,
    Resume,
    BreakEnd,
    GapEnd,
    Restart,
}

impl StartTrigger {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Switch => "switch",
            Self::Resume => "resume",
            Self::BreakEnd => "break_end",
            Self::GapEnd => "gap_end",
            Self::Restart => "restart",
        }
    }
}

fn event(
    id: Uuid,
    event_type: &str,
    at: &Moment,
    payload: Payload,
) -> Result<NewEvent, TrackerError> {
    Ok(NewEvent {
        event_id: EventId::from_uuid(id)?,
        event_type: EventType::new(event_type)?,
        time: at.to_timestamp()?,
        // Measured by the sync engine (E09-S05); unknown until then.
        clock_offset_ms: None,
        payload,
    })
}

/// A payload from its fields.
pub(crate) fn fields<const N: usize>(pairs: [(&str, Value); N]) -> Payload {
    pairs
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

fn target_fields(target: &WorkTarget) -> (Value, Value) {
    (
        json!(target.project_id),
        target.task_id.map_or(Value::Null, |task| json!(task)),
    )
}

pub(crate) fn timer_started(
    entry: &Entry,
    trigger: StartTrigger,
) -> Result<NewEvent, TrackerError> {
    let (project_id, task_id) = target_fields(&entry.target);
    event(
        entry.time_entry_id,
        "timer.started",
        &entry.started,
        fields([
            ("project_id", project_id),
            ("task_id", task_id),
            ("trigger", json!(trigger.as_str())),
        ]),
    )
}

pub(crate) fn timer_stopped(
    time_entry_id: Uuid,
    reason: StopReason,
    at: &Moment,
) -> Result<NewEvent, TrackerError> {
    event(
        new_id(),
        "timer.stopped",
        at,
        fields([
            ("time_entry_id", json!(time_entry_id)),
            ("reason", json!(reason.as_str())),
        ]),
    )
}

pub(crate) fn idle_detected(
    idle_period_id: Uuid,
    time_entry_id: Uuid,
    since: &Moment,
) -> Result<NewEvent, TrackerError> {
    event(
        idle_period_id,
        "idle.detected",
        since,
        fields([("time_entry_id", json!(time_entry_id))]),
    )
}

pub(crate) fn idle_resolved(
    idle: &IdlePeriod,
    resolution: IdleResolution,
    decided_by: DecidedBy,
    at: &Moment,
) -> Result<NewEvent, TrackerError> {
    let reassigned = match resolution {
        IdleResolution::Reassign { project_id } => json!(project_id),
        _ => Value::Null,
    };
    event(
        new_id(),
        "idle.resolved",
        at,
        fields([
            ("idle_period_id", json!(idle.idle_period_id)),
            ("time_entry_id", json!(idle.time_entry_id)),
            ("resolution", json!(resolution.as_str())),
            ("reassigned_project_id", reassigned),
            ("decided_by", json!(decided_by.as_str())),
        ]),
    )
}

pub(crate) fn break_started(
    break_id: Uuid,
    break_policy_id: Uuid,
    at: &Moment,
) -> Result<NewEvent, TrackerError> {
    event(
        break_id,
        "break.started",
        at,
        fields([("break_policy_id", json!(break_policy_id))]),
    )
}

pub(crate) fn break_ended(break_id: Uuid, at: &Moment) -> Result<NewEvent, TrackerError> {
    event(
        new_id(),
        "break.ended",
        at,
        fields([("break_id", json!(break_id))]),
    )
}

pub(crate) fn slot_closed(
    slot_id: Uuid,
    at: &Moment,
    payload: Payload,
) -> Result<NewEvent, TrackerError> {
    event(slot_id, "slot.closed", at, payload)
}

/// A new, time-ordered ID for a time entry, slot, idle period, break or event.
/// Time per app in a slot: `apps` are `{identifier, name, title, seconds}` objects.
pub(crate) fn app_usage_batch(
    slot_id: Uuid,
    slot_start: &str,
    at: &Moment,
    apps: Vec<Value>,
) -> Result<NewEvent, TrackerError> {
    event(
        new_id(),
        "app_usage.batch",
        at,
        fields([
            ("slot_id", json!(slot_id)),
            ("slot_start", json!(slot_start)),
            ("apps", Value::Array(apps)),
        ]),
    )
}

/// Time per website in a slot: `sites` are `{domain, path, browser, private_window, seconds}`
/// objects.
pub(crate) fn url_usage_batch(
    slot_id: Uuid,
    slot_start: &str,
    at: &Moment,
    sites: Vec<Value>,
) -> Result<NewEvent, TrackerError> {
    event(
        new_id(),
        "url_usage.batch",
        at,
        fields([
            ("slot_id", json!(slot_id)),
            ("slot_start", json!(slot_start)),
            ("sites", Value::Array(sites)),
        ]),
    )
}

pub(crate) fn new_id() -> Uuid {
    *EventId::new_v7().as_uuid()
}
