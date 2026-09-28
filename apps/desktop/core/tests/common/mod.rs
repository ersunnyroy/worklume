//! A controllable clock and a store in a temporary folder, for the tracker tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers in a test crate fail the test on bad setup"
)]

use std::sync::Arc;

use serde_json::Value;
use tempfile::TempDir;
use uuid::Uuid;
use worklume_core::tracker::{Activity, InputMethod, Tracker, TrackerConfig, WorkTarget};
use worklume_platform::fake::MemorySecretStore;
use worklume_storage::{OutboxEvent, Store, Timestamp};

/// 2026-09-21 14:03:00 UTC: three minutes into a 10-minute window.
pub const START_WALL_MS: i64 = 1_789_999_380_000;

/// A clock the test moves by hand. Wall and monotonic time advance together unless the test
/// changes the wall clock or reboots.
#[derive(Debug, Clone)]
pub struct Clock {
    pub wall_ms: i64,
    pub mono_ms: i64,
    pub boot_id: String,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            wall_ms: START_WALL_MS,
            mono_ms: 5_000_000,
            boot_id: "boot-1".to_owned(),
        }
    }

    pub fn now(&self) -> Timestamp {
        Timestamp::new(self.wall_ms, self.mono_ms, self.boot_id.clone()).unwrap()
    }

    pub fn advance_ms(&mut self, ms: i64) -> Timestamp {
        self.wall_ms += ms;
        self.mono_ms += ms;
        self.now()
    }

    pub fn advance_secs(&mut self, seconds: i64) -> Timestamp {
        self.advance_ms(seconds * 1_000)
    }

    /// Someone changes the system clock; monotonic time is unaffected.
    pub fn set_wall_by(&mut self, ms: i64) {
        self.wall_ms += ms;
    }

    /// The device restarts after `downtime_ms`: a new boot, monotonic time from near zero.
    pub fn reboot(&mut self, downtime_ms: i64) {
        self.wall_ms += downtime_ms;
        self.mono_ms = 20_000;
        self.boot_id = format!("{}-next", self.boot_id);
    }
}

pub struct Harness {
    pub dir: TempDir,
    pub secrets: MemorySecretStore,
    store: Option<Arc<Store>>,
    pub clock: Clock,
}

impl Harness {
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let secrets = MemorySecretStore::default();
        let (store, _) = Store::open(dir.path(), &secrets).unwrap();
        Self {
            dir,
            secrets,
            store: Some(Arc::new(store)),
            clock: Clock::new(),
        }
    }

    pub fn store(&self) -> &Arc<Store> {
        self.store.as_ref().expect("the store is open")
    }

    pub fn config() -> TrackerConfig {
        TrackerConfig::new(InputMethod::Hooks)
    }

    pub fn tracker(&self) -> Tracker {
        Tracker::open(Arc::clone(self.store()), Self::config(), &self.clock.now())
            .unwrap()
            .0
    }

    /// Closes and reopens the store, as a restarted agent would. Drop every tracker first: the
    /// store's lock allows one opener.
    pub fn restart_store(&mut self) {
        self.store = None;
        let (store, _) = Store::open(self.dir.path(), &self.secrets).unwrap();
        self.store = Some(Arc::new(store));
    }

    /// Lets `seconds` pass with a tick every second, as the agent's loop would, and returns the
    /// final time.
    pub fn pass(&mut self, tracker: &mut Tracker, seconds: i64) -> Timestamp {
        for _ in 0..seconds {
            let now = self.clock.advance_secs(1);
            tracker.tick(&now, Activity::NONE).unwrap();
        }
        self.clock.now()
    }

    /// Every event in the outbox, oldest first.
    pub fn events(&self) -> Vec<OutboxEvent> {
        self.store().outbox_pending(usize::MAX).unwrap()
    }

    /// Events of one type.
    pub fn events_of(&self, event_type: &str) -> Vec<OutboxEvent> {
        self.events()
            .into_iter()
            .filter(|event| event.event_type.as_str() == event_type)
            .collect()
    }

    /// Types of every event, oldest first.
    pub fn event_types(&self) -> Vec<String> {
        self.events()
            .iter()
            .map(|event| event.event_type.as_str().to_owned())
            .collect()
    }

    /// Acknowledges every pending event, so later assertions see only new ones.
    pub fn clear_events(&self) {
        let seqs: Vec<_> = self.events().iter().map(|event| event.seq).collect();
        self.store()
            .write(|tx| tx.remove_acknowledged(&seqs))
            .unwrap();
    }
}

pub fn target(n: u128) -> WorkTarget {
    WorkTarget {
        project_id: Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0000 | n),
        task_id: None,
    }
}

pub fn uuid(value: &Value) -> Uuid {
    value.as_str().unwrap().parse().unwrap()
}

pub fn seconds(event: &OutboxEvent, field: &str) -> u64 {
    event.payload[field].as_u64().unwrap()
}
