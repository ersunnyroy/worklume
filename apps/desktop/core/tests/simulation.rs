//! The E09-S04 acceptance criterion: over 24 hours of simulation, slot totals equal the tracked
//! wall time within 1 second.
//!
//! Each run ticks about once a second with jitter, sometimes misses ticks for up to 47 seconds,
//! takes random actions (start, stop, switch, pause, resume, idle and its resolutions, breaks),
//! moves the system clock, and crashes and restarts the agent three times. The test measures the
//! tracked time itself and compares it with the slots. It also checks the invariants the server
//! relies on: one open time entry at a time, one slot per window, no slot over 10 minutes, and
//! activity within tracked time.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use common::{Harness, seconds, target, uuid};
use uuid::Uuid;
use worklume_core::tracker::{
    Activity, DecidedBy, IdleResolution, PauseReason, Tracker, TrackerState,
};
use worklume_storage::Timestamp;

const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
const BREAK_POLICY: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0b0b);

/// xorshift64*: small, seeded and good enough to vary a simulation.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn chance(&mut self, per_million: u64) -> bool {
        self.below(1_000_000) < per_million
    }
}

fn i64_of(value: u64) -> i64 {
    i64::try_from(value).unwrap()
}

fn random_activity(rng: &mut Rng, step_ms: u64) -> Activity {
    let active_ms = rng.below(step_ms + 1);
    if rng.chance(50_000) {
        return Activity {
            active_ms,
            keyboard_ms: None,
            mouse_ms: None,
        };
    }
    let keyboard_ms = rng.below(active_ms + 1);
    Activity {
        active_ms,
        keyboard_ms: Some(keyboard_ms),
        mouse_ms: Some(active_ms - keyboard_ms),
    }
}

/// Takes one valid action for the current state.
fn act(tracker: &mut Tracker, harness: &Harness, rng: &mut Rng) {
    let now = harness.clock.now();
    let project = target(u128::from(rng.below(4)) + 1);
    let roll = rng.below(100);
    let result = match tracker.state() {
        TrackerState::Stopped if roll < 85 => tracker.start(project, &now),
        TrackerState::Stopped => tracker.start_break(BREAK_POLICY, &now),
        TrackerState::Running { .. } => match roll {
            0..30 => tracker.stop(&now),
            30..60 => tracker.switch(project, &now),
            60..75 => tracker.pause(PauseReason::Sleep, &now),
            75..90 => {
                let back = i64_of(rng.below(300_000));
                let since =
                    Timestamp::new(now.wall_ms() - back, now.mono_ms() - back, now.boot_id())
                        .unwrap();
                tracker.idle_detected(&since, &now)
            }
            _ => tracker.start_break(BREAK_POLICY, &now),
        },
        // The device sleeps while idle time awaits a decision.
        TrackerState::IdlePending { .. } if roll < 15 => tracker.pause(PauseReason::Sleep, &now),
        TrackerState::IdlePending { .. } | TrackerState::Paused { idle: Some(_), .. } => {
            let resolution = match roll {
                0..40 => IdleResolution::Keep,
                40..70 => IdleResolution::Discard,
                70..85 => IdleResolution::Reassign {
                    project_id: project.project_id,
                },
                _ => IdleResolution::Stop,
            };
            let decided_by = if roll.is_multiple_of(2) {
                DecidedBy::Member
            } else {
                DecidedBy::Policy
            };
            tracker.resolve_idle(resolution, decided_by, &now)
        }
        TrackerState::Paused { .. } if roll < 80 => tracker.resume(&now),
        TrackerState::Paused { .. } => tracker.stop(&now),
        TrackerState::OnBreak { .. } => tracker.end_break(&now),
    };
    result.unwrap();
}

/// Brings the tracker to a stop without letting time pass.
fn wind_down(tracker: &mut Tracker, harness: &Harness) {
    let now = harness.clock.now();
    if tracker.state().pending_idle().is_some() {
        tracker
            .resolve_idle(IdleResolution::Keep, DecidedBy::Member, &now)
            .unwrap();
    }
    if matches!(tracker.state(), TrackerState::OnBreak { .. }) {
        tracker.end_break(&now).unwrap();
    }
    if matches!(
        tracker.state(),
        TrackerState::Running { .. } | TrackerState::Paused { .. }
    ) {
        tracker.stop(&now).unwrap();
    }
}

