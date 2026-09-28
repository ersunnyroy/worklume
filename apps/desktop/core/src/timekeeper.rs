//! The timekeeper (E09-S08): runs the tracker on the device's clock, detects idle time and
//! handles sleep.
//!
//! **One timeline.** Every timer action goes through the timekeeper. Before any action it reads
//! the clock, deals with a sleep that happened since the previous reading, and accounts the time
//! up to now, so an action taken just after waking can never land before the sleep.
//!
//! **Idle.** While the timer runs, inactivity is the time since the member's last input, counted
//! at the earliest from the last decision about idle time or the timer's start. When it reaches
//! the policy's timeout, the tracker records idle time from where inactivity began.
//!
//! **Sleep.** The clock reports the time spent asleep (see `worklume_platform::Clock`), so the
//! first reading after a wake knows how long the sleep lasted. The entry closes at the last
//! reading before it (`timer.stopped`, reason `sleep`), so sleep is never tracked. What happens on waking depends on how long the member was away: inactivity before
//! the sleep plus the sleep itself.
//!
//! - Shorter than the idle timeout: the timer resumes in a new entry.
//! - Longer: the member is asked once. Inactivity before the sleep becomes idle time awaiting a
//!   decision; if there was none, the question is only whether to resume. If idle time was
//!   already awaiting a decision when the device slept, that one question stands.
//!
//! **Keep-idle policy.** `prompt` asks the member. `always` keeps idle time without asking,
//! ending the idle period when input returns. `never` discards it and stops the timer where idle
//! began; a long absence with no idle time also stops the timer. The resulting `idle.resolved`
//! says the policy decided. The question after a long sleep is asked under `always` too: it is
//! about resuming, not about idle time.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;
use worklume_platform::{Clock, ClockError, ClockReading, IdleSource};
use worklume_storage::{StorageError, Store, Timestamp};

use crate::tracker::{
    Activity, DecidedBy, IdlePeriod, IdleResolution, PauseReason, Recovery, Tracker, TrackerConfig,
    TrackerError, TrackerState, WorkTarget,
};

/// A smaller growth in time asleep is noise from reading two counters microseconds apart.
const MIN_SLEEP_MS: i64 = 1_000;
/// Inactivity before a sleep shorter than this is not recorded as idle time: slots count whole
/// seconds.
const MIN_IDLE_MS: i64 = 1_000;

/// What happens to idle time: the organization's `idle.keep` policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepIdle {
    /// The member decides in the idle dialog.
    Prompt,
    /// Idle time is kept without asking.
    Always,
    /// Idle time is discarded and the timer stops, without asking.
    Never,
}

/// The idle policy that applies to the member, from the agent's configuration (E09-S06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdlePolicy {
    /// Inactivity after which time counts as idle (`idle.timeout_seconds`). `None` turns idle
    /// detection off; sleep is still never tracked.
    pub timeout: Option<Duration>,
    pub keep: KeepIdle,
}

/// A question for the member, shown by the idle dialog (E09-S09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// Idle time awaits the member's decision: keep, discard, reassign or stop. When `paused`,
    /// the device slept meanwhile; the answer also resumes the timer, unless it is `stop`.
    Idle { idle: IdlePeriod, paused: bool },
    /// The device slept for longer than the idle timeout. The timer resumes on `target` only
    /// when the member says so.
    Resume { target: WorkTarget },
}

impl Prompt {
    /// Whether two prompts ask the same question: the same idle period, or whether to resume.
    /// A pending idle decision that outlives a sleep stays the same question.
    #[must_use]
    pub fn same_question(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Idle { idle: a, .. }, Self::Idle { idle: b, .. }) => {
                a.idle_period_id == b.idle_period_id
            }
            (Self::Resume { .. }, Self::Resume { .. }) => true,
            _ => false,
        }
    }
}

/// What a call to the timekeeper did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// The device slept this long since the previous call.
    pub slept: Option<Duration>,
    /// A question the member was not being asked before this call, to announce.
    pub new_prompt: Option<Prompt>,
}

