//! A simulated device running the real agent core.

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use uuid::Uuid;
use worklume_core::timekeeper::{
    IdlePolicy, KeepIdle, Outcome, Prompt, Timekeeper, TimekeeperError,
};
use worklume_core::tracker::{
    Activity, IdleResolution, InputMethod, TrackerConfig, TrackerError, TrackerState, WorkTarget,
};
use worklume_platform::ClockReading;
use worklume_platform::fake::{FakeClock, FakeIdle, MemorySecretStore};
use worklume_storage::{StorageError, Store};
use worklume_sync::envelope;

use crate::server::{FakeServer, Network};

/// 2026-09-21 09:00:00 UTC, a Monday.
pub const START_WALL_MS: i64 = 1_789_981_200_000;

/// Events per sync batch, as `POST /v1/desktop/sync` allows.
const BATCH: usize = 500;
/// A crashed agent is back after the watchdog's delay (E09-S11).
const WATCHDOG_DELAY: Duration = Duration::from_secs(1);
/// After a reboot, the agent starts with the member's session.
const UPTIME_AT_LAUNCH: Duration = Duration::from_secs(30);
const BREAK_POLICY: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0b0b);

/// How the simulated device behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceConfig {
    /// Time between the agent's ticks. The real agent ticks every second; long scenarios tick
    /// less often to run faster. It must stay under the tracker's one-minute gap.
    pub tick: Duration,
    /// The idle policy the agent starts with.
    pub policy: IdlePolicy,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            tick: Duration::from_secs(1),
            policy: IdlePolicy {
                timeout: Some(Duration::from_mins(5)),
                keep: KeepIdle::Prompt,
            },
        }
    }
}

/// A failure of the harness itself, or of the agent in a way no scenario asks for.
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("the agent failed: {0}")]
    Agent(#[from] TimekeeperError),
    #[error("the store failed: {0}")]
    Store(#[from] StorageError),
    #[error("the harness could not set up the device: {0}")]
    Setup(#[from] std::io::Error),
    #[error("the timer would not stop at the end of the script: {0}")]
    WontStop(String),
}

/// A stretch of one boot's monotonic time during which nothing may be tracked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub boot_id: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub why: &'static str,
}

/// The running agent: its store and its timekeeper.
struct Agent {
    store: Arc<Store>,
    keeper: Timekeeper,
}

/// A simulated device. See the crate documentation.
pub struct Device {
    clock: Arc<FakeClock>,
    idle: Arc<FakeIdle>,
    secrets: MemorySecretStore,
    dir: TempDir,
    config: DeviceConfig,
    policy: IdlePolicy,
    agent: Option<Agent>,
    down_since: Option<ClockReading>,
    untracked: Vec<Span>,
    prompts: Vec<Prompt>,
    server: FakeServer,
    network: Network,
    log: Vec<String>,
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Device")
            .field("config", &self.config)
            .field("running", &self.agent.is_some())
            .finish_non_exhaustive()
    }
}

impl Device {
    /// A new device with an empty store, its agent running and the member at the keyboard.
    ///
    /// # Errors
    ///
    /// Fails when the store or the agent cannot open.
    pub fn new(config: DeviceConfig) -> Result<Self, SimError> {
        let clock = Arc::new(FakeClock::new(START_WALL_MS));
        let idle = Arc::new(FakeIdle::new(Arc::clone(&clock)));
        let mut device = Self {
            clock,
            idle,
            secrets: MemorySecretStore::default(),
            dir: TempDir::new()?,
            config,
            policy: config.policy,
            agent: None,
            down_since: None,
            untracked: Vec::new(),
            prompts: Vec::new(),
            server: FakeServer::default(),
            network: Network::default(),
            log: Vec::new(),
        };
        device.launch()?;
        Ok(device)
    }

    // --- Observation -------------------------------------------------------------------------

    /// The tracker's state, while the agent runs.
    #[must_use]
    pub fn state(&self) -> Option<&TrackerState> {
        self.agent.as_ref().map(|agent| agent.keeper.state())
    }