fn simulate_a_day(seed: u64) {
    let mut rng = Rng(seed);
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    let mut expected_ms: u64 = 0;
    let mut crashes_left = 3;
    let end = harness.clock.mono_ms + DAY_MS;

    while harness.clock.mono_ms < end {
        let step = if rng.chance(10_000) {
            2_000 + rng.below(45_000)
        } else {
            800 + rng.below(400)
        };
        let tracking = tracker.state().is_tracking();
        let now = harness.clock.advance_ms(i64_of(step));
        if tracking {
            expected_ms += step;
        }
        tracker.tick(&now, random_activity(&mut rng, step)).unwrap();

        if rng.chance(50) {
            let shift = i64_of(rng.below(1_200_000)) - 600_000;
            harness.clock.set_wall_by(shift);
        }
        if rng.chance(1_100) {
            act(&mut tracker, &harness, &mut rng);
        }
        if crashes_left > 0 && rng.chance(40) {
            crashes_left -= 1;
            let tracking = tracker.state().is_tracking();
            drop(tracker);
            let downtime = 1_000 + rng.below(4_000);
            harness.clock.advance_ms(i64_of(downtime));
            harness.restart_store();
            let (reopened, recovery) = Tracker::open(
                Arc::clone(harness.store()),
                Harness::config(),
                &harness.clock.now(),
            )
            .unwrap();
            if tracking {
                // Time between the last snapshot and the crash was counted here but is lost.
                expected_ms -= recovery.unaccounted_ms.unwrap() - downtime;
            }
            tracker = reopened;
        }
    }
    wind_down(&mut tracker, &harness);
    harness.pass(&mut tracker, 11 * 60);

    check_slots(&harness, expected_ms, seed);
    check_entries(&harness, seed);
}

fn check_slots(harness: &Harness, expected_ms: u64, seed: u64) {
    let slots = harness.events_of("slot.closed");
    assert!(slots.len() > 50, "seed {seed}: a day holds many slots");
    let mut windows = HashSet::new();
    let mut total = 0;
    for slot in &slots {
        let start = slot.payload["slot_start"].as_str().unwrap().to_owned();
        assert!(
            start.ends_with("0:00Z"),
            "seed {seed}: {start} is not a window start"
        );
        assert!(
            windows.insert(start.clone()),
            "seed {seed}: two slots for {start}"
        );
        let tracked = seconds(slot, "tracked_seconds");
        assert!(
            (1..=600).contains(&tracked),
            "seed {seed}: {start} tracked {tracked}"
        );
        for field in [
            "active_seconds",
            "idle_seconds",
            "keyboard_seconds",
            "mouse_seconds",
        ] {
            if let Some(value) = slot.payload[field].as_u64() {
                assert!(
                    value <= tracked,
                    "seed {seed}: {start} {field} {value} > {tracked}"
                );
            }
        }
        total += tracked;
    }
    let expected = expected_ms / 1_000;
    assert!(
        total.abs_diff(expected) <= 1,
        "seed {seed}: slots hold {total} s, tracked time was {expected_ms} ms"
    );
}

/// At most one open time entry at any point in the event stream, and none at the end.
fn check_entries(harness: &Harness, seed: u64) {
    let mut open: Option<Uuid> = None;
    for event in harness.events() {
        let payload = &event.payload;
        match event.event_type.as_str() {
            "timer.started" => {
                assert_eq!(open, None, "seed {seed}: a second entry opened");
                open = Some(*event.event_id.as_uuid());
            }
            "timer.stopped" => {
                assert_eq!(open, Some(uuid(&payload["time_entry_id"])), "seed {seed}");
                open = None;
            }
            "idle.detected" => {
                assert_eq!(open, Some(uuid(&payload["time_entry_id"])), "seed {seed}");
            }
            "idle.resolved" if payload["resolution"] == "stop" => {
                assert!(open.is_some(), "seed {seed}: idle stop without a timer");
                open = None;
            }
            "break.started" => assert_eq!(open, None, "seed {seed}: break over a timer"),
            _ => {}
        }
    }
    assert_eq!(open, None, "seed {seed}: an entry was left open");
}

#[test]
fn a_day_of_tracking_adds_up_to_the_second() {
    simulate_a_day(0x5eed_0001);
}

#[test]
fn another_day_with_different_luck() {
    simulate_a_day(0x5eed_0002);
}