/// A failed call. Nothing is lost: the next call accounts the same time again.
#[derive(Debug, thiserror::Error)]
pub enum TimekeeperError {
    #[error(transparent)]
    Clock(#[from] ClockError),
    #[error("the clock gave an unusable reading: {0}")]
    Reading(StorageError),
    #[error(transparent)]
    Tracker(#[from] TrackerError),
}

/// The tracker on the device's clock. See the module documentation.
pub struct Timekeeper {
    tracker: Tracker,
    clock: Arc<dyn Clock>,
    idle: Arc<dyn IdleSource>,
    policy: IdlePolicy,
    /// The previous reading, to spot a sleep.
    last: ClockReading,
    /// Monotonic time of the member's last input, when the idle source knows it.
    last_input_mono: Option<i64>,
    /// Inactivity counts from here at the earliest.
    armed_mono: Option<i64>,
}

impl std::fmt::Debug for Timekeeper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timekeeper")
            .field("tracker", &self.tracker)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl Timekeeper {
    /// Restores the tracker (see [`Tracker::open`]) and applies the policy to its state.
    ///
    /// # Errors
    ///
    /// Fails when the clock, the store or the snapshot fails.
    pub fn open(
        store: Arc<Store>,
        config: TrackerConfig,
        clock: Arc<dyn Clock>,
        idle: Arc<dyn IdleSource>,
        policy: IdlePolicy,
    ) -> Result<(Self, Recovery), TimekeeperError> {
        let reading = clock.read()?;
        let (tracker, recovery) = Tracker::open(store, config, &timestamp(&reading)?)?;
        let mut keeper = Self {
            tracker,
            clock,
            idle,
            policy,
            last: reading,
            last_input_mono: None,
            armed_mono: None,
        };
        keeper.run(Activity::NONE, |_, _| Ok(()))?;
        Ok((keeper, recovery))
    }

    /// The tracker's state.
    #[must_use]
    pub fn state(&self) -> &TrackerState {
        self.tracker.state()
    }

    /// The question the member is being asked, if any.
    #[must_use]
    pub fn prompt(&self) -> Option<Prompt> {
        let asks = self.policy.keep == KeepIdle::Prompt;
        match self.tracker.state() {
            TrackerState::IdlePending { idle, .. } if asks => Some(Prompt::Idle {
                idle: idle.clone(),
                paused: false,
            }),
            TrackerState::Paused {
                idle: Some(idle), ..
            } if asks => Some(Prompt::Idle {
                idle: idle.clone(),
                paused: true,
            }),
            TrackerState::Paused {
                target,
                reason: PauseReason::Sleep,
                idle: None,
            } => Some(Prompt::Resume { target: *target }),
            _ => None,
        }
    }

    #[must_use]
    pub const fn policy(&self) -> IdlePolicy {
        self.policy
    }

    /// Replaces the idle policy, from the next call.
    pub const fn set_policy(&mut self, policy: IdlePolicy) {
        self.policy = policy;
    }

    /// Accounts time since the previous call, with the input measured meanwhile, and detects
    /// idle time. Call it about once a second.
    ///
    /// # Errors
    ///
    /// Fails when the clock or the store fails; the next call catches up.
    pub fn tick(&mut self, activity: Activity) -> Result<Outcome, TimekeeperError> {
        self.run(activity, Self::detect_idle)
    }

    /// Starts the timer on `target`. See [`Tracker::start`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::start`], or when the clock fails.
    pub fn start(&mut self, target: WorkTarget) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            keeper.tracker.start(target, now)?;
            keeper.arm(now);
            Ok(())
        })
    }

    /// Switches to `target`. See [`Tracker::switch`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::switch`], or when the clock fails.
    pub fn switch(&mut self, target: WorkTarget) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            Ok(keeper.tracker.switch(target, now)?)
        })
    }

    /// Stops the timer. See [`Tracker::stop`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::stop`], or when the clock fails.
    pub fn stop(&mut self) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            Ok(keeper.tracker.stop(now)?)
        })
    }

    /// Starts a break. See [`Tracker::start_break`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::start_break`], or when the clock fails.
    pub fn start_break(&mut self, break_policy_id: Uuid) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            Ok(keeper.tracker.start_break(break_policy_id, now)?)
        })
    }

    /// Ends the break. See [`Tracker::end_break`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::end_break`], or when the clock fails.
    pub fn end_break(&mut self) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            keeper.tracker.end_break(now)?;
            keeper.arm(now);
            Ok(())
        })
    }

    /// The member's answer to [`Prompt::Idle`]. After a sleep, any answer but `stop` also
    /// resumes the timer.
    ///
    /// # Errors
    ///
    /// Fails unless idle time awaits a decision, or when the clock or the store fails.
    pub fn resolve_idle(&mut self, resolution: IdleResolution) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            let paused = matches!(keeper.tracker.state(), TrackerState::Paused { .. });
            keeper
                .tracker
                .resolve_idle(resolution, DecidedBy::Member, now)?;
            if paused && resolution != IdleResolution::Stop {
                keeper.tracker.resume(now)?;
            }
            keeper.arm(now);
            Ok(())
        })
    }

    /// The member's answer to [`Prompt::Resume`]: resume the timer. (To stay stopped, call
    /// [`Timekeeper::stop`].)
    ///
    /// # Errors
    ///
    /// As [`Tracker::resume`], or when the clock fails.
    pub fn resume(&mut self) -> Result<Outcome, TimekeeperError> {
        self.run(Activity::NONE, |keeper, now| {
            keeper.tracker.resume(now)?;
            keeper.arm(now);
            Ok(())
        })
    }

    /// Reads the clock, catches up with any sleep and the time since the previous call, runs
    /// `action`, then applies the keep-idle policy.
    fn run(
        &mut self,
        activity: Activity,
        action: impl FnOnce(&mut Self, &Timestamp) -> Result<(), TimekeeperError>,
    ) -> Result<Outcome, TimekeeperError> {
        let before = self.prompt();
        let reading = self.clock.read()?;
        let now = timestamp(&reading)?;
        let slept = self.catch_up(&reading, &now, activity)?;
        action(self, &now)?;
        self.settle(&now)?;
        let after = self.prompt();
        Ok(Outcome {
            slept,
            new_prompt: after.filter(|prompt| {
                before
                    .as_ref()
                    .is_none_or(|before| !before.same_question(prompt))
            }),
        })
    }

    fn catch_up(
        &mut self,
        reading: &ClockReading,
        now: &Timestamp,
        activity: Activity,
    ) -> Result<Option<Duration>, TimekeeperError> {
        let same_boot =
            reading.boot_id == self.last.boot_id && reading.mono_ms >= self.last.mono_ms;
        if !same_boot {
            // Input and arming times belong to the previous boot's clock.
            self.last_input_mono = None;
            self.armed_mono = None;
        }
        let asleep = if same_boot {
            sleep_between(&self.last, reading)?
        } else {
            None
        };
        let slept = if let Some((asleep_at, slept_ms)) = asleep {
            self.fall_asleep(&asleep_at, slept_ms, now, activity)?;
            self.tracker.tick(now, Activity::NONE)?;
            Some(Duration::from_millis(u64::try_from(slept_ms).unwrap_or(0)))
        } else {
            self.tracker.tick(now, activity)?;
            None
        };
        // Only once the sleep is dealt with: a failure above sees the same sleep next time.
        self.last = reading.clone();
        self.last_input_mono = self
            .idle
            .idle_for()
            .ok()
            .map(|idle_for| reading.mono_ms.saturating_sub(millis(idle_for)));
        Ok(slept)
    }

    /// The device slept from `asleep_at` for `slept_ms` and woke at `now`.
    fn fall_asleep(
        &mut self,
        asleep_at: &Timestamp,
        slept_ms: i64,
        now: &Timestamp,
        activity: Activity,
    ) -> Result<(), TimekeeperError> {
        let inactive_since = self.inactive_since();
        let inactive_ms = inactive_since.map_or(0, |since| (asleep_at.mono_ms() - since).max(0));
        let away_long = self
            .policy
            .timeout
            .is_some_and(|timeout| inactive_ms + slept_ms >= millis(timeout));

        match self.tracker.state() {
            TrackerState::Running { .. } => {
                self.tracker.tick(asleep_at, activity)?;
                if away_long
                    && let Some(since) = inactive_since
                    && asleep_at.mono_ms() - since >= MIN_IDLE_MS
                {
                    let since = moment_before(asleep_at, since)?;
                    self.tracker.idle_detected(&since, asleep_at)?;
                }
                self.tracker.pause(PauseReason::Sleep, asleep_at)?;
            }
            TrackerState::IdlePending { .. } => {
                self.tracker.tick(asleep_at, activity)?;
                self.tracker.pause(PauseReason::Sleep, asleep_at)?;
            }
            _ => {}
        }
        if !away_long
            && matches!(
                self.tracker.state(),
                TrackerState::Paused {
                    reason: PauseReason::Sleep,
                    idle: None,
                    ..
                }
            )
        {
            self.tracker.resume(now)?;
        }
        Ok(())
    }

    /// Records idle time once inactivity reaches the timeout.
    fn detect_idle(&mut self, now: &Timestamp) -> Result<(), TimekeeperError> {
        let (Some(timeout), TrackerState::Running { .. }, Some(since)) = (
            self.policy.timeout,
            self.tracker.state(),
            self.inactive_since(),
        ) else {
            return Ok(());
        };
        if now.mono_ms() - since >= millis(timeout) {
            let since = moment_before(now, since)?;
            self.tracker.idle_detected(&since, now)?;
        }
        Ok(())
    }

    /// Applies the keep-idle policy to the current state.
    fn settle(&mut self, now: &Timestamp) -> Result<(), TimekeeperError> {
        let keep = self.policy.keep;
        match self.tracker.state() {
            TrackerState::IdlePending { .. } | TrackerState::Paused { idle: Some(_), .. }
                if keep == KeepIdle::Never =>
            {
                self.tracker
                    .resolve_idle(IdleResolution::Stop, DecidedBy::Policy, now)?;
            }
            TrackerState::Paused {
                reason: PauseReason::Sleep,
                idle: None,
                ..
            } if keep == KeepIdle::Never => {
                self.tracker.stop(now)?;
            }
            TrackerState::Paused { idle: Some(_), .. } if keep == KeepIdle::Always => {
                self.tracker
                    .resolve_idle(IdleResolution::Keep, DecidedBy::Policy, now)?;
            }
            TrackerState::IdlePending { idle, .. }
                if keep == KeepIdle::Always && self.input_since(idle) =>
            {
                self.tracker
                    .resolve_idle(IdleResolution::Keep, DecidedBy::Policy, now)?;
                self.arm(now);
            }
            _ => {}
        }
        Ok(())
    }

    /// Where inactivity began, as far as the idle source knows.
    fn inactive_since(&self) -> Option<i64> {
        let input = self.last_input_mono?;
        Some(
            self.armed_mono
                .map_or(input, |armed| armed.max(input))
                .max(0),
        )
    }

    /// Whether the member touched the keyboard or mouse after `idle` began.
    fn input_since(&self, idle: &IdlePeriod) -> bool {
        self.last_input_mono
            .is_some_and(|input| input > idle.since.mono_ms)
    }

    const fn arm(&mut self, now: &Timestamp) {
        self.armed_mono = Some(now.mono_ms());
    }
}

