//! The tracker (E09-S04): the timer's state machine and 10-minute slot aggregation.
//!
//! **Journal first.** Every action runs on a copy of the tracker. The copy's snapshot and the
//! outbox events it produced are written to the store in one transaction, and only then does
//! the copy replace the live tracker. If the write fails, nothing changed, and the next call
//! simply accounts the time again. The journal holds a single snapshot: each write replaces the
//! previous one. While time counts, a snapshot is also written every `heartbeat_interval`, which
//! bounds what a crash can lose.
//!
//! **Time.** Durations come only from the monotonic clock. Wall time is derived from one anchor
//! per boot (`anchor.wall + (now.mono − anchor.mono)`), so changing the system clock neither
//! adds nor removes tracked time; it only flags the slot `clock_adjusted`. The server corrects
//! the anchor's offset with the clock offset the sync engine measures (E09-S05).
//!
//! **Gaps.** If no tick arrives for longer than `max_tick_gap` while time counts, the gap is
//! not counted: the entry is closed where ticks stopped and a new one opens now.
//!
//! **Restarts.** [`Tracker::open`] restores the last snapshot. If time was counting, the entry
//! is closed at the snapshot and a new one opens now: time between the two is unaccounted, and
//! the timer carries on.
//!
//! **Reboots.** A new boot re-anchors derived time to the wall clock, but never before the last
//! accounted moment, so no 10-minute window comes around twice. Open slots stay open across the
//! reboot: if the new boot lands in the same window, it adds to the same slot.
//!
//! **Today.** A small ledger keeps the tracked time per 10-minute window for two days, with idle
//! time pending until the member decides, for the agent's day total (see
//! [`Tracker::tracked_since`]).

mod events;
mod ledger;
mod slots;
mod state;

use std::sync::Arc;
use std::time::Duration;

use worklume_platform::InputEvent;
use worklume_storage::{
    JournalId, JournalKind, NewEvent, NewJournalEntry, StorageError, Store, Timestamp,
};

pub use slots::{Activity, InputMethod, SLOT_MS};
pub use state::{
    Action, DecidedBy, Entry, IdlePeriod, IdleResolution, PauseReason, StateName, TrackerState,
    WorkTarget,
};

use events::{StartTrigger, StopReason};
use slots::{Interval, Slots};

use crate::time::Moment;
use crate::usage::UsageSample;

/// What the device observed since the previous tick, besides the activity totals.
#[derive(Debug, Clone, Copy)]
pub struct Observation<'a> {
    /// What was in front (E09-S12), already reduced to what the usage policy keeps.
    pub usage: &'a UsageSample,
    /// Input events (E09-S13), oldest first. Only their timing is counted, and only while the
    /// timer runs and the member isn't idle.
    pub input: &'a [InputEvent],
}

impl Observation<'_> {
    /// Nothing observed.
    pub const NONE: Observation<'static> = Observation {
        usage: &UsageSample::NONE,
        input: &[],
    };
}

/// Journal kind of the tracker's snapshot.
pub const SNAPSHOT_KIND: &str = "tracker.snapshot";
const SNAPSHOT_VERSION: u32 = 1;

/// Timings that shape the tracker. The defaults suit a tick every second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackerConfig {
    /// While time counts, a snapshot at least this often: the most a crash can lose.
    pub heartbeat_interval: Duration,
    /// A longer silence between ticks is a gap and is not counted.
    pub max_tick_gap: Duration,
    /// A wall clock further than this from the derived time flags the slot `clock_adjusted`.
    pub clock_jump_threshold: Duration,
    pub input_method: InputMethod,
}

impl TrackerConfig {
    #[must_use]
    pub const fn new(input_method: InputMethod) -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            max_tick_gap: Duration::from_mins(1),
            clock_jump_threshold: Duration::from_mins(1),
            input_method,
        }
    }
}

/// A failed tracker call. The tracker is unchanged afterwards.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("{action} is not possible while the tracker is {state}")]
    InvalidTransition { state: StateName, action: Action },
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("the saved tracker state cannot be used: {0}")]
    Snapshot(&'static str),
}

