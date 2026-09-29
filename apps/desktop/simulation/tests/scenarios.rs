//! Named scenarios (E09-S14): offline, sleep, clock changes, crashes and reboots, each played
//! on a simulated device and checked end to end, from the agent's store to what the server
//! received. And a jiggler (E09-S13), which keeps the timer running but not the rhythm of a
//! person.

#![allow(
    clippy::unwrap_used,
    reason = "a scenario fails on a harness error, like on a broken invariant"
)]

use std::time::Duration;

use serde_json::Value;
use worklume_core::timekeeper::{IdlePolicy, KeepIdle};
use worklume_simulation::{Answer, Device, DeviceConfig, Report, Step, check, run};

const fn mins(minutes: u64) -> Duration {
    Duration::from_secs(minutes * 60)
}

const fn hours(hours: u64) -> Duration {
    Duration::from_secs(hours * 3_600)
}

fn play(steps: &[Step], config: DeviceConfig) -> Report {
    let report = run(steps, config).unwrap();
    assert!(report.is_clean(), "{report}");
    report
}

fn tracked_seconds(report: &Report) -> i64 {
    report.timeline.summary.slot_seconds
}

/// Ticks every 10 seconds, for scenarios that span days.
fn coarse() -> DeviceConfig {
    DeviceConfig {
        tick: Duration::from_secs(10),
        ..DeviceConfig::default()
    }
}

#[test]
fn a_working_day() {
    let report = play(
        &[
            Step::Start(1),
            Step::Work(hours(2)),
            Step::StartBreak,
            Step::Away(mins(30)),
            Step::EndBreak,
            Step::Work(hours(2)),
            Step::Switch(2),
            Step::Work(hours(1)),
            Step::Stop,
        ],
        DeviceConfig::default(),
    );
    assert_eq!(tracked_seconds(&report), 5 * 3_600);
    assert_eq!(report.timeline.summary.breaks, 1);
    assert_eq!(report.timeline.summary.entries, 3);
    assert_eq!(report.prompts, 0);
}

#[test]
fn a_week_offline_arrives_complete_and_once() {
    let mut steps = vec![Step::Offline];
    for _ in 0..7 {
        steps.extend([
            Step::Start(1),
            Step::Work(hours(3)),
            Step::Switch(2),
            Step::Work(hours(2)),
            Step::StartBreak,
            Step::Away(mins(30)),
            Step::EndBreak,
            Step::Work(hours(5)),
            Step::Switch(3),
            Step::Work(hours(1)),
            Step::Stop,
            Step::Away(mins(15)),
            // The laptop is shut overnight.
            Step::Sleep(hours(13)),
            Step::Away(mins(15)),
        ]);
    }
    // Back online after seven days, and the first answers are lost on the way back.
    steps.extend([Step::LoseResponses(3), Step::Online]);

    let report = play(&steps, coarse());
    assert_eq!(tracked_seconds(&report), 7 * 11 * 3_600);
    assert_eq!(report.timeline.summary.breaks, 7);
    assert_eq!(report.timeline.summary.slots, 7 * 66);
    assert!(
        report.timeline.summary.events > 500,
        "the week needs more than one batch: {}",
        report.timeline.summary.events
    );
    assert_eq!(
        report.duplicates,
        3 * 500,
        "each lost answer made the agent send its first batch again"
    );
}

#[test]
fn sleep_is_never_tracked_and_asks_once() {
    for keep in [KeepIdle::Prompt, KeepIdle::Always, KeepIdle::Never] {
        let config = DeviceConfig {
            policy: IdlePolicy {
                timeout: Some(mins(5)),
                keep,
            },
            ..DeviceConfig::default()
        };
        let report = play(
            &[
                Step::Start(1),
                Step::Work(mins(30)),
                Step::Away(mins(10)),
                Step::Sleep(hours(2)),
                Step::Away(mins(1)),
                Step::Answer(Answer::Keep),
                Step::Resume,
                Step::Work(mins(20)),
            ],
            config,
        );
        assert!(report.prompts <= 2, "{keep:?}: {report}");
        let tracked = tracked_seconds(&report);
        // Before the sleep: 30 minutes of work and the 10 away (or 5 until `never` stopped the
        // timer). After it: 20 minutes, unless `never` left the timer stopped.
        let expected = match keep {
            KeepIdle::Never => 35 * 60,
            _ => 60 * 60,
        };
        assert_eq!(tracked, expected, "{keep:?}");
    }
}

