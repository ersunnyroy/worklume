//! Input timing features (E09-S13): what a slot counts, when, and that no event time is ever
//! kept or sent (the acceptance criterion).

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::collections::HashSet;
use std::time::Duration;

use common::rig::{BUSY, Rig, policy};
use serde_json::Value;
use worklume_core::time::rfc3339_utc;
use worklume_core::timekeeper::KeepIdle;
use worklume_core::tracker::{Activity, IdleResolution, SNAPSHOT_KIND};
use worklume_platform::{InputEvent, InputKind, SensorError};

/// A person's uneven typing, the same every run.
struct Person(u64);

impl Person {
    fn next(&mut self, bound: u64) -> i64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        i64::try_from((self.0 >> 33) % bound).unwrap()
    }

    /// One second: one to five key presses and sometimes a click, never on a whole second.
    fn second(&mut self) -> Vec<(i64, InputKind)> {
        let keys = 1 + self.next(5);
        let mut events: Vec<_> = (0..keys)
            .map(|_| (1 + self.next(998), InputKind::Key))
            .collect();
        if self.next(4) == 0 {
            events.push((1 + self.next(998), InputKind::Click));
        }
        events.sort_by_key(|&(offset, _)| offset);
        events
    }
}

/// A key press in the middle of every second.
fn steady(_: u64) -> Vec<(i64, InputKind)> {
    vec![(500, InputKind::Key)]
}

/// Ticks every second for `seconds`, with `input` giving each second's events as offsets into
/// it. Returns the events.
fn type_along(
    rig: &mut Rig,
    seconds: u64,
    mut input: impl FnMut(u64) -> Vec<(i64, InputKind)>,
) -> Vec<InputEvent> {
    let mut all = Vec::new();
    for second in 0..seconds {
        let from = rig.clock.reading().mono_ms;
        rig.clock.advance(Duration::from_secs(1));
        let events: Vec<_> = input(second)
            .into_iter()
            .map(|(offset, kind)| InputEvent {
                mono_ms: from + offset,
                kind,
            })
            .collect();
        let activity = if events.is_empty() {
            Activity::NONE
        } else {
            rig.idle.input();
            BUSY
        };
        rig.input.push(events.iter().copied());
        all.extend(events);
        rig.tick(activity);
    }
    all
}

fn features(rig: &Rig) -> Vec<Value> {
    rig.harness
        .events_of("slot.closed")
        .iter()
        .map(|slot| slot.payload["input_features"].clone())
        .collect()
}

/// The sum of a number over every slot's features.
fn total(rig: &Rig, pointer: &str) -> u64 {
    features(rig)
        .iter()
        .filter_map(|features| features.pointer(pointer).and_then(Value::as_u64))
        .sum()
}

fn count_of(events: &[InputEvent], kind: InputKind) -> u64 {
    u64::try_from(events.iter().filter(|event| event.kind == kind).count()).unwrap()
}

/// The longest list anywhere in `value`.
fn longest(value: &Value) -> usize {
    match value {
        Value::Array(items) => items
            .iter()
            .map(longest)
            .max()
            .unwrap_or(0)
            .max(items.len()),
        Value::Object(fields) => fields.values().map(longest).max().unwrap_or(0),
        _ => 0,
    }
}

/// Every number and string in `value`.
fn leaves(value: &Value, numbers: &mut Vec<i64>, strings: &mut Vec<String>) {
    match value {
        Value::Number(number) => numbers.extend(number.as_i64()),
        Value::String(text) => strings.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| leaves(item, numbers, strings)),
        Value::Object(fields) => fields
            .values()
            .for_each(|field| leaves(field, numbers, strings)),
        _ => {}
    }
}

