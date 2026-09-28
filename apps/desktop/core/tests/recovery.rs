//! Restarts: the tracker resumes from its snapshot. Time since the snapshot is not counted, the
//! entry is split there, and the timer carries on.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::sync::Arc;

use common::{Harness, seconds, target, uuid};
use serde_json::json;
use worklume_core::tracker::{
    PauseReason, Recovery, SNAPSHOT_KIND, Tracker, TrackerError, TrackerState,
};
use worklume_storage::{JournalKind, NewJournalEntry};

fn reopen(harness: &mut Harness) -> (Tracker, Recovery) {
    harness.restart_store();
    Tracker::open(
        Arc::clone(harness.store()),
        Harness::config(),
        &harness.clock.now(),
    )
    .unwrap()
}

#[test]
fn a_crash_splits_the_entry_at_the_last_snapshot() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    // Snapshots every 30 s; the crash comes 10 s after the one at 90 s.
    harness.pass(&mut tracker, 100);
    let TrackerState::Running { entry: before } = tracker.state().clone() else {
        panic!("running");
    };
    drop(tracker);
    harness.clock.advance_secs(5);

    let (mut tracker, recovery) = reopen(&mut harness);
    assert_eq!(
        recovery,
        Recovery {
            restored: true,
            resumed_tracking: true,
            unaccounted_ms: Some(15_000),
        }
    );
    let stop = harness.events_of("timer.stopped").remove(0);
    assert_eq!(stop.payload["reason"], "agent_restart");
    assert_eq!(uuid(&stop.payload["time_entry_id"]), before.time_entry_id);
    let restarted = harness.events_of("timer.started").remove(1);
    assert_eq!(restarted.payload["trigger"], "restart");
    assert_eq!(restarted.time.wall_ms() - stop.time.wall_ms(), 15_000);
    let TrackerState::Running { entry: after } = tracker.state().clone() else {
        panic!("still running");
    };
    assert_eq!(after.target, before.target);
    assert_eq!(*restarted.event_id.as_uuid(), after.time_entry_id);

    // The slot keeps the 90 s saved before the crash and adds what follows the restart.
    harness.pass(&mut tracker, 7 * 60 - 105 + 1);
    let slot = harness.events_of("slot.closed").remove(0);
    assert_eq!(seconds(&slot, "tracked_seconds"), 420 - 15);
    assert_eq!(uuid(&slot.payload["time_entry_id"]), after.time_entry_id);
}

#[test]
fn a_restart_while_paused_or_on_a_break_keeps_the_state() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let paused = harness.pass(&mut tracker, 30);
    tracker.pause(PauseReason::Sleep, &paused).unwrap();
    let state = tracker.state().clone();
    drop(tracker);
    harness.clock.advance_secs(600);
    harness.clear_events();

    let (tracker, recovery) = reopen(&mut harness);
    assert!(recovery.restored);
    assert!(!recovery.resumed_tracking);
    assert_eq!(tracker.state(), &state);
    assert_eq!(
        harness.event_types(),
        ["slot.closed"],
        "only the passed window closes"
    );
}

#[test]
fn a_reboot_closes_open_slots_at_once() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    harness.pass(&mut tracker, 90);
    drop(tracker);
    // The device restarts: a new boot, and back within the same window.
    harness.clock.reboot(60_000);

    let (tracker, recovery) = reopen(&mut harness);
    assert_eq!(
        recovery.unaccounted_ms, None,
        "time across a reboot cannot be measured"
    );
    assert!(recovery.resumed_tracking);
    let slot = harness.events_of("slot.closed").remove(0);
    assert_eq!(seconds(&slot, "tracked_seconds"), 90);
    assert_eq!(
        slot.time.boot_id(),
        "boot-1",
        "dated in the boot it belongs to"
    );
    let restarted = harness.events_of("timer.started").remove(1);
    assert_eq!(restarted.time.boot_id(), "boot-1-next");
    assert!(tracker.state().is_tracking());
}

#[test]
fn a_fresh_store_starts_stopped() {
    let harness = Harness::new();
    let (tracker, recovery) = Tracker::open(
        Arc::clone(harness.store()),
        Harness::config(),
        &harness.clock.now(),
    )
    .unwrap();
    assert_eq!(tracker.state(), &TrackerState::Stopped);
    assert!(!recovery.restored);
    assert!(harness.events().is_empty());
}

#[test]
fn a_snapshot_from_another_version_is_refused() {
    let harness = Harness::new();
    let mut payload = serde_json::Map::new();
    payload.insert("version".to_owned(), json!(99));
    harness
        .store()
        .write(|tx| {
            tx.append_journal(&NewJournalEntry {
                kind: JournalKind::new(SNAPSHOT_KIND)?,
                time: harness.clock.now(),
                payload,
            })
        })
        .unwrap();
    let result = Tracker::open(
        Arc::clone(harness.store()),
        Harness::config(),
        &harness.clock.now(),
    );
    assert!(matches!(result, Err(TrackerError::Snapshot(_))));
}
