//! A locked session (E12-S03): the timer pauses, locked time and the lock screen's input are
//! never kept, and unlocking resumes or asks as after a sleep. Also activity measured from the
//! sensors: input events, active spans (E13-S04) or the idle source.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::time::Duration;

use common::rig::{Rig, policy};
use common::seconds;
use serde_json::Value;
use worklume_core::timekeeper::{IdlePolicy, KeepIdle, Prompt};
use worklume_core::tracker::{IdleResolution, PauseReason, TrackerState};
use worklume_platform::{InputEvent, InputKind, SensorError};

/// One second passes with a key press in its middle; the agent ticks and measures. The reader
/// and the compositor both see it; the test decides which the agent has.
fn type_for(rig: &mut Rig, secs: u64) {
    for _ in 0..secs {
        let from = rig.clock.reading().mono_ms;
        rig.clock.advance(Duration::from_secs(1));
        press(rig, [from + 500]);
        rig.idle.input();
        tick(rig);
    }
}

fn press(rig: &Rig, at: impl IntoIterator<Item = i64>) {
    for mono_ms in at {
        rig.spans.input(mono_ms);
        rig.input.push([key(mono_ms)]);
    }
}

/// The agent sees active spans (Wayland) instead of input events.
fn with_spans(rig: &Rig) {
    rig.input.set_error(Some(SensorError::Unsupported));
    rig.spans.set_error(None);
}

/// One second passes with nobody at the device.
fn wait(rig: &mut Rig, secs: u64) {
    for _ in 0..secs {
        rig.clock.advance(Duration::from_secs(1));
        tick(rig);
    }
}

fn tick(rig: &mut Rig) {
    let outcome = rig.keeper().tick_measured().unwrap();
    rig.record(outcome);
}

const fn key(mono_ms: i64) -> InputEvent {
    InputEvent {
        mono_ms,
        kind: InputKind::Key,
    }
}

/// The session locks and stays locked for `secs`, while someone types at the lock screen.
fn lock_for(rig: &mut Rig, secs: u64) {
    rig.lock.set(true);
    type_for(rig, secs);
}

/// The member types their password and the session unlocks: the reader hands the keystrokes
/// over on the tick that sees the unlock.
fn unlock(rig: &mut Rig) {
    let from = rig.clock.reading().mono_ms;
    rig.clock.advance(Duration::from_secs(1));
    press(rig, (1..=8).map(|n| from + n * 100));
    rig.idle.input();
    rig.lock.set(false);
    tick(rig);
}

fn keys_sent(rig: &Rig) -> u64 {
    rig.harness
        .events_of("slot.closed")
        .iter()
        .filter_map(|slot| {
            slot.payload
                .get("input_features")?
                .pointer("/events/key")?
                .as_u64()
        })
        .sum()
}

fn slot_sum(rig: &Rig, field: &str) -> u64 {
    rig.harness
        .events_of("slot.closed")
        .iter()
        .map(|slot| seconds(slot, field))
        .sum()
}

fn reasons(rig: &Rig) -> Vec<String> {
    rig.harness
        .events_of("timer.stopped")
        .iter()
        .map(|event| event.payload["reason"].as_str().unwrap().to_owned())
        .collect()
}

fn is_paused_for_lock(state: &TrackerState) -> bool {
    matches!(
        state,
        TrackerState::Paused {
            reason: PauseReason::SessionLocked,
            idle: None,
            ..
        }
    )
}

#[test]
fn a_short_lock_pauses_and_resumes_without_counting_it() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 60);
    lock_for(&mut rig, 120);
    assert!(is_paused_for_lock(rig.state()));
    assert!(rig.keeper().locked());
    unlock(&mut rig);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    assert!(rig.prompts.is_empty());
    type_for(&mut rig, 60);

    assert_eq!(rig.tracked_seconds(), 120);
    assert_eq!(reasons(&rig), ["session_locked", "user"]);
    let triggers: Vec<_> = rig
        .harness
        .events_of("timer.started")
        .iter()
        .map(|event| event.payload["trigger"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(triggers, ["user", "resume"]);
    // Neither the lock screen's typing nor the password was counted.
    assert_eq!(keys_sent(&rig), 120);
}

#[test]
fn a_long_lock_asks_to_resume_once_unlocked() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 600);
    // Nobody could see a question behind the lock screen.
    assert_eq!(rig.prompt(), None);
    assert!(rig.prompts.is_empty());
    unlock(&mut rig);
    assert!(is_paused_for_lock(rig.state()));
    assert!(matches!(rig.prompts.as_slice(), [Prompt::Resume { .. }]));
    let outcome = rig.keeper().resume().unwrap();
    rig.record(outcome);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    type_for(&mut rig, 30);
    assert_eq!(rig.tracked_seconds(), 60);
}