#[test]
fn no_event_time_is_kept_or_sent() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let mut person = Person(7);
    // Each event's monotonic time, and its wall time: wall and monotonic time move together
    // on this device.
    let mut mono_times = HashSet::new();
    let mut wall_times = HashSet::new();
    let mut remember = |rig: &Rig, events: Vec<InputEvent>| {
        let reading = rig.clock.reading();
        for event in events {
            mono_times.insert(event.mono_ms);
            wall_times.insert(event.mono_ms + reading.wall_ms - reading.mono_ms);
        }
    };

    rig.start();
    let events = type_along(&mut rig, 700, |_| person.second());
    remember(&rig, events);
    rig.restart(policy(KeepIdle::Prompt));
    let events = type_along(&mut rig, 200, |_| person.second());
    remember(&rig, events);
    rig.sleep_and_wake(Duration::from_mins(2));
    let events = type_along(&mut rig, 200, |_| person.second());
    remember(&rig, events);
    // A snapshot mid-slot, before the slots close.
    let journal = rig.harness.store().journal_after(None, usize::MAX).unwrap();
    assert!(!journal.is_empty());
    rig.tracked_seconds();

    let mut numbers = Vec::new();
    let mut strings = Vec::new();
    for entry in &journal {
        leaves(
            &Value::Object(entry.payload.clone()),
            &mut numbers,
            &mut strings,
        );
        numbers.extend([entry.time.wall_ms(), entry.time.mono_ms()]);
    }
    for event in rig.harness.events() {
        leaves(
            &Value::Object(event.payload.clone()),
            &mut numbers,
            &mut strings,
        );
        numbers.extend([event.time.wall_ms(), event.time.mono_ms()]);
    }
    let wall_texts: HashSet<String> = wall_times.iter().map(|&ms| rfc3339_utc(ms)).collect();
    for number in &numbers {
        assert!(
            !mono_times.contains(number) && !wall_times.contains(number),
            "an input's time, {number}, was kept"
        );
    }
    for text in &strings {
        assert!(
            !wall_texts.contains(text),
            "an input's time, {text}, was kept"
        );
    }
    // Not vacuous: the inputs were counted.
    assert!(features(&rig).iter().filter(|f| !f.is_null()).count() >= 2);
    assert!(total(&rig, "/events/key") > 1_000);
}

#[test]
fn the_saved_state_does_not_grow_with_input() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let busy = |_| {
        let mut events: Vec<_> = (0..20)
            .map(|n| (1 + n * 47 + n * n % 13, InputKind::Key))
            .collect();
        events.extend((0..8).map(|n| (700 + n * 8, InputKind::Pointer)));
        events
    };
    let snapshot = |rig: &Rig| {
        let journal = rig.harness.store().journal_after(None, usize::MAX).unwrap();
        let entry = journal
            .iter()
            .rev()
            .find(|entry| entry.kind.as_str() == SNAPSHOT_KIND)
            .unwrap();
        Value::Object(entry.payload.clone())
    };
    rig.start();
    type_along(&mut rig, 60, busy);
    let before = snapshot(&rig).to_string().len();
    let inputs = type_along(&mut rig, 240, busy).len();
    let after = snapshot(&rig);

    assert!(inputs > 6_000);
    assert!(
        after.to_string().len() < before + 1_000,
        "{inputs} more events grew the snapshot from {before} to {} bytes",
        after.to_string().len()
    );
    // No list in it holds more than a histogram's 16 buckets.
    assert!(longest(&after) <= 16);
}

#[test]
fn a_jiggler_looks_nothing_like_a_person() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let mut person = Person(11);
    rig.start();
    // 14:03 to 14:10, a person; 14:10 to 14:20, a jiggler every 30 s.
    type_along(&mut rig, 420, |_| person.second());
    type_along(&mut rig, 600, |second| {
        if second % 30 == 29 {
            vec![(500, InputKind::Pointer)]
        } else {
            Vec::new()
        }
    });
    rig.tracked_seconds();

    let slots = features(&rig);
    let (human, jiggler) = (&slots[0], &slots[1]);
    let keys = &human["intervals"]["key"];
    assert!(keys["entropy_bits"].as_f64().unwrap() >= 1.5, "{human}");
    assert!(keys["cv"].as_f64().unwrap() >= 0.3, "{human}");
    assert!(human["keyboard_share"].as_f64().unwrap() > 0.5);

    assert_eq!(
        jiggler["events"],
        serde_json::json!({"key": 0, "pointer": 20, "click": 0, "scroll": 0})
    );
    assert_eq!(jiggler["keyboard_share"], 0.0);
    assert_eq!(
        jiggler["intervals"]["pointer"],
        serde_json::json!({"count": 19, "entropy_bits": 0.0, "cv": 0.0})
    );
    assert_eq!(jiggler["intervals"]["key"], Value::Null);
}

