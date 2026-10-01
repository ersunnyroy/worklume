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
//!
//! **A locked session** (E12-S03) is an absence like a sleep, except that the device stays
//! awake. The entry closes at the reading before the lock was seen (`timer.stopped`, reason
//! `session_locked`), so locked time is never tracked. On unlocking, the absence (inactivity
//! before the lock plus the lock) decides as it does for a sleep: shorter than the idle timeout,
//! the timer resumes in a new entry; longer, the member is asked, and under `never` it stops. A
//! device that sleeps while locked resumes only once unlocked. Questions about resuming wait
//! until the session is unlocked, so the member sees them.
//!
//! **Input events** (E09-S13) are drained from the reader on every call, whatever the state, so
//! the reader never holds more than a call's worth, and are handed to the tracker with the time
//! they fall in, which counts only their timing (see [`crate::input`]). If the call fails, they
//! wait for the next one, up to [`MAX_INPUT_BACKLOG`]. Those drained after a sleep happened
//! around it, in time that isn't tracked, are dropped. So is any event up to a reading that
//! found the session locked, even one the reader hands over late: on X11 the lock screen's
//! input, a password being typed, still reaches the agent.
//!
//! **Activity** is measured from the input events, or from the idle source when there is no
//! working event reader (see [`crate::activity`]), by [`Timekeeper::tick_measured`] and by every
//! timer action. [`Timekeeper::tick`] takes it from the caller instead.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;
use worklume_platform::{
    ActiveWindow, BrowserUrl, Clock, ClockError, ClockReading, IdleSource, InputActivity,
    InputEvent, SessionLock,
};
use worklume_storage::{StorageError, Store, Timestamp};

use crate::activity::ActivityMeter;
use crate::tracker::{
    Activity, DecidedBy, IdlePeriod, IdleResolution, Observation, PauseReason, Recovery, Tracker,
    TrackerConfig, TrackerError, TrackerState, WorkTarget,
};
use crate::usage::{UsagePolicy, UsageSample};

/// A smaller growth in time asleep is noise from reading two counters microseconds apart.
const MIN_SLEEP_MS: i64 = 1_000;
/// Inactivity before a sleep shorter than this is not recorded as idle time: slots count whole
/// seconds.
const MIN_IDLE_MS: i64 = 1_000;
/// Input events kept for the next call while calls fail: a minute of reports at 2,000 a
/// second. Beyond it the oldest are dropped; by then the tracker counts the time as a gap
/// anyway.
pub const MAX_INPUT_BACKLOG: usize = 120_000;

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
    /// The device slept, or the session was locked, for longer than the idle timeout. The
    /// timer resumes on `target` only when the member says so.
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

/// The device's sensors the timekeeper reads, each behind its platform trait.
#[derive(Debug, Clone)]
pub struct Sensors {
    pub clock: Arc<dyn Clock>,
    pub idle: Arc<dyn IdleSource>,
    /// The foreground app, for app usage (E09-S12).
    pub window: Arc<dyn ActiveWindow>,
    /// The browser's active tab, for URL usage (E09-S12).
    pub browser: Arc<dyn BrowserUrl>,
    /// Input events, for input timing features (E09-S13) and activity (E12-S03).
    pub input: Arc<dyn InputActivity>,
    /// Whether the session is locked (E12-S03).
    pub lock: Arc<dyn SessionLock>,
}

/// Where the activity of an interval comes from.
#[derive(Debug, Clone, Copy)]
enum Measure {
    /// The caller measured it.
    Given(Activity),
    /// The sensors: input events, or the idle source without them.
    Sensors,
}

/// A locked session: the absence began at `since`, after `inactive_ms` without input. `since`
/// is unknown when the timer was already paused for a lock when the agent started.
#[derive(Debug, Clone)]
struct Absence {
    since: Option<Timestamp>,
    inactive_ms: i64,
}