    /// The question the member is being asked, while the agent runs.
    #[must_use]
    pub fn prompt(&self) -> Option<Prompt> {
        self.agent.as_ref().and_then(|agent| agent.keeper.prompt())
    }

    /// Every question the agent raised, in order.
    #[must_use]
    pub fn prompts(&self) -> &[Prompt] {
        &self.prompts
    }

    /// When the device slept or ran no agent, in its monotonic time.
    #[must_use]
    pub fn untracked(&self) -> &[Span] {
        &self.untracked
    }

    #[must_use]
    pub const fn server(&self) -> &FakeServer {
        &self.server
    }

    pub const fn network(&mut self) -> &mut Network {
        &mut self.network
    }

    #[must_use]
    pub fn clock(&self) -> ClockReading {
        self.clock.reading()
    }

    /// What happened, one line per step and notable outcome.
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Events waiting in the agent's outbox.
    ///
    /// # Errors
    ///
    /// Fails when the store fails.
    pub fn outbox_len(&self) -> Result<u64, SimError> {
        Ok(match &self.agent {
            Some(agent) => agent.store.outbox_len()?,
            None => 0,
        })
    }

    pub(crate) fn note(&mut self, line: impl Into<String>) {
        let reading = self.clock.reading();
        let minutes = (reading.wall_ms - START_WALL_MS) / 60_000;
        self.log.push(format!("[+{minutes} min] {}", line.into()));
    }

    // --- The device --------------------------------------------------------------------------

    /// The member works for `duration`: input on every tick.
    ///
    /// # Errors
    ///
    /// Fails when the agent fails.
    pub fn work(&mut self, duration: Duration) -> Result<(), SimError> {
        self.pass(duration, true)
    }

    /// The member is away for `duration`; the device stays awake and the agent keeps ticking.
    ///
    /// # Errors
    ///
    /// Fails when the agent fails.
    pub fn away(&mut self, duration: Duration) -> Result<(), SimError> {
        self.pass(duration, false)
    }

    fn pass(&mut self, duration: Duration, input: bool) -> Result<(), SimError> {
        let mut left = duration;
        while !left.is_zero() {
            let step = left.min(self.config.tick);
            left -= step;
            self.clock.advance(step);
            if input {
                self.idle.input();
            }
            let activity = if input {
                Activity {
                    active_ms: millis_u64(step) * 3 / 4,
                    keyboard_ms: Some(millis_u64(step) / 2),
                    mouse_ms: Some(millis_u64(step) / 4),
                }
            } else {
                Activity::NONE
            };
            if let Some(agent) = self.agent.as_mut() {
                let outcome = agent.keeper.tick(activity)?;
                self.record(outcome);
            }
        }
        Ok(())
    }

    /// The device sleeps for `duration`. The agent sees it on its next tick.
    pub fn sleep(&mut self, duration: Duration) {
        let now = self.clock.reading();
        self.untracked.push(Span {
            boot_id: now.boot_id,
            from_ms: now.mono_ms,
            to_ms: now.mono_ms + millis_i64(duration),
            why: "asleep",
        });
        self.clock.sleep(duration);
        self.note(format!("slept {}", minutes(duration)));
    }

    /// Someone moves the wall clock by `seconds`; the monotonic clocks do not notice.
    pub fn shift_clock(&mut self, seconds: i64) {
        self.clock.shift_wall(seconds * 1_000);
        self.note(format!("wall clock moved by {seconds} s"));
    }

    /// The device restarts: the agent dies, the device is off for `downtime`, and the agent
    /// starts again in the new boot.
    ///
    /// # Errors
    ///
    /// Fails when the agent cannot start again.
    pub fn reboot(&mut self, downtime: Duration) -> Result<(), SimError> {
        self.agent = None;
        self.down_since = None;
        self.clock.reboot(downtime, UPTIME_AT_LAUNCH);
        self.note(format!("rebooted after {}", minutes(downtime)));
        self.launch()
    }

    /// The agent crashes and the watchdog starts it again after `down`.
    ///
    /// # Errors
    ///
    /// Fails when the agent cannot start again.
    pub fn crash(&mut self, down: Duration) -> Result<(), SimError> {
        self.kill("crashed");
        self.clock.advance(down.max(WATCHDOG_DELAY));
        self.launch()
    }