/// What [`Tracker::open`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// A snapshot was restored (otherwise the tracker starts stopped).
    pub restored: bool,
    /// Time was counting when the agent stopped; the entry was split at the last snapshot.
    pub resumed_tracking: bool,
    /// Milliseconds between the last snapshot and now that were not counted, when both fall in
    /// the same boot.
    pub unaccounted_ms: Option<u64>,
}

/// Everything the snapshot holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Core {
    version: u32,
    state: TrackerState,
    /// The raw clock reading derived wall time is computed from, for the current boot.
    anchor: Option<Moment>,
    /// Derived time of the last accounting.
    last_tick: Option<Moment>,
    /// Monotonic time of the last snapshot, for heartbeats.
    last_saved_mono: Option<i64>,
    slots: Slots,
}

impl Core {
    fn new() -> Self {
        Self {
            version: SNAPSHOT_VERSION,
            state: TrackerState::Stopped,
            anchor: None,
            last_tick: None,
            last_saved_mono: None,
            slots: Slots::default(),
        }
    }

    /// Converts a raw reading to derived time, re-anchoring on the first reading of a boot.
    /// Returns the derived moment, whether the boot changed, and whether the wall clock has
    /// moved away from derived time.
    fn observe(&mut self, now: &Timestamp, config: &TrackerConfig) -> (Moment, bool, bool) {
        let raw = Moment::from_timestamp(now);
        // A monotonic clock that went backwards has restarted, whatever the boot ID says.
        let rebooted = self
            .anchor
            .as_ref()
            .is_some_and(|anchor| anchor.boot_id != raw.boot_id || raw.mono_ms < anchor.mono_ms);
        let anchor = match &self.anchor {
            Some(anchor) if !rebooted => anchor.clone(),
            _ => {
                // A new boot anchors to the device's wall clock, but never before time already
                // accounted, so a clock set back while the device was off cannot bring a
                // 10-minute window around twice.
                let mut anchor = raw.clone();
                if let Some(last) = &self.last_tick {
                    anchor.wall_ms = anchor.wall_ms.max(last.wall_ms);
                }
                self.anchor = Some(anchor.clone());
                anchor
            }
        };
        let elapsed = (raw.mono_ms - anchor.mono_ms).max(0);
        let derived = Moment {
            wall_ms: anchor.wall_ms.saturating_add(elapsed),
            mono_ms: raw.mono_ms,
            boot_id: raw.boot_id.clone(),
        };
        let threshold = i64::try_from(config.clock_jump_threshold.as_millis()).unwrap_or(i64::MAX);
        let jumped = (raw.wall_ms - derived.wall_ms).abs() > threshold;
        (derived, rebooted, jumped)
    }

    /// Accounts time from the last tick to `now` in the current state, and closes slots whose
    /// window has passed. Returns the derived moment of `now`.
    fn advance(
        &mut self,
        now: &Timestamp,
        activity: &Activity,
        observed: &Observation<'_>,
        config: &TrackerConfig,
        out: &mut Vec<NewEvent>,
    ) -> Result<Moment, TrackerError> {
        let (current, rebooted, jumped) = self.observe(now, config);
        let last = self.last_tick.take();
        let mut accounted = false;
        // Time cannot be measured across a reboot; recovery handles the timer itself. Open
        // slots stay open: the new boot may carry on in the same window.
        if !rebooted && let (Some(last), Some(entry)) = (last, self.state.entry().cloned()) {
            let elapsed = u64::try_from(current.mono_ms - last.mono_ms).unwrap_or(0);
            let max_gap = u64::try_from(config.max_tick_gap.as_millis()).unwrap_or(u64::MAX);
            if elapsed > max_gap {
                self.split_entry(&last, &current, StopReason::Gap, StartTrigger::GapEnd, out)?;
            } else if let Some(anchor) = self.anchor.clone() {
                self.slots.account(&Interval {
                    from_wall_ms: last.wall_ms,
                    from_mono_ms: last.mono_ms,
                    elapsed_ms: elapsed,
                    activity: *activity,
                    entry: &entry,
                    idle: matches!(self.state, TrackerState::IdlePending { .. }),
                    clock_adjusted: jumped,
                    anchor: &anchor,
                    usage: observed.usage,
                    input: observed.input,
                });
                accounted = true;
            }
        }
        // Time that doesn't count breaks the rhythm of input: no interval may span it.
        if !accounted {
            self.slots.forget_rhythm();
        }
        if jumped {
            self.slots.flag_clock_adjusted(current.wall_ms);
        }
        self.slots
            .close_due(current.wall_ms, config.input_method, out)?;
        self.last_tick = Some(current.clone());
        Ok(current)
    }