#[test]
fn the_keep_idle_policy_decides_after_a_long_lock() {
    // `always` still asks: the question is about resuming, not idle time.
    let mut rig = Rig::new(policy(KeepIdle::Always));
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 600);
    unlock(&mut rig);
    assert!(matches!(rig.prompts.as_slice(), [Prompt::Resume { .. }]));

    // `never` stops the timer once unlocked, not before.
    let mut rig = Rig::new(policy(KeepIdle::Never));
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 600);
    assert!(is_paused_for_lock(rig.state()));
    unlock(&mut rig);
    assert_eq!(rig.state(), &TrackerState::Stopped);
    assert!(rig.prompts.is_empty());
    assert_eq!(rig.tracked_seconds(), 30);
}

#[test]
fn the_absence_counts_inactivity_before_the_lock() {
    // Four minutes away, then a two-minute lock: six minutes, over the five-minute timeout.
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 10);
    wait(&mut rig, 240);
    rig.lock.set(true);
    wait(&mut rig, 120);
    unlock(&mut rig);
    assert!(is_paused_for_lock(rig.state()));
    assert!(matches!(rig.prompts.as_slice(), [Prompt::Resume { .. }]));
}

#[test]
fn input_from_locked_time_is_never_kept_even_when_handed_over_late() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    let locked_from = rig.clock.reading().mono_ms;
    rig.lock.set(true);
    wait(&mut rig, 60);
    let locked_to = rig.clock.reading().mono_ms;
    rig.lock.set(false);
    wait(&mut rig, 1);
    // The reader hands over keystrokes from the lock screen only now, with a real one.
    let from = rig.clock.reading().mono_ms;
    rig.clock.advance(Duration::from_secs(1));
    rig.input.push([
        key(locked_from + 5_000),
        key(locked_to - 200),
        key(from + 500),
    ]);
    rig.idle.input();
    tick(&mut rig);
    type_for(&mut rig, 29);
    rig.tracked_seconds();
    assert_eq!(keys_sent(&rig), 60);
}

#[test]
fn idle_time_awaiting_a_decision_stays_the_question_through_a_lock() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 10);
    wait(&mut rig, 400);
    assert!(matches!(
        rig.prompt(),
        Some(Prompt::Idle { paused: false, .. })
    ));
    lock_for(&mut rig, 30);
    assert!(matches!(
        rig.prompt(),
        Some(Prompt::Idle { paused: true, .. })
    ));
    unlock(&mut rig);
    // The same question: nothing new to announce, and no resume without an answer.
    assert_eq!(rig.prompts.len(), 1);
    assert!(matches!(
        rig.state(),
        TrackerState::Paused { idle: Some(_), .. }
    ));
    rig.resolve(IdleResolution::Keep);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
}

#[test]
fn a_device_that_sleeps_while_locked_resumes_once_unlocked() {
    // Locked, then asleep, then awake and still locked: nothing resumes until the unlock.
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 5);
    rig.clock.sleep(Duration::from_mins(1));
    wait(&mut rig, 1);
    assert!(is_paused_for_lock(rig.state()));
    unlock(&mut rig);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));

    // The lid closes: the session locks and the device sleeps between two ticks.
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    rig.lock.set(true);
    rig.clock.sleep(Duration::from_mins(1));
    let outcome = rig.sleep_and_wake(Duration::ZERO);
    assert!(outcome.slept.is_some());
    assert!(matches!(
        rig.state(),
        TrackerState::Paused {
            reason: PauseReason::Sleep,
            ..
        }
    ));
    assert_eq!(rig.prompt(), None);
    unlock(&mut rig);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    type_for(&mut rig, 30);
    assert_eq!(rig.tracked_seconds(), 60);
}

#[test]
fn a_long_sleep_while_locked_asks_once_unlocked() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    rig.lock.set(true);
    rig.sleep_and_wake(Duration::from_hours(1));
    assert_eq!(rig.prompt(), None);
    unlock(&mut rig);
    assert!(matches!(rig.prompts.as_slice(), [Prompt::Resume { .. }]));
}

#[test]
fn a_failing_lock_reading_keeps_the_last_state() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 5);
    rig.lock.set(false);
    rig.lock
        .set_error(Some(SensorError::Failed("the bus went away".to_owned())));
    type_for(&mut rig, 5);
    assert!(is_paused_for_lock(rig.state()));
    rig.lock.set_error(None);
    type_for(&mut rig, 30);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    // The seconds read while the reading failed stayed locked time.
    assert_eq!(rig.tracked_seconds(), 59);
}

#[test]
fn without_lock_detection_nothing_pauses() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.lock.set_error(Some(SensorError::Unsupported));
    rig.start();
    type_for(&mut rig, 30);
    rig.lock.set(true);
    type_for(&mut rig, 30);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    assert_eq!(rig.tracked_seconds(), 60);
}