    /// The agent crashes while writing: its next store write fails, then it dies, and the
    /// watchdog starts it again.
    ///
    /// # Errors
    ///
    /// Fails when the agent cannot start again.
    pub fn crash_mid_write(&mut self) -> Result<(), SimError> {
        if let Some(agent) = self.agent.as_mut() {
            agent.store.fail_next_writes(1);
            self.clock.advance(self.config.tick);
            self.idle.input();
            // The tick writes whenever the tracker is due a snapshot or an event; a failed
            // write is the point, so its error is expected.
            if let Ok(outcome) = agent.keeper.tick(Activity::NONE) {
                self.record(outcome);
            }
        }
        self.kill("crashed mid-write");
        self.clock.advance(WATCHDOG_DELAY);
        self.launch()
    }

    fn kill(&mut self, why: &str) {
        if self.agent.take().is_some() {
            self.down_since = Some(self.clock.reading());
            self.note(why);
        }
    }

    fn launch(&mut self) -> Result<(), SimError> {
        let (store, _) = Store::open(self.dir.path(), &self.secrets)?;
        let store = Arc::new(store);
        let (keeper, recovery) = Timekeeper::open(
            Arc::clone(&store),
            TrackerConfig::new(InputMethod::Hooks),
            Arc::clone(&self.clock) as _,
            Arc::clone(&self.idle) as _,
            self.policy,
        )?;
        let now = self.clock.reading();
        if let Some(down) = self.down_since.take()
            && down.boot_id == now.boot_id
        {
            self.untracked.push(Span {
                boot_id: now.boot_id.clone(),
                from_ms: down.mono_ms,
                to_ms: now.mono_ms,
                why: "no agent",
            });
        }
        self.agent = Some(Agent { store, keeper });
        if recovery.resumed_tracking {
            self.note("agent started and resumed tracking");
        }
        Ok(())
    }

    // --- The member --------------------------------------------------------------------------

    /// Starts the timer on project `n`. Returns whether the agent accepted it.
    ///
    /// # Errors
    ///
    /// Fails when the agent fails for another reason than refusing the action.
    pub fn start(&mut self, project: u8) -> Result<bool, SimError> {
        self.act(&format!("start project {project}"), |keeper| {
            keeper.start(target(project))
        })
    }

    /// Switches to project `n`.
    ///
    /// # Errors
    ///
    /// As [`Device::start`].
    pub fn switch(&mut self, project: u8) -> Result<bool, SimError> {
        self.act(&format!("switch to project {project}"), |keeper| {
            keeper.switch(target(project))
        })
    }

    /// Stops the timer.
    ///
    /// # Errors
    ///
    /// As [`Device::start`].
    pub fn stop(&mut self) -> Result<bool, SimError> {
        self.act("stop", Timekeeper::stop)
    }

    /// Starts a break.
    ///
    /// # Errors
    ///
    /// As [`Device::start`].
    pub fn start_break(&mut self) -> Result<bool, SimError> {
        self.act("start a break", |keeper| keeper.start_break(BREAK_POLICY))
    }

    /// Ends the break.
    ///
    /// # Errors
    ///
    /// As [`Device::start`].
    pub fn end_break(&mut self) -> Result<bool, SimError> {
        self.act("end the break", Timekeeper::end_break)
    }

    /// Answers the idle question.
    ///
    /// # Errors
    ///
    /// As [`Device::start`].
    pub fn answer(&mut self, resolution: IdleResolution) -> Result<bool, SimError> {
        self.act(&format!("answer {resolution:?}"), |keeper| {
            keeper.resolve_idle(resolution)
        })
    }

    /// Resumes the timer after a long sleep.
    ///
    /// # Errors
    ///
    /// As [`Device::start`].
    pub fn resume(&mut self) -> Result<bool, SimError> {
        self.act("resume", Timekeeper::resume)
    }

