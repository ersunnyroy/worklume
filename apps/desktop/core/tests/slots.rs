//! Slot aggregation: UTC windows, closing, attribution, activity, idle, flags, clock changes
//! and gaps.

#![allow(
    clippy::unwrap_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use common::{Harness, START_WALL_MS, seconds, target, uuid};
use serde_json::json;
use worklume_core::tracker::{Activity, DecidedBy, IdleResolution, Tracker, TrackerState};
use worklume_storage::OutboxEvent;

fn tick_with(harness: &mut Harness, tracker: &mut Tracker, seconds: i64, activity: Activity) {
    for _ in 0..seconds {
        let now = harness.clock.advance_secs(1);
        tracker.tick(&now, activity).unwrap();
    }
}

fn slots(harness: &Harness) -> Vec<OutboxEvent> {
    harness.events_of("slot.closed")
}

#[test]
fn slots_close_once_their_window_has_passed() {
    // Start at 14:03:00, stop at 14:28:00, then let 14:30 pass.
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    harness.pass(&mut tracker, 6 * 60 + 59);
    assert!(slots(&harness).is_empty(), "the 14:00 window has not ended");
    harness.pass(&mut tracker, 1);
    assert_eq!(slots(&harness).len(), 1);

    let stopped = harness.pass(&mut tracker, 18 * 60);
    tracker.stop(&stopped).unwrap();
    harness.pass(&mut tracker, 60);
    assert_eq!(slots(&harness).len(), 2, "14:20 stays open until 14:30");
    harness.pass(&mut tracker, 60);

    let slots = slots(&harness);
    let starts: Vec<_> = slots
        .iter()
        .map(|slot| slot.payload["slot_start"].clone())
        .collect();
    assert_eq!(
        starts,
        [
            json!("2026-09-21T14:00:00Z"),
            json!("2026-09-21T14:10:00Z"),
            json!("2026-09-21T14:20:00Z"),
        ]
    );
    let tracked_seconds: Vec<_> = slots
        .iter()
        .map(|slot| seconds(slot, "tracked_seconds"))
        .collect();
    assert_eq!(tracked_seconds, [420, 600, 480]);
    assert_eq!(slots[0].payload["flags"], json!(["partial_slot"]));
    assert_eq!(slots[1].payload["flags"], json!([]));
    assert_eq!(slots[2].payload["flags"], json!(["partial_slot"]));
    // Each is dated at the end of its window.
    assert_eq!(slots[0].time.wall_ms(), START_WALL_MS + 7 * 60_000);
    assert_eq!(slots[0].payload["input_method"], "hooks");
    assert_eq!(slots[0].payload["project_id"], json!(target(1).project_id));
}

#[test]
fn a_slot_belongs_to_the_entry_with_the_most_time_in_it() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    // 14:03 to 14:14 on project 1, then 14:14 to 14:20 on project 2.
    let switched = harness.pass(&mut tracker, 11 * 60);
    tracker.switch(target(2), &switched).unwrap();
    harness.pass(&mut tracker, 6 * 60 + 1);

    let slot = slots(&harness).remove(1);
    assert_eq!(slot.payload["slot_start"], "2026-09-21T14:10:00Z");
    assert_eq!(slot.payload["project_id"], json!(target(2).project_id));
    let TrackerState::Running { entry } = tracker.state() else {
        unreachable!("running");
    };
    assert_eq!(uuid(&slot.payload["time_entry_id"]), entry.time_entry_id);
    assert_eq!(seconds(&slot, "tracked_seconds"), 600);
}