#[test]
fn a_restart_during_a_lock_asks_after_unlocking() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 5);
    rig.restart(policy(KeepIdle::Prompt));
    wait(&mut rig, 5);
    assert!(is_paused_for_lock(rig.state()));
    // The lock's length is unknown after the restart, so the member decides.
    unlock(&mut rig);
    assert!(matches!(rig.prompts.as_slice(), [Prompt::Resume { .. }]));
}

#[test]
fn without_idle_detection_an_unlock_always_resumes() {
    let mut rig = Rig::new(IdlePolicy {
        timeout: None,
        keep: KeepIdle::Prompt,
    });
    rig.start();
    type_for(&mut rig, 30);
    lock_for(&mut rig, 3_600);
    unlock(&mut rig);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    assert!(rig.prompts.is_empty());
}

#[test]
fn activity_is_measured_from_input_events() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_for(&mut rig, 120);
    wait(&mut rig, 60);
    assert_eq!(rig.tracked_seconds(), 180);
    // A key press in the middle of each second keeps the member active from the first one on.
    let active = slot_sum(&rig, "active_seconds");
    assert!((119..=120).contains(&active), "active {active}");
    assert_eq!(slot_sum(&rig, "keyboard_seconds"), active);
    assert_eq!(slot_sum(&rig, "mouse_seconds"), 0);
}

#[test]
fn without_an_event_reader_activity_comes_from_the_idle_source() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.input.set_error(Some(SensorError::Unsupported));
    rig.start();
    type_for(&mut rig, 120);
    wait(&mut rig, 60);
    assert_eq!(rig.tracked_seconds(), 180);
    let active = slot_sum(&rig, "active_seconds");
    assert!((119..=121).contains(&active), "active {active}");
    // The combined figure: keyboard and mouse unknown.
    for slot in rig.harness.events_of("slot.closed") {
        assert_eq!(slot.payload["keyboard_seconds"], Value::Null);
        assert_eq!(slot.payload["mouse_seconds"], Value::Null);
    }
}

#[test]
fn without_an_event_reader_activity_comes_from_active_spans() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    with_spans(&rig);
    rig.start();
    type_for(&mut rig, 120);
    wait(&mut rig, 60);
    assert_eq!(rig.tracked_seconds(), 180);
    // As with events: active from the first key press on.
    let active = slot_sum(&rig, "active_seconds");
    assert!((119..=120).contains(&active), "active {active}");
    for slot in rig.harness.events_of("slot.closed") {
        assert_eq!(slot.payload["keyboard_seconds"], Value::Null);
        assert_eq!(slot.payload["mouse_seconds"], Value::Null);
        // Spans carry no events, so no input features either.
        assert!(
            slot.payload
                .get("input_features")
                .is_none_or(Value::is_null)
        );
    }
}

#[test]
fn events_are_preferred_to_spans() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.spans.set_error(None);
    rig.start();
    type_for(&mut rig, 60);
    assert_eq!(rig.tracked_seconds(), 60);
    let active = slot_sum(&rig, "active_seconds");
    assert!((59..=60).contains(&active), "active {active}");
    assert_eq!(slot_sum(&rig, "keyboard_seconds"), active);
}

#[test]
fn the_lock_screen_never_counts_through_active_spans() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    with_spans(&rig);
    rig.start();
    type_for(&mut rig, 60);
    // Typing at the lock screen, the password and then the member's own typing make one burst.
    lock_for(&mut rig, 120);
    unlock(&mut rig);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    type_for(&mut rig, 30);
    wait(&mut rig, 30);
    assert_eq!(rig.tracked_seconds(), 120);
    // 59.5 s before the lock, then from a second past the unlock to a second after the last
    // key (29.5 s): the lock screen's input kept the burst going for that first second.
    assert_eq!(slot_sum(&rig, "active_seconds"), 89);
}

#[test]
fn the_password_typed_to_unlock_never_counts_through_active_spans() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    with_spans(&rig);
    rig.start();
    type_for(&mut rig, 60);
    lock_for(&mut rig, 120);
    unlock(&mut rig);
    wait(&mut rig, 30);
    assert_eq!(rig.tracked_seconds(), 90);
    // Only the 59.5 s before the lock: the password's last second runs past the unlock, and
    // doesn't count.
    assert_eq!(slot_sum(&rig, "active_seconds"), 59);
}

#[test]
fn spans_that_fail_leave_activity_to_the_idle_source() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    with_spans(&rig);
    rig.spans.set_error(Some(SensorError::Failed(
        "the compositor went away".to_owned(),
    )));
    rig.start();
    type_for(&mut rig, 60);
    assert_eq!(rig.tracked_seconds(), 60);
    let active = slot_sum(&rig, "active_seconds");
    assert!((59..=61).contains(&active), "active {active}");
}