/// Where a sleep between two readings of one boot began, and how long it lasted.
///
/// The sleep is taken to begin at the earlier reading. The interval also holds up to a tick's
/// worth of awake time, split in an unknown way around the sleep; it is not tracked, so a sleep
/// never adds time or idle time that cannot be placed.
fn sleep_between(
    previous: &ClockReading,
    current: &ClockReading,
) -> Result<Option<(Timestamp, i64)>, TimekeeperError> {
    let elapsed = current.mono_ms - previous.mono_ms;
    let slept = (current.suspended_ms - previous.suspended_ms).min(elapsed);
    if slept < MIN_SLEEP_MS {
        return Ok(None);
    }
    Ok(Some((timestamp(previous)?, slept)))
}

/// The moment at monotonic `mono_ms`, earlier in the same boot as `now`.
fn moment_before(now: &Timestamp, mono_ms: i64) -> Result<Timestamp, TimekeeperError> {
    let back = (now.mono_ms() - mono_ms).max(0);
    Timestamp::new(
        now.wall_ms().saturating_sub(back).max(0),
        now.mono_ms() - back,
        now.boot_id(),
    )
    .map_err(TimekeeperError::Reading)
}

fn timestamp(reading: &ClockReading) -> Result<Timestamp, TimekeeperError> {
    Timestamp::new(reading.wall_ms, reading.mono_ms, reading.boot_id.clone())
        .map_err(TimekeeperError::Reading)
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}
