//! Keeps the agent's events and the API contract in step. The tracker runs a scenario that
//! produces every event type; the encoded events must equal the committed fixture, which the
//! contract package validates against the `DesktopEvent` schema. IDs are replaced by
//! placeholders in order of appearance, so the fixture is stable.
//!
//! After changing an event, regenerate the fixture with
//! `UPDATE_FIXTURES=1 cargo test -p worklume-sync --test contract_fixture`, then run the
//! contract package's tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tempfile::TempDir;
use uuid::Uuid;
use worklume_core::tracker::{
    Activity, DecidedBy, IdleResolution, InputMethod, Tracker, TrackerConfig, WorkTarget,
};
use worklume_core::usage::{AppUse, SiteUse, UsageSample};
use worklume_platform::fake::MemorySecretStore;
use worklume_storage::{Store, Timestamp};
use worklume_sync::envelope;

const PROJECT: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00a1);
const TASK: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00a2);
const OTHER_PROJECT: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00b1);
const BREAK_POLICY: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_00c1);

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/api-contract/test/fixtures/desktop-events.json")
}

struct Clock {
    wall_ms: i64,
    mono_ms: i64,
}

impl Clock {
    fn now(&self) -> Timestamp {
        Timestamp::new(self.wall_ms, self.mono_ms, "4c1e0f7a-boot").unwrap()
    }

    fn tick(&mut self, tracker: &mut Tracker, seconds: u32, activity: Activity) -> Timestamp {
        self.tick_with(tracker, seconds, activity, &UsageSample::NONE)
    }

    fn tick_with(
        &mut self,
        tracker: &mut Tracker,
        seconds: u32,
        activity: Activity,
        usage: &UsageSample,
    ) -> Timestamp {
        for _ in 0..seconds {
            self.wall_ms += 1_000;
            self.mono_ms += 1_000;
            tracker.tick_with(&self.now(), activity, usage).unwrap();
        }
        self.now()
    }
}

/// Every event type, once or more.
fn scenario() -> Vec<Value> {
    let dir = TempDir::new().unwrap();
    let (store, _) = Store::open(dir.path(), &MemorySecretStore::default()).unwrap();
    let store = Arc::new(store);
    // 2026-09-21 14:03:00 UTC.
    let mut clock = Clock {
        wall_ms: 1_789_999_380_000,
        mono_ms: 7_200_000,
    };
    let config = TrackerConfig::new(InputMethod::Hooks);
    let (mut tracker, _) = Tracker::open(Arc::clone(&store), config, &clock.now()).unwrap();
    let busy = Activity {
        active_ms: 700,
        keyboard_ms: Some(500),
        mouse_ms: Some(200),
    };

    let target = WorkTarget {
        project_id: PROJECT,
        task_id: Some(TASK),
    };
    tracker.start(target, &clock.now()).unwrap();
    let editor = UsageSample {
        app: Some(AppUse {
            identifier: "com.microsoft.VSCode".to_owned(),
            name: "Visual Studio Code".to_owned(),
            title: Some("main.rs — worklume".to_owned()),
        }),
        site: None,
    };
    let browser = UsageSample {
        app: Some(AppUse {
            identifier: "com.google.Chrome".to_owned(),
            name: "Google Chrome".to_owned(),
            title: None,
        }),
        site: Some(SiteUse {
            domain: "github.com".to_owned(),
            path: Some("/acme/api/pull/7".to_owned()),
            browser: "chrome".to_owned(),
            private_window: false,
        }),
    };
    clock.tick_with(&mut tracker, 90, busy, &editor);
    let idle_since = clock.tick_with(&mut tracker, 30, busy, &browser);
    let detected = clock.tick(&mut tracker, 300, Activity::NONE);
    tracker.idle_detected(&idle_since, &detected).unwrap();
    let decided = clock.tick(&mut tracker, 30, Activity::NONE);
    tracker
        .resolve_idle(
            IdleResolution::Reassign {
                project_id: OTHER_PROJECT,
            },
            DecidedBy::Member,
            &decided,
        )
        .unwrap();
    let break_at = clock.tick(&mut tracker, 60, busy);
    tracker.start_break(BREAK_POLICY, &break_at).unwrap();
    let back = clock.tick(&mut tracker, 300, Activity::NONE);
    tracker.end_break(&back).unwrap();
    let stop_at = clock.tick(&mut tracker, 45, busy);
    tracker.stop(&stop_at).unwrap();
    clock.tick(&mut tracker, 600, Activity::NONE);

    store
        .outbox_pending(usize::MAX)
        .unwrap()
        .iter()
        .map(envelope::encode)
        .collect()
}

/// Replaces every UUID with a placeholder, the same UUID always with the same placeholder.
fn normalize(value: &mut Value, seen: &mut HashMap<String, String>) {
    match value {
        Value::String(text) if text.len() == 36 && Uuid::parse_str(text).is_ok() => {
            let next = seen.len() + 1;
            let placeholder = seen
                .entry(text.clone())
                .or_insert_with(|| format!("00000000-0000-7000-8000-{next:012x}"));
            text.clone_from(placeholder);
        }
        Value::Array(items) => {
            for item in items {
                normalize(item, seen);
            }
        }
        Value::Object(fields) => {
            for field in fields.values_mut() {
                normalize(field, seen);
            }
        }
        _ => {}
    }
}

#[test]
fn events_match_the_contract_fixture() {
    let mut events = Value::Array(scenario());
    normalize(&mut events, &mut HashMap::new());

    let types: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["type"].as_str().unwrap())
        .collect();
    for expected in [
        "timer.started",
        "timer.stopped",
        "idle.detected",
        "idle.resolved",
        "break.started",
        "break.ended",
        "slot.closed",
        "app_usage.batch",
        "url_usage.batch",
    ] {
        assert!(types.contains(&expected), "the scenario covers {expected}");
    }

    let encoded = serde_json::to_string_pretty(&events).unwrap() + "\n";
    let path = fixture_path();
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, encoded).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).expect("the fixture exists");
    assert!(
        committed == encoded,
        "the agent's events changed; regenerate {} with UPDATE_FIXTURES=1 and check the contract",
        path.display()
    );
}