/// The tracker on the device's clock. See the module documentation.
pub struct Timekeeper {
    tracker: Tracker,
    clock: Arc<dyn Clock>,
    idle: Arc<dyn IdleSource>,
    window: Arc<dyn ActiveWindow>,
    browser: Arc<dyn BrowserUrl>,
    input: Arc<dyn InputActivity>,
    lock: Arc<dyn SessionLock>,
    /// Input events drained but not yet handed to the tracker, oldest first.
    input_backlog: Vec<InputEvent>,
    /// Whether the last drain worked; otherwise activity comes from the idle source.
    input_works: bool,
    /// No input at or before this monotonic time is kept: it was the lock screen's.
    input_floor_mono: i64,
    /// Present while the session is known to be locked.
    locked: Option<Absence>,
    meter: ActivityMeter,
    policy: IdlePolicy,
    usage_policy: UsagePolicy,
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
    /// Restores the tracker (see [`Tracker::open`]) and applies the idle policy to its state.
    /// App and URL usage is off until [`Timekeeper::set_usage_policy`] turns it on.
    ///
    /// # Errors
    ///
    /// Fails when the clock, the store or the snapshot fails.
    pub fn open(
        store: Arc<Store>,
        config: TrackerConfig,
        sensors: Sensors,
        policy: IdlePolicy,
    ) -> Result<(Self, Recovery), TimekeeperError> {
        let reading = sensors.clock.read()?;
        let (tracker, recovery) = Tracker::open(store, config, &timestamp(&reading)?)?;
        let mut keeper = Self {
            tracker,
            clock: sensors.clock,
            idle: sensors.idle,
            window: sensors.window,
            browser: sensors.browser,
            input: sensors.input,
            lock: sensors.lock,
            input_backlog: Vec::new(),
            input_works: false,
            input_floor_mono: i64::MIN,
            locked: None,
            meter: ActivityMeter::default(),
            policy,
            usage_policy: UsagePolicy::OFF,
            last: reading,
            last_input_mono: None,
            armed_mono: None,
        };
        keeper.run(
            Measure::Given(Activity::NONE),
            &UsageSample::NONE,
            |_, _| Ok(()),
        )?;
        Ok((keeper, recovery))
    }

    /// The tracker's state.
    #[must_use]
    pub fn state(&self) -> &TrackerState {
        self.tracker.state()
    }

    /// Time tracked from `from_wall_ms`. See [`Tracker::tracked_since`].
    #[must_use]
    pub fn tracked_since(&self, from_wall_ms: i64) -> Duration {
        self.tracker.tracked_since(from_wall_ms)
    }

    /// The tracker's wall time at the last call. See [`Tracker::wall_now_ms`].
    #[must_use]
    pub fn wall_now_ms(&self) -> Option<i64> {
        self.tracker.wall_now_ms()
    }

    /// The clock reading of the last call.
    pub(crate) const fn last_reading(&self) -> &ClockReading {
        &self.last
    }

    /// Whether the session was locked at the last call.
    #[must_use]
    pub const fn locked(&self) -> bool {
        self.locked.is_some()
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
                target, idle: None, ..
            } if self.locked.is_none() => Some(Prompt::Resume { target: *target }),
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

    #[must_use]
    pub const fn usage_policy(&self) -> UsagePolicy {
        self.usage_policy
    }

    /// Replaces the app and URL policy, from the next tick.
    pub const fn set_usage_policy(&mut self, policy: UsagePolicy) {
        self.usage_policy = policy;
    }

    /// Accounts time since the previous call, with `activity` as the input measured meanwhile,
    /// and detects idle time. Call it about once a second. The app calls
    /// [`Timekeeper::tick_measured`] instead.
    ///
    /// # Errors
    ///
    /// Fails when the clock or the store fails; the next call catches up.
    pub fn tick(&mut self, activity: Activity) -> Result<Outcome, TimekeeperError> {
        self.tick_with(Measure::Given(activity))
    }

    /// As [`Timekeeper::tick`], measuring activity from the sensors.
    ///
    /// # Errors
    ///
    /// As [`Timekeeper::tick`].
    pub fn tick_measured(&mut self) -> Result<Outcome, TimekeeperError> {
        self.tick_with(Measure::Sensors)
    }

    fn tick_with(&mut self, measure: Measure) -> Result<Outcome, TimekeeperError> {
        // App and URL usage counts only while the timer runs and the member isn't idle, so the
        // sensors are left alone otherwise.
        let usage = if matches!(self.tracker.state(), TrackerState::Running { .. }) {
            self.usage_policy.observe(&*self.window, &*self.browser)
        } else {
            UsageSample::NONE
        };
        self.run(measure, &usage, Self::detect_idle)
    }