#[test]
fn activity_and_idle_are_counted_per_slot() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    // 14:03 to 14:08 with input 60% of each second (40% keyboard, 20% mouse).
    let busy = Activity {
        active_ms: 600,
        keyboard_ms: Some(400),
        mouse_ms: Some(200),
    };
    tick_with(&mut harness, &mut tracker, 300, busy);
    // 14:08 to 14:10 idle, awaiting a decision.
    let since = harness.clock.now();
    tick_with(&mut harness, &mut tracker, 60, Activity::NONE);
    tracker.idle_detected(&since, &harness.clock.now()).unwrap();
    tick_with(&mut harness, &mut tracker, 60, Activity::NONE);
    tracker
        .resolve_idle(
            IdleResolution::Keep,
            DecidedBy::Member,
            &harness.clock.now(),
        )
        .unwrap();
    tick_with(&mut harness, &mut tracker, 1, Activity::NONE);

    let slot = slots(&harness).remove(0);
    assert_eq!(seconds(&slot, "tracked_seconds"), 420);
    assert_eq!(seconds(&slot, "active_seconds"), 180);
    assert_eq!(seconds(&slot, "keyboard_seconds"), 120);
    assert_eq!(seconds(&slot, "mouse_seconds"), 60);
    // Idle counts from detection; the server dates the idle period from `since`.
    assert_eq!(seconds(&slot, "idle_seconds"), 60);
}

#[test]
fn keyboard_and_mouse_are_null_without_a_split_and_activity_never_exceeds_time() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let wayland = Activity {
        active_ms: 5_000,
        keyboard_ms: None,
        mouse_ms: None,
    };
    tick_with(&mut harness, &mut tracker, 7 * 60 + 1, wayland);

    let slot = slots(&harness).remove(0);
    assert_eq!(slot.payload["keyboard_seconds"], json!(null));
    assert_eq!(slot.payload["mouse_seconds"], json!(null));
    assert_eq!(
        seconds(&slot, "active_seconds"),
        420,
        "clamped to tracked time"
    );
}

#[test]
fn changing_the_system_clock_neither_adds_nor_removes_time() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    harness.pass(&mut tracker, 120);
    // Someone moves the clock forward an hour, then back by 90 minutes.
    harness.clock.set_wall_by(3_600_000);
    harness.pass(&mut tracker, 300);
    harness.clock.set_wall_by(-5_400_000);
    harness.pass(&mut tracker, 180);
    let stopped = harness.clock.now();
    tracker.stop(&stopped).unwrap();
    harness.pass(&mut tracker, 700);

    let slots = slots(&harness);
    let starts: Vec<_> = slots
        .iter()
        .map(|slot| slot.payload["slot_start"].clone())
        .collect();
    assert_eq!(
        starts,
        [json!("2026-09-21T14:00:00Z"), json!("2026-09-21T14:10:00Z")],
        "windows follow monotonic time"
    );
    let total: u64 = slots
        .iter()
        .map(|slot| seconds(slot, "tracked_seconds"))
        .sum();
    assert_eq!(total, 600);
    assert_eq!(
        slots[0].payload["flags"],
        json!(["partial_slot", "clock_adjusted"])
    );
    assert_eq!(
        slots[1].payload["flags"],
        json!(["partial_slot", "clock_adjusted"])
    );
    // Events are dated in derived time too.
    let stop = harness.events_of("timer.stopped").remove(0);
    assert_eq!(stop.time.wall_ms(), START_WALL_MS + 600_000);
}

#[test]
fn a_silence_longer_than_a_minute_is_not_counted() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let last_tick = harness.pass(&mut tracker, 60);
    // Five minutes without a tick: the agent was frozen or the device slept unseen.
    let resumed = harness.clock.advance_secs(300);
    tracker.tick(&resumed, Activity::NONE).unwrap();
    harness.pass(&mut tracker, 60);

    let stops = harness.events_of("timer.stopped");
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0].payload["reason"], "gap");
    assert_eq!(stops[0].time.wall_ms(), last_tick.wall_ms());
    let starts = harness.events_of("timer.started");
    assert_eq!(starts[1].payload["trigger"], "gap_end");
    assert_eq!(starts[1].time.wall_ms(), resumed.wall_ms());

    let slot = slots(&harness).remove(0);
    assert_eq!(
        seconds(&slot, "tracked_seconds"),
        120,
        "the five minutes are left out"
    );
}

#[test]
fn a_slot_with_no_whole_second_is_not_sent() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let now = harness.clock.advance_ms(400);
    tracker.tick(&now, Activity::NONE).unwrap();
    tracker.stop(&now).unwrap();
    harness.pass(&mut tracker, 600);
    assert!(slots(&harness).is_empty());
}
