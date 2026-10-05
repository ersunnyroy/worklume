//! A timekeeper on fake sensors, for the timekeeper and usage tests.

use std::sync::Arc;
use std::time::Duration;

use worklume_core::timekeeper::{IdlePolicy, KeepIdle, Outcome, Prompt, Sensors, Timekeeper};
use worklume_core::tracker::{Activity, IdleResolution, TrackerState};
use worklume_core::usage::UsagePolicy;
use worklume_platform::SensorError;
use worklume_platform::fake::{
    FakeBrowser, FakeClock, FakeIdle, FakeInput, FakeLock, FakeSpans, FakeWindow,
};

use super::{Harness, START_WALL_MS, seconds, target};

pub const TIMEOUT: Duration = Duration::from_mins(5);
pub const TWO_HOURS: Duration = Duration::from_hours(2);

pub const BUSY: Activity = Activity {
    active_ms: 800,
    keyboard_ms: Some(500),
    mouse_ms: Some(300),
};

pub fn policy(keep: KeepIdle) -> IdlePolicy {
    IdlePolicy {
        timeout: Some(TIMEOUT),
        keep,
    }
}

/// A timekeeper on fake sensors, recording every new prompt it raises.
pub struct Rig {
    pub harness: Harness,
    pub clock: Arc<FakeClock>,
    pub idle: Arc<FakeIdle>,
    pub window: Arc<FakeWindow>,
    pub browser: Arc<FakeBrowser>,
    pub input: Arc<FakeInput>,
    /// Off (`Unsupported`) unless a test turns it on.
    pub spans: Arc<FakeSpans>,
    pub lock: Arc<FakeLock>,
    pub keeper: Option<Timekeeper>,
    pub prompts: Vec<Prompt>,
}

impl Rig {
    pub fn new(policy: IdlePolicy) -> Self {
        let harness = Harness::new();
        let clock = Arc::new(FakeClock::new(START_WALL_MS));
        let idle = Arc::new(FakeIdle::new(Arc::clone(&clock)));
        let spans = Arc::new(FakeSpans::new(Arc::clone(&clock)));
        spans.set_error(Some(SensorError::Unsupported));
        let mut rig = Self {
            harness,
            clock,
            idle,
            window: Arc::new(FakeWindow::default()),
            browser: Arc::new(FakeBrowser::default()),
            input: Arc::new(FakeInput::default()),
            spans,
            lock: Arc::new(FakeLock::default()),
            keeper: None,
            prompts: Vec::new(),
        };
        rig.open(policy);
        rig
    }

    /// Opens the timekeeper, as the agent does at launch.
    pub fn open(&mut self, policy: IdlePolicy) {
        let sensors = Sensors {
            clock: Arc::clone(&self.clock) as _,
            idle: Arc::clone(&self.idle) as _,
            window: Arc::clone(&self.window) as _,
            browser: Arc::clone(&self.browser) as _,
            input: Arc::clone(&self.input) as _,
            spans: Arc::clone(&self.spans) as _,
            lock: Arc::clone(&self.lock) as _,
        };
        let (keeper, _) = Timekeeper::open(
            Arc::clone(self.harness.store()),
            Harness::config(),
            sensors,
            policy,
        )
        .unwrap();
        self.keeper = Some(keeper);
    }

    /// Closes the timekeeper and the store, and opens both again.
    pub fn restart(&mut self, policy: IdlePolicy) {
        self.keeper = None;
        self.harness.restart_store();
        self.open(policy);
    }

    pub fn set_usage_policy(&mut self, policy: UsagePolicy) {
        self.keeper().set_usage_policy(policy);
    }

    pub fn keeper(&mut self) -> &mut Timekeeper {
        self.keeper.as_mut().unwrap()
    }

    pub fn state(&self) -> &TrackerState {
        self.keeper.as_ref().unwrap().state()
    }

    pub fn prompt(&self) -> Option<Prompt> {
        self.keeper.as_ref().unwrap().prompt()
    }

    pub fn record(&mut self, outcome: Outcome) -> Outcome {
        if let Some(prompt) = &outcome.new_prompt {
            self.prompts.push(prompt.clone());
        }
        outcome
    }

    pub fn tick(&mut self, activity: Activity) -> Outcome {
        let outcome = self.keeper().tick(activity).unwrap();
        self.record(outcome)
    }

    /// The member works for `seconds`, with input every second.
    pub fn work(&mut self, seconds: u64) {
        for _ in 0..seconds {
            self.clock.advance(Duration::from_secs(1));
            self.idle.input();
            self.tick(BUSY);
        }
    }

    /// The member is away for `seconds`; the agent keeps ticking.
    pub fn away(&mut self, seconds: u64) {
        for _ in 0..seconds {
            self.clock.advance(Duration::from_secs(1));
            self.tick(Activity::NONE);
        }
    }

    /// The device sleeps for `duration`; opening the lid counts as input. Returns what the
    /// first tick after waking saw.
    pub fn sleep_and_wake(&mut self, duration: Duration) -> Outcome {
        self.clock.sleep(duration);
        self.clock.advance(Duration::from_secs(1));
        self.idle.input();
        self.tick(Activity::NONE)
    }

    pub fn start(&mut self) {
        let outcome = self.keeper().start(target(1)).unwrap();
        self.record(outcome);
    }

    pub fn resolve(&mut self, resolution: IdleResolution) {
        let outcome = self.keeper().resolve_idle(resolution).unwrap();
        self.record(outcome);
    }

    /// Stops the timer if it runs, then lets the open window pass so every slot closes, and
    /// returns the tracked seconds of all slots.
    pub fn tracked_seconds(&mut self) -> u64 {
        if matches!(
            self.state(),
            TrackerState::Running { .. } | TrackerState::Paused { idle: None, .. }
        ) {
            self.keeper().stop().unwrap();
        }
        self.away(600);
        self.harness
            .events_of("slot.closed")
            .iter()
            .map(|slot| seconds(slot, "tracked_seconds"))
            .sum()
    }

    pub fn slot_starts(&self) -> Vec<String> {
        self.harness
            .events_of("slot.closed")
            .iter()
            .map(|slot| slot.payload["slot_start"].as_str().unwrap().to_owned())
            .collect()
    }
}
