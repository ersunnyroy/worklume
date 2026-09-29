//! The checker itself: a clean timeline passes, and each kind of damage is reported. Without
//! these, a checker that never complains would make every scenario pass.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test fails on a harness error, like on a wrong verdict"
)]

use std::time::Duration;

use serde_json::{Value, json};
use worklume_simulation::{Answer, Device, DeviceConfig, Span, Step, check};

/// A short day with every event type the tracker sends while working: entries, idle time, a
/// break and several slots.
fn clean_day() -> (Vec<Value>, Vec<Span>) {
    let mut device = Device::new(DeviceConfig::default()).unwrap();
    for step in [
        Step::Start(1),
        Step::Work(Duration::from_mins(12)),
        Step::Away(Duration::from_mins(6)),
        Step::Answer(Answer::Keep),
        Step::Work(Duration::from_mins(3)),
        Step::StartBreak,
        Step::Away(Duration::from_mins(5)),
        Step::EndBreak,
        Step::Work(Duration::from_mins(4)),
        Step::Stop,
    ] {
        device.perform(step).unwrap();
    }
    device.wind_down().unwrap();
    (
        device.server().events().to_vec(),
        device.untracked().to_vec(),
    )
}

/// Numbers the events 1, 2, 3… again, so a test's damage is the only thing wrong.
fn renumber(mut events: Vec<Value>) -> Vec<Value> {
    for (index, event) in events.iter_mut().enumerate() {
        event["seq"] = json!(index + 1);
    }
    events
}

fn position(events: &[Value], kind: &str) -> usize {
    events
        .iter()
        .position(|event| event["type"] == kind)
        .unwrap_or_else(|| panic!("the day has a {kind}"))
}

fn assert_reported(events: &[Value], untracked: &[Span], expected: &str) {
    let result = check(events, untracked);
    assert!(
        result
            .violations
            .iter()
            .any(|violation| violation.contains(expected)),
        "expected a violation containing {expected:?}, got {:?}",
        result.violations
    );
}

#[test]
fn a_clean_day_passes() {
    let (events, untracked) = clean_day();
    let result = check(&events, &untracked);
    assert!(result.violations.is_empty(), "{result}");
    assert_eq!(result.summary.idle_periods, 1);
    assert_eq!(result.summary.breaks, 1);
    assert!(result.summary.slots >= 3);
}

#[test]
fn a_missing_stop_is_reported() {
    let (mut events, untracked) = clean_day();
    let last_stop = events
        .iter()
        .rposition(|event| event["type"] == "timer.stopped")
        .unwrap();
    events.remove(last_stop);
    assert_reported(&renumber(events), &untracked, "still open at the end");
}

#[test]
fn a_second_open_entry_is_reported() {
    let (mut events, untracked) = clean_day();
    let start = position(&events, "timer.started");
    let mut again = events[start].clone();
    again["event_id"] = json!("01920000-0000-7000-8000-00000000ffff");
    events.insert(start + 1, again);
    assert_reported(&renumber(events), &untracked, "is still open");
}

#[test]
fn a_repeated_window_is_reported() {
    let (mut events, untracked) = clean_day();
    let slot = position(&events, "slot.closed");
    let mut again = events[slot].clone();
    again["event_id"] = json!("01920000-0000-7000-8000-00000000fffe");
    events.insert(slot + 1, again);
    assert_reported(&renumber(events), &untracked, "a second slot");
}

#[test]
fn a_clock_change_in_event_times_is_reported() {
    let (mut events, untracked) = clean_day();
    let slot = position(&events, "slot.closed");
    let wall = events[slot]["wall_time"].as_str().unwrap().to_owned();
    // One second later on the wall clock, the same monotonic reading.
    let shifted = worklume_core::time::rfc3339_utc(
        worklume_simulation::parse_rfc3339_ms(&wall).unwrap() + 1_000,
    );
    events[slot]["wall_time"] = json!(shifted);
    assert_reported(&events, &untracked, "off this boot's monotonic time");
}

#[test]
fn time_tracked_while_asleep_is_reported() {
    let (events, mut untracked) = clean_day();
    let start = position(&events, "timer.started");
    let mono = &events[start]["mono_time"];
    untracked.push(Span {
        boot_id: mono["boot_id"].as_str().unwrap().to_owned(),
        from_ms: mono["ms"].as_i64().unwrap() + 60_000,
        to_ms: mono["ms"].as_i64().unwrap() + 120_000,
        why: "asleep",
    });
    assert_reported(&events, &untracked, "tracks time while asleep");
}

#[test]
fn an_unresolved_idle_period_is_reported() {
    let (mut events, untracked) = clean_day();
    events.remove(position(&events, "idle.resolved"));
    assert_reported(&renumber(events), &untracked, "still pending at the end");
}

#[test]
fn slots_that_disagree_with_the_entries_are_reported() {
    let (mut events, untracked) = clean_day();
    let slot = position(&events, "slot.closed");
    let tracked = events[slot]["payload"]["tracked_seconds"].as_i64().unwrap();
    events[slot]["payload"]["tracked_seconds"] = json!(tracked - 2);
    assert_reported(&events, &untracked, "slots hold");
}

#[test]
fn an_out_of_range_slot_is_reported() {
    let (mut events, untracked) = clean_day();
    let slot = position(&events, "slot.closed");
    events[slot]["payload"]["tracked_seconds"] = json!(601);
    assert_reported(&events, &untracked, "601 tracked seconds");
}

#[test]
fn a_gap_in_the_sequence_is_reported() {
    let (mut events, untracked) = clean_day();
    events[3]["seq"] = json!(40);
    assert_reported(&events, &untracked, "follows seq");
}

#[test]
fn a_break_while_tracking_is_reported() {
    let (mut events, untracked) = clean_day();
    let stop_before_break = position(&events, "break.started") - 1;
    assert_eq!(events[stop_before_break]["payload"]["reason"], "break");
    events.remove(stop_before_break);
    assert_reported(&renumber(events), &untracked, "the timer still runs");
}