#[test]
fn input_counts_only_while_the_timer_runs_and_the_member_is_not_idle() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let mut person = Person(3);
    // Before the timer starts: drained, never counted.
    type_along(&mut rig, 30, |_| person.second());
    assert_eq!(rig.input.pending(), 0);
    rig.start();
    let first = type_along(&mut rig, 60, |_| person.second());
    // Away until idle is detected, then back at the keyboard before answering.
    rig.away(300);
    assert!(rig.state().pending_idle().is_some());
    type_along(&mut rig, 30, |_| person.second());
    rig.resolve(IdleResolution::Keep);
    let second = type_along(&mut rig, 30, |_| person.second());
    rig.tracked_seconds();

    let keys = count_of(&first, InputKind::Key) + count_of(&second, InputKind::Key);
    assert_eq!(total(&rig, "/events/key"), keys);
    // Idle time breaks the rhythm: no interval spans it.
    assert_eq!(total(&rig, "/intervals/key/count"), keys - 2);
}

#[test]
fn no_interval_spans_a_stop_a_restart_or_a_sleep() {
    // Stretches of 30 s: the restart comes just after a heartbeat saved everything.
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_along(&mut rig, 30, steady);
    rig.keeper().stop().unwrap();
    rig.away(5);
    rig.start();
    type_along(&mut rig, 30, steady);
    rig.restart(policy(KeepIdle::Prompt));
    type_along(&mut rig, 30, steady);
    rig.sleep_and_wake(Duration::from_mins(1));
    type_along(&mut rig, 30, steady);
    rig.tracked_seconds();

    assert_eq!(total(&rig, "/events/key"), 120);
    assert_eq!(total(&rig, "/intervals/key/count"), 4 * 29);
    // Every counted interval is the steady second: none spans a break.
    for slot in features(&rig).iter().filter(|features| !features.is_null()) {
        assert_eq!(slot["intervals"]["key"]["entropy_bits"], 0.0, "{slot}");
        assert_eq!(slot["intervals"]["key"]["cv"], 0.0, "{slot}");
    }
}

#[test]
fn an_interval_belongs_to_the_slot_of_the_input_that_ends_it() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    // 14:03 to 14:12: 420 presses in the 14:00 window, 120 in the next.
    type_along(&mut rig, 540, steady);
    rig.tracked_seconds();

    let slots = features(&rig);
    assert_eq!(slots[0]["events"]["key"], 420);
    assert_eq!(slots[0]["intervals"]["key"]["count"], 419);
    assert_eq!(slots[1]["events"]["key"], 120);
    assert_eq!(
        slots[1]["intervals"]["key"]["count"], 120,
        "the first press of the window ends an interval begun in the previous one"
    );
}

#[test]
fn a_failed_save_counts_its_input_once() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.harness.store().fail_next_writes(1);
    let mut failures = 0;
    for _ in 0..60 {
        let from = rig.clock.reading().mono_ms;
        rig.clock.advance(Duration::from_secs(1));
        rig.idle.input();
        rig.input.push([InputEvent {
            mono_ms: from + 500,
            kind: InputKind::Key,
        }]);
        if rig.keeper().tick(BUSY).is_err() {
            failures += 1;
        }
    }
    assert_eq!(failures, 1, "the heartbeat's write failed once");
    rig.tracked_seconds();

    let slot = &features(&rig)[0];
    assert_eq!(slot["events"]["key"], 60);
    assert_eq!(
        slot["intervals"]["key"],
        serde_json::json!({"count": 59, "entropy_bits": 0.0, "cv": 0.0}),
        "the failed tick's press is counted by the next tick, at its own time"
    );
}

#[test]
fn input_around_a_sleep_is_dropped() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    type_along(&mut rig, 20, steady);
    // A press just before the lid closes and one just after it opens, drained together.
    let before = rig.clock.reading().mono_ms + 300;
    rig.clock.sleep(Duration::from_mins(2));
    rig.clock.advance(Duration::from_secs(1));
    let after = rig.clock.reading().mono_ms - 300;
    rig.input.push([before, after].map(|mono_ms| InputEvent {
        mono_ms,
        kind: InputKind::Key,
    }));
    rig.idle.input();
    let outcome = rig.tick(Activity::NONE);
    assert!(outcome.slept.is_some());
    type_along(&mut rig, 20, steady);
    rig.tracked_seconds();

    assert_eq!(total(&rig, "/events/key"), 40);
    assert_eq!(total(&rig, "/intervals/key/count"), 38);
}

#[test]
fn a_reader_that_cannot_see_input_sends_no_features() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.input.set_error(Some(SensorError::Unsupported));
    let mut person = Person(5);
    rig.start();
    type_along(&mut rig, 420, |_| person.second());
    assert_eq!(rig.tracked_seconds(), 420);
    assert_eq!(features(&rig), vec![Value::Null]);
}