    /// Closes the open entry at `stopped` and opens a new one on the same target at `started`.
    fn split_entry(
        &mut self,
        stopped: &Moment,
        started: &Moment,
        reason: StopReason,
        trigger: StartTrigger,
        out: &mut Vec<NewEvent>,
    ) -> Result<(), TrackerError> {
        let Some(entry) = self.state.entry().cloned() else {
            return Ok(());
        };
        out.push(events::timer_stopped(entry.time_entry_id, reason, stopped)?);
        let next = new_entry(entry.target, started);
        out.push(events::timer_started(&next, trigger)?);
        match &mut self.state {
            TrackerState::Running { entry } | TrackerState::IdlePending { entry, .. } => {
                *entry = next;
            }
            _ => {}
        }
        Ok(())
    }
}

fn new_entry(target: WorkTarget, started: &Moment) -> Entry {
    Entry {
        time_entry_id: events::new_id(),
        target,
        started: started.clone(),
    }
}

/// The agent's timer. See the module documentation.
pub struct Tracker {
    store: Arc<Store>,
    config: TrackerConfig,
    core: Core,
    /// The journal entry holding the current snapshot, removed when the next one is written.
    snapshot_id: Option<JournalId>,
}

impl std::fmt::Debug for Tracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tracker")
            .field("state", &self.core.state.name())
            .finish_non_exhaustive()
    }
}

impl Tracker {
    /// Restores the tracker from the store's journal, or starts it stopped. See the module
    /// documentation for what happens when time was counting.
    ///
    /// # Errors
    ///
    /// Fails when the store fails or the snapshot cannot be read.
    pub fn open(
        store: Arc<Store>,
        config: TrackerConfig,
        now: &Timestamp,
    ) -> Result<(Self, Recovery), TrackerError> {
        let journal = store.journal_after(None, usize::MAX)?;
        let saved = journal
            .iter()
            .rev()
            .find(|entry| entry.kind.as_str() == SNAPSHOT_KIND);
        let (mut core, snapshot_id) = match saved {
            Some(entry) => {
                let core: Core =
                    serde_json::from_value(serde_json::Value::Object(entry.payload.clone()))
                        .map_err(|_| TrackerError::Snapshot("the snapshot is malformed"))?;
                if core.version != SNAPSHOT_VERSION {
                    return Err(TrackerError::Snapshot(
                        "the snapshot was written by another version of the agent",
                    ));
                }
                (core, Some(entry.id))
            }
            None => (Core::new(), None),
        };
        let restored = snapshot_id.is_some();

        let mut out = Vec::new();
        // Observed while the last tick is known, which a new boot's anchor must not precede.
        let (current, rebooted, _) = core.observe(now, &config);
        let last = core.last_tick.take();
        let resumed_tracking = core.state.is_tracking() && last.is_some();
        let unaccounted_ms = last.as_ref().and_then(|last| {
            (!rebooted && last.boot_id == current.boot_id)
                .then(|| u64::try_from(current.mono_ms - last.mono_ms).unwrap_or(0))
        });
        if let Some(last) = &last
            && core.state.is_tracking()
        {
            core.split_entry(
                last,
                &current,
                StopReason::AgentRestart,
                StartTrigger::Restart,
                &mut out,
            )?;
        }
        core.slots
            .close_due(current.wall_ms, config.input_method, &mut out)?;
        core.last_tick = Some(current.clone());

        let mut tracker = Self {
            store,
            config,
            core: core.clone(),
            snapshot_id,
        };
        tracker.save(&mut core, &out, &current)?;
        tracker.core = core;
        Ok((
            tracker,
            Recovery {
                restored,
                resumed_tracking,
                unaccounted_ms,
            },
        ))
    }

    /// The current state.
    #[must_use]
    pub fn state(&self) -> &TrackerState {
        &self.core.state
    }