    /// Starts the timer on `target`. See [`Tracker::start`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::start`], or when the clock fails.
    pub fn start(&mut self, target: WorkTarget) -> Result<Outcome, TimekeeperError> {
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
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
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
            Ok(keeper.tracker.switch(target, now)?)
        })
    }

    /// Stops the timer. See [`Tracker::stop`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::stop`], or when the clock fails.
    pub fn stop(&mut self) -> Result<Outcome, TimekeeperError> {
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
            Ok(keeper.tracker.stop(now)?)
        })
    }

    /// Starts a break. See [`Tracker::start_break`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::start_break`], or when the clock fails.
    pub fn start_break(&mut self, break_policy_id: Uuid) -> Result<Outcome, TimekeeperError> {
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
            Ok(keeper.tracker.start_break(break_policy_id, now)?)
        })
    }

    /// Ends the break. See [`Tracker::end_break`].
    ///
    /// # Errors
    ///
    /// As [`Tracker::end_break`], or when the clock fails.
    pub fn end_break(&mut self) -> Result<Outcome, TimekeeperError> {
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
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
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
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
        self.run(Measure::Sensors, &UsageSample::NONE, |keeper, now| {
            keeper.tracker.resume(now)?;
            keeper.arm(now);
            Ok(())
        })
    }

    /// Reads the clock, catches up with any sleep and the time since the previous call, runs
    /// `action`, then applies the keep-idle policy.
    fn run(
        &mut self,
        measure: Measure,
        usage: &UsageSample,
        action: impl FnOnce(&mut Self, &Timestamp) -> Result<(), TimekeeperError>,
    ) -> Result<Outcome, TimekeeperError> {
        let before = self.prompt();
        self.drain_input();
        let reading = self.clock.read()?;
        let now = timestamp(&reading)?;
        let slept = self.catch_up(&reading, &now, measure, usage)?;
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
        measure: Measure,
        usage: &UsageSample,
    ) -> Result<Option<Duration>, TimekeeperError> {
        let same_boot =
            reading.boot_id == self.last.boot_id && reading.mono_ms >= self.last.mono_ms;
        if !same_boot {
            // Input, arming and lock times belong to the previous boot's clock.
            self.last_input_mono = None;
            self.armed_mono = None;
            self.input_floor_mono = i64::MIN;
            self.meter.forget();
            if self.locked.is_some() {
                self.locked = None;
            }
        }
        // A reading that fails keeps the state last known: a flaky reader must not unlock.
        let was_locked = self.locked.is_some();
        let locked_now = self.lock.locked().unwrap_or(was_locked);
        if was_locked || locked_now {
            // Input up to now may be the lock screen's.
            self.input_floor_mono = self.input_floor_mono.max(reading.mono_ms);
        }
        let floor = self.input_floor_mono;
        self.input_backlog.retain(|event| event.mono_ms > floor);
        let input_now = self
            .idle
            .idle_for()
            .ok()
            .map(|idle_for| reading.mono_ms.saturating_sub(millis(idle_for)));

        let asleep = if same_boot {
            sleep_between(&self.last, reading)?
        } else {
            None
        };
        let slept = if let Some((asleep_at, slept_ms)) = asleep {
            let inactive_ms = self.inactive_ms_at(&asleep_at);
            let activity = match measure {
                Measure::Given(activity) => activity,
                Measure::Sensors => Activity::NONE,
            };
            self.fall_asleep(&asleep_at, slept_ms, now, activity, locked_now)?;
            self.tracker.tick(now, Activity::NONE)?;
            match (was_locked, locked_now) {
                (false, true) => {
                    self.locked = Some(Absence {
                        since: Some(asleep_at),
                        inactive_ms,
                    });
                }
                (true, false) => self.unlocked(now)?,
                _ => {}
            }
            Some(Duration::from_millis(u64::try_from(slept_ms).unwrap_or(0)))
        } else {
            match (was_locked, locked_now) {
                (false, true) => {
                    // Seen at this reading; the lock began after the previous one.
                    let at = if same_boot {
                        timestamp(&self.last)?
                    } else {
                        now.clone()
                    };
                    self.lock_began(&at, now)?;
                }
                (true, true) => self.tracker.tick(now, Activity::NONE)?,
                (true, false) => {
                    self.tracker.tick(now, Activity::NONE)?;
                    self.unlocked(now)?;
                }
                (false, false) => {
                    let activity = match measure {
                        Measure::Given(activity) => activity,
                        Measure::Sensors if self.input_works => self.meter.from_events(
                            &self.input_backlog,
                            self.last.mono_ms,
                            reading.mono_ms,
                        ),
                        Measure::Sensors => self.meter.from_last_input(
                            input_now,
                            self.last.mono_ms,
                            reading.mono_ms,
                        ),
                    };
                    let observed = Observation {
                        usage,
                        input: &self.input_backlog,
                    };
                    self.tracker.tick_with(now, activity, &observed)?;
                }
            }
            None
        };
        if slept.is_some() || was_locked || locked_now {
            // Input around a sleep or a lock fell in time that isn't tracked.
            self.meter.forget();
        }
        self.input_backlog.clear();
        // Only once the sleep is dealt with: a failure above sees the same sleep next time.
        self.last = reading.clone();
        self.last_input_mono = input_now;
        Ok(slept)
    }

    /// The session was locked after the reading at `at`: tracking pauses there.
    fn lock_began(&mut self, at: &Timestamp, now: &Timestamp) -> Result<(), TimekeeperError> {
        let inactive_ms = self.inactive_ms_at(at);
        // Paused for a lock already: the agent started during it.
        let began_earlier = matches!(
            self.tracker.state(),
            TrackerState::Paused {
                reason: PauseReason::SessionLocked,
                ..
            }
        );
        self.tracker.pause(PauseReason::SessionLocked, at)?;
        self.tracker.tick(now, Activity::NONE)?;
        self.locked = Some(Absence {
            since: (!began_earlier).then(|| at.clone()),
            inactive_ms,
        });
        Ok(())
    }

    /// The session was found unlocked at `now`.
    fn unlocked(&mut self, now: &Timestamp) -> Result<(), TimekeeperError> {
        let absence = self.locked.clone();
        self.back_from_absence(absence.as_ref(), now)?;
        self.locked = None;
        Ok(())
    }

    /// Inactivity up to `at`, as far as the idle source knows.
    fn inactive_ms_at(&self, at: &Timestamp) -> i64 {
        self.inactive_since()
            .map_or(0, |since| (at.mono_ms() - since).max(0))
    }

    /// The session was unlocked at `now` after `absence`. A short absence resumes the timer; a
    /// long one, or one of unknown length, leaves the question to the member or to the `never`
    /// policy. Without idle detection every absence is short, as for a sleep.
    fn back_from_absence(
        &mut self,
        absence: Option<&Absence>,
        now: &Timestamp,
    ) -> Result<(), TimekeeperError> {
        if !matches!(
            self.tracker.state(),
            TrackerState::Paused { idle: None, .. }
        ) {
            return Ok(());
        }
        let away_long = self.policy.timeout.is_some_and(|timeout| {
            absence
                .and_then(|absence| {
                    let since = absence.since.as_ref()?;
                    Some(absence.inactive_ms + (now.mono_ms() - since.mono_ms()))
                })
                .is_none_or(|away_ms| away_ms >= millis(timeout))
        });
        if !away_long {
            self.tracker.resume(now)?;
            self.arm(now);
        }
        Ok(())
    }

    /// The device slept from `asleep_at` for `slept_ms` and woke at `now`. A short sleep resumes
    /// the timer, unless the session is `locked` now: then it resumes once unlocked.
    fn fall_asleep(
        &mut self,
        asleep_at: &Timestamp,
        slept_ms: i64,
        now: &Timestamp,
        activity: Activity,
        locked: bool,
    ) -> Result<(), TimekeeperError> {
        let inactive_since = self.inactive_since();
        let inactive_ms = self.inactive_ms_at(asleep_at);
        let away_long = self
            .policy
            .timeout
            .is_some_and(|timeout| inactive_ms + slept_ms >= millis(timeout));

        match self.tracker.state() {
            TrackerState::Running { entry } => {
                // Idle time can only begin inside the entry.
                let idle_since = inactive_since.map(|since| since.max(entry.started.mono_ms));
                self.tracker.tick(asleep_at, activity)?;
                if away_long
                    && let Some(since) = idle_since
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
            && !locked
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

    /// Takes the reader's events into the backlog. A reader that cannot see input (no permission,
    /// or Wayland) gives none: the slots then carry no input features, and activity comes from
    /// the idle source.
    fn drain_input(&mut self) {
        match self.input.drain() {
            Ok(events) => {
                self.input_works = true;
                self.input_backlog.extend(events);
            }
            Err(_) => self.input_works = false,
        }
        let excess = self.input_backlog.len().saturating_sub(MAX_INPUT_BACKLOG);
        self.input_backlog.drain(..excess);
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
            // A long absence: short ones resumed already. Once unlocked, so it isn't decided
            // before its length is known.
            TrackerState::Paused { idle: None, .. }
                if keep == KeepIdle::Never && self.locked.is_none() =>
            {
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