    /// Replaces the idle policy, as a config update would (E09-S06).
    pub fn set_policy(&mut self, policy: IdlePolicy) {
        self.policy = policy;
        if let Some(agent) = self.agent.as_mut() {
            agent.keeper.set_policy(policy);
        }
        self.note(format!("policy {policy:?}"));
    }

    /// The member's action: done, refused (`false`), or failed.
    fn act(
        &mut self,
        name: &str,
        action: impl FnOnce(&mut Timekeeper) -> Result<Outcome, TimekeeperError>,
    ) -> Result<bool, SimError> {
        let Some(agent) = self.agent.as_mut() else {
            return Ok(false);
        };
        // The member reaches for the keyboard.
        self.idle.input();
        match action(&mut agent.keeper) {
            Ok(outcome) => {
                self.record(outcome);
                self.note(name);
                Ok(true)
            }
            Err(TimekeeperError::Tracker(TrackerError::InvalidTransition { .. })) => {
                self.note(format!("{name}: refused"));
                Ok(false)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn record(&mut self, outcome: Outcome) {
        if let Some(slept) = outcome.slept {
            self.note(format!("agent saw a sleep of {}", minutes(slept)));
        }
        if let Some(prompt) = outcome.new_prompt {
            self.note(match &prompt {
                Prompt::Idle { paused, .. } => format!("asked about idle time (paused: {paused})"),
                Prompt::Resume { .. } => "asked whether to resume".to_owned(),
            });
            self.prompts.push(prompt);
        }
    }

    // --- The network -------------------------------------------------------------------------

    /// Sends the outbox while online, in ordered batches, deleting each batch once the server
    /// answered. A lost answer means the batch is sent again. This stands in for the sync
    /// engine (E09-S05).
    ///
    /// # Errors
    ///
    /// Fails when the store fails.
    pub fn sync(&mut self) -> Result<(), SimError> {
        let Some(agent) = self.agent.as_ref() else {
            return Ok(());
        };
        while self.network.is_online() {
            let pending = agent.store.outbox_pending(BATCH)?;
            if pending.is_empty() {
                break;
            }
            let batch: Vec<_> = pending.iter().map(envelope::encode).collect();
            self.server.receive(&batch);
            if self.network.take_lost_response() {
                continue;
            }
            let seqs: Vec<_> = pending.iter().map(|event| event.seq).collect();
            agent.store.write(|tx| tx.remove_acknowledged(&seqs))?;
        }
        Ok(())
    }

    /// Ends the day: the agent runs and catches up, pending questions are answered, the timer
    /// and any break stop, the open 10-minute window passes so every slot closes, and the
    /// outbox is sent.
    ///
    /// # Errors
    ///
    /// Fails when the agent or the store fails, or the timer won't stop.
    pub fn wind_down(&mut self) -> Result<(), SimError> {
        if self.agent.is_none() {
            self.launch()?;
        }
        self.note("winding down");
        // A tick at this same moment first: a sleep the agent hasn't seen yet may still raise a
        // question.
        if let Some(agent) = self.agent.as_mut() {
            let outcome = agent.keeper.tick(Activity::NONE)?;
            self.record(outcome);
        }
        for _ in 0..4 {
            let Some(state) = self.state() else { break };
            if state.pending_idle().is_some() {
                self.answer(IdleResolution::Keep)?;
            } else if matches!(state, TrackerState::OnBreak { .. }) {
                self.end_break()?;
            } else if matches!(state, TrackerState::Stopped) {
                break;
            } else {
                self.stop()?;
            }
        }
        if !matches!(self.state(), Some(TrackerState::Stopped)) {
            return Err(SimError::WontStop(format!("{:?}", self.state())));
        }
        self.away(Duration::from_mins(11))?;
        self.network.set_online(true);
        self.network.lose_responses(0);
        self.sync()
    }
}

/// The project the member works on for `n`.
#[must_use]
fn target(project: u8) -> WorkTarget {
    WorkTarget {
        project_id: Uuid::from_u128(
            0x0192_0000_0000_7000_8000_0000_0000_0000 | u128::from(project),
        ),
        task_id: None,
    }
}

fn millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn millis_i64(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn minutes(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 120 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds / 60)
    }
}