    /// Time tracked from `from_wall_ms` (a day's start, say) until the last call, within the last
    /// two days. Idle time awaiting a decision counts; discarded idle time doesn't. Time is
    /// counted by 10-minute window, and a window belongs to the day it starts in.
    #[must_use]
    pub fn tracked_since(&self, from_wall_ms: i64) -> Duration {
        Duration::from_millis(self.core.slots.tracked_since(from_wall_ms))
    }

    /// The tracker's wall time at the last call, as its events are dated: derived from the
    /// monotonic clock, so a changed system clock doesn't move it (see the module
    /// documentation). The two days [`Tracker::tracked_since`] covers end here.
    #[must_use]
    pub fn wall_now_ms(&self) -> Option<i64> {
        self.core.last_tick.as_ref().map(|moment| moment.wall_ms)
    }

    /// Accounts time since the last call, with the input measured meanwhile. Call it about once
    /// a second; slots close and heartbeats are written as time passes.
    ///
    /// # Errors
    ///
    /// Fails when the store fails; the time is then accounted by the next call.
    pub fn tick(&mut self, now: &Timestamp, activity: Activity) -> Result<(), TrackerError> {
        self.tick_with(now, activity, &Observation::NONE)
    }

    /// As [`Tracker::tick`], with what the device observed meanwhile: the app in front and the
    /// input events.
    ///
    /// # Errors
    ///
    /// Fails when the store fails; the time is then accounted by the next call.
    pub fn tick_with(
        &mut self,
        now: &Timestamp,
        activity: Activity,
        observed: &Observation<'_>,
    ) -> Result<(), TrackerError> {
        self.run_with(now, activity, observed, |_, _, _| Ok(()))
    }