#[test]
fn clock_changes_never_reach_event_times() {
    let report = play(
        &[
            Step::Start(1),
            Step::Work(mins(20)),
            Step::ShiftClock(3_600),
            Step::Work(mins(20)),
            Step::ShiftClock(-7_200),
            Step::Work(mins(20)),
            Step::Stop,
        ],
        DeviceConfig::default(),
    );
    assert_eq!(tracked_seconds(&report), 60 * 60);
}

#[test]
fn crashes_lose_at_most_the_last_heartbeat() {
    let report = play(
        &[
            Step::Start(1),
            Step::Work(mins(10)),
            Step::Crash(Duration::from_secs(5)),
            Step::Work(mins(10)),
            Step::CrashMidWrite,
            Step::Work(mins(10)),
            Step::Stop,
        ],
        DeviceConfig::default(),
    );
    let tracked = tracked_seconds(&report);
    // Each crash loses what followed the last snapshot: 30 seconds at most, plus the tick in
    // flight.
    assert!(
        (30 * 60 - 2 * 31..=30 * 60).contains(&tracked),
        "{tracked} s"
    );
    assert_eq!(report.timeline.summary.entries, 3, "split at each crash");
}

#[test]
fn a_reboot_splits_the_entry_across_boots() {
    let report = play(
        &[
            Step::Start(1),
            Step::Work(mins(15)),
            Step::Reboot(mins(2)),
            Step::Work(mins(15)),
            Step::Stop,
        ],
        DeviceConfig::default(),
    );
    let tracked = tracked_seconds(&report);
    assert!((30 * 60 - 31..=30 * 60).contains(&tracked), "{tracked} s");
    assert_eq!(report.timeline.summary.entries, 2);
}

#[test]
fn lost_answers_are_sent_again_and_stored_once() {
    let report = play(
        &[
            Step::Start(1),
            Step::Work(mins(20)),
            Step::LoseResponses(2),
            Step::Work(mins(20)),
            Step::Stop,
        ],
        DeviceConfig::default(),
    );
    assert!(report.duplicates >= 2, "{report}");
    assert_eq!(tracked_seconds(&report), 40 * 60);
}

#[test]
fn a_jiggler_keeps_the_timer_running_but_not_the_rhythm_of_a_person() {
    let mut device = Device::new(DeviceConfig::default()).unwrap();
    // 09:00 to 09:20 working, 09:20 to 09:50 away with a jiggler, 09:50 to 10:00 working.
    for step in [
        Step::Start(1),
        Step::Work(mins(20)),
        Step::Jiggle {
            duration: mins(30),
            every: Duration::from_secs(30),
        },
        Step::Work(mins(10)),
        Step::Stop,
    ] {
        device.perform(step).unwrap();
    }
    device.wind_down().unwrap();
    let result = check(
        device.server().events(),
        device.untracked(),
        &device.allowance(),
    );
    assert!(result.violations.is_empty(), "{result}");
    // The OS saw input, so no idle time: only the input's rhythm gives the jiggler away.
    assert_eq!(result.summary.idle_periods, 0);
    assert_eq!(result.summary.slot_seconds, 60 * 60);
    assert_eq!(
        result.summary.inputs,
        device.inputs(),
        "all of it while tracking"
    );

    let slots: Vec<&Value> = device
        .server()
        .events()
        .iter()
        .filter(|event| event["type"] == "slot.closed")
        .map(|event| &event["payload"]["input_features"])
        .collect();
    assert_eq!(slots.len(), 6);
    for person in [slots[0], slots[1], slots[5]] {
        for kind in ["key", "pointer"] {
            let intervals = &person["intervals"][kind];
            assert!(
                intervals["entropy_bits"].as_f64().unwrap() >= 1.0,
                "{person}"
            );
            assert!(intervals["cv"].as_f64().unwrap() >= 0.3, "{person}");
        }
    }
    assert_eq!(slots[2]["events"]["key"], 0);
    // 09:30 and 09:40: nothing but the jiggler, a movement every 30 s.
    for jiggler in [slots[3], slots[4]] {
        assert_eq!(
            jiggler["events"],
            serde_json::json!({"key": 0, "pointer": 20, "click": 0, "scroll": 0})
        );
        for kind in ["any", "pointer"] {
            assert_eq!(
                jiggler["intervals"][kind],
                serde_json::json!({"count": 20, "entropy_bits": 0.0, "cv": 0.0}),
                "{jiggler}"
            );
        }
    }
}