    /// Starts the timer on `target`.
    ///
    /// # Errors
    ///
    /// Fails unless the tracker is stopped, or when the store fails.
    pub fn start(&mut self, target: WorkTarget, now: &Timestamp) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            if !matches!(core.state, TrackerState::Stopped) {
                return Err(invalid(&core.state, Action::Start));
            }
            let entry = new_entry(target, at);
            out.push(events::timer_started(&entry, StartTrigger::User)?);
            core.state = TrackerState::Running { entry };
            Ok(())
        })
    }

    /// Stops the current entry and starts one on `target`, at the same moment.
    ///
    /// # Errors
    ///
    /// Fails unless the timer is running, or when the store fails.
    pub fn switch(&mut self, target: WorkTarget, now: &Timestamp) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            let TrackerState::Running { entry } = &core.state else {
                return Err(invalid(&core.state, Action::Switch));
            };
            out.push(events::timer_stopped(
                entry.time_entry_id,
                StopReason::Switch,
                at,
            )?);
            let entry = new_entry(target, at);
            out.push(events::timer_started(&entry, StartTrigger::Switch)?);
            core.state = TrackerState::Running { entry };
            Ok(())
        })
    }

    /// Stops the timer. While paused, the server's entry is already closed, so only the local
    /// state changes.
    ///
    /// # Errors
    ///
    /// Fails while idle time awaits a decision (the `stop` resolution stops the timer then), on
    /// a break or when already stopped, or when the store fails.
    pub fn stop(&mut self, now: &Timestamp) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            match &core.state {
                TrackerState::Running { entry } => {
                    out.push(events::timer_stopped(
                        entry.time_entry_id,
                        StopReason::User,
                        at,
                    )?);
                }
                TrackerState::Paused { idle: None, .. } => {}
                _ => return Err(invalid(&core.state, Action::Stop)),
            }
            core.state = TrackerState::Stopped;
            Ok(())
        })
    }

    /// Records that the member has been idle since `since`. Ignored unless the timer is running.
    /// Idle cannot begin before the entry did, or after `now`.
    ///
    /// # Errors
    ///
    /// Fails when the store fails.
    pub fn idle_detected(
        &mut self,
        since: &Timestamp,
        now: &Timestamp,
    ) -> Result<(), TrackerError> {
        let raw_since = Moment::from_timestamp(since);
        self.run(now, Activity::NONE, |core, at, out| {
            let TrackerState::Running { entry } = &core.state else {
                return Ok(());
            };
            // Derive `since` like any other reading, then keep it inside the entry.
            let offset = at.mono_ms - raw_since.mono_ms;
            let mut begun = Moment {
                wall_ms: at.wall_ms - offset.max(0),
                mono_ms: raw_since.mono_ms,
                boot_id: at.boot_id.clone(),
            };
            if raw_since.boot_id != at.boot_id || begun.mono_ms < entry.started.mono_ms {
                begun = entry.started.clone();
            }
            if begun.mono_ms > at.mono_ms {
                begun = at.clone();
            }
            // The time already counted since inactivity began now waits for the decision.
            core.slots.hold_idle(begun.wall_ms, at.wall_ms);
            let idle = IdlePeriod {
                idle_period_id: events::new_id(),
                time_entry_id: entry.time_entry_id,
                since: begun,
            };
            out.push(events::idle_detected(
                idle.idle_period_id,
                idle.time_entry_id,
                &idle.since,
            )?);
            core.state = TrackerState::IdlePending {
                entry: entry.clone(),
                idle,
            };
            Ok(())
        })
    }

    /// Applies a decision about pending idle time. The server adjusts the time entries. Locally
    /// a running timer keeps running and a paused one stays paused, except that
    /// [`IdleResolution::Stop`] stops it.
    ///
    /// # Errors
    ///
    /// Fails unless idle time is pending, or when the store fails.
    pub fn resolve_idle(
        &mut self,
        resolution: IdleResolution,
        decided_by: DecidedBy,
        now: &Timestamp,
    ) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            let Some(idle) = core.state.pending_idle() else {
                return Err(invalid(&core.state, Action::ResolveIdle));
            };
            out.push(events::idle_resolved(idle, resolution, decided_by, at)?);
            core.slots.settle_idle(matches!(
                resolution,
                IdleResolution::Keep | IdleResolution::Reassign { .. }
            ));
            core.state = match (resolution, &core.state) {
                (IdleResolution::Stop, _) => TrackerState::Stopped,
                (_, TrackerState::IdlePending { entry, .. }) => TrackerState::Running {
                    entry: entry.clone(),
                },
                (_, TrackerState::Paused { target, reason, .. }) => TrackerState::Paused {
                    target: *target,
                    reason: *reason,
                    idle: None,
                },
                (_, other) => other.clone(),
            };
            Ok(())
        })
    }

    /// Pauses for sleep or a locked session, closing the entry. Idle time awaiting a decision
    /// keeps waiting, and its period ends with the entry. Safe to repeat: ignored unless time
    /// counts.
    ///
    /// # Errors
    ///
    /// Fails when the store fails.
    pub fn pause(&mut self, reason: PauseReason, now: &Timestamp) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            let (entry, idle) = match &core.state {
                TrackerState::Running { entry } => (entry, None),
                TrackerState::IdlePending { entry, idle } => (entry, Some(idle.clone())),
                _ => return Ok(()),
            };
            out.push(events::timer_stopped(
                entry.time_entry_id,
                StopReason::Pause(reason),
                at,
            )?);
            core.state = TrackerState::Paused {
                target: entry.target,
                reason,
                idle,
            };
            Ok(())
        })
    }

    /// Resumes after a pause, in a new entry on the same target. Safe to repeat: ignored unless
    /// paused.
    ///
    /// # Errors
    ///
    /// Fails while idle time from before the pause awaits a decision, or when the store fails.
    pub fn resume(&mut self, now: &Timestamp) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            match &core.state {
                TrackerState::Paused {
                    target, idle: None, ..
                } => {
                    let entry = new_entry(*target, at);
                    out.push(events::timer_started(&entry, StartTrigger::Resume)?);
                    core.state = TrackerState::Running { entry };
                }
                TrackerState::Paused { idle: Some(_), .. } => {
                    return Err(invalid(&core.state, Action::Resume));
                }
                _ => {}
            }
            Ok(())
        })
    }

    /// Starts a break under `break_policy_id`, closing the running entry, if any.
    ///
    /// # Errors
    ///
    /// Fails unless the timer is running or stopped, or when the store fails.
    pub fn start_break(
        &mut self,
        break_policy_id: uuid::Uuid,
        now: &Timestamp,
    ) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            let resume_to = match &core.state {
                TrackerState::Running { entry } => {
                    out.push(events::timer_stopped(
                        entry.time_entry_id,
                        StopReason::Break,
                        at,
                    )?);
                    Some(entry.target)
                }
                TrackerState::Stopped => None,
                _ => return Err(invalid(&core.state, Action::StartBreak)),
            };
            let break_id = events::new_id();
            out.push(events::break_started(break_id, break_policy_id, at)?);
            core.state = TrackerState::OnBreak {
                break_id,
                break_policy_id,
                resume_to,
            };
            Ok(())
        })
    }

    /// Ends the break, resuming the timer if it was running when the break began.
    ///
    /// # Errors
    ///
    /// Fails unless on a break, or when the store fails.
    pub fn end_break(&mut self, now: &Timestamp) -> Result<(), TrackerError> {
        self.run(now, Activity::NONE, |core, at, out| {
            let TrackerState::OnBreak {
                break_id,
                resume_to,
                ..
            } = &core.state
            else {
                return Err(invalid(&core.state, Action::EndBreak));
            };
            out.push(events::break_ended(*break_id, at)?);
            core.state = match resume_to {
                Some(target) => {
                    let entry = new_entry(*target, at);
                    out.push(events::timer_started(&entry, StartTrigger::BreakEnd)?);
                    TrackerState::Running { entry }
                }
                None => TrackerState::Stopped,
            };
            Ok(())
        })
    }

    /// Runs `action` on a copy after accounting time up to `now`, saves it if anything needs
    /// saving, and only then makes the copy live.
    fn run(
        &mut self,
        now: &Timestamp,
        activity: Activity,
        action: impl FnOnce(&mut Core, &Moment, &mut Vec<NewEvent>) -> Result<(), TrackerError>,
    ) -> Result<(), TrackerError> {
        self.run_with(now, activity, &Observation::NONE, action)
    }

    fn run_with(
        &mut self,
        now: &Timestamp,
        activity: Activity,
        observed: &Observation<'_>,
        action: impl FnOnce(&mut Core, &Moment, &mut Vec<NewEvent>) -> Result<(), TrackerError>,
    ) -> Result<(), TrackerError> {
        let mut next = self.core.clone();
        let mut out = Vec::new();
        let current = next.advance(now, &activity, observed, &self.config, &mut out)?;
        action(&mut next, &current, &mut out)?;

        let heartbeat = u64::try_from(self.config.heartbeat_interval.as_millis()).unwrap_or(0);
        // A monotonic reading below the last save means a new boot: save at once.
        let heartbeat_due = next.state.is_tracking()
            && next.last_saved_mono.is_none_or(|saved| {
                u64::try_from(current.mono_ms - saved).map_or(true, |since| since >= heartbeat)
            });
        if !out.is_empty() || next.state != self.core.state || heartbeat_due {
            self.save(&mut next, &out, &current)?;
        }
        self.core = next;
        Ok(())
    }

    /// Writes `events` and the snapshot of `core` in one transaction, replacing the previous
    /// snapshot.
    fn save(
        &mut self,
        core: &mut Core,
        events: &[NewEvent],
        now: &Moment,
    ) -> Result<(), TrackerError> {
        core.last_saved_mono = Some(now.mono_ms);
        let Ok(serde_json::Value::Object(payload)) = serde_json::to_value(&*core) else {
            return Err(TrackerError::Snapshot("the snapshot cannot be encoded"));
        };
        let snapshot = NewJournalEntry {
            kind: JournalKind::new(SNAPSHOT_KIND)?,
            time: now.to_timestamp()?,
            payload,
        };
        let previous = self.snapshot_id;
        let id = self.store.write(|tx| {
            for event in events {
                tx.enqueue(event)?;
            }
            let id = tx.append_journal(&snapshot)?;
            if let Some(previous) = previous {
                tx.remove_journal_through(previous)?;
            }
            Ok(id)
        })?;
        self.snapshot_id = Some(id);
        Ok(())
    }
}

fn invalid(state: &TrackerState, action: Action) -> TrackerError {
    TrackerError::InvalidTransition {
        state: state.name(),
        action,
    }
}
