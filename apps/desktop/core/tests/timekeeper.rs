//! The timekeeper: idle detection, sleep and the keep-idle policy, on fake clocks.
//!
//! The acceptance criterion of E09-S08: a two-hour sleep produces no tracked time and one idle
//! prompt at most, whatever the policy and whatever the member was doing before it.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::time::Duration;

use common::rig::{Rig, TWO_HOURS, policy};
use common::{START_WALL_MS, target, uuid};
use worklume_core::timekeeper::{IdlePolicy, KeepIdle, Prompt};
use worklume_core::tracker::{Activity, IdleResolution, PauseReason, TrackerState};
use worklume_platform::IdleError;

// ---------------------------------------------------------------------------------------------
// The acceptance criterion
// ---------------------------------------------------------------------------------------------

/// What the member was doing when the device went to sleep.
#[derive(Debug, Clone, Copy)]
enum Before {
    /// Working until the lid closed.
    Working,
    /// Away for three minutes, below the idle timeout.
    BrieflyAway,
    /// Away for ten minutes: idle time was detected and awaits a decision.
    LongAway,
}

impl Before {
    const fn away_seconds(self) -> u64 {
        match self {
            Self::Working => 0,
            Self::BrieflyAway => 180,
            Self::LongAway => 600,
        }
    }
}

#[test]
fn a_two_hour_sleep_is_never_tracked_and_asks_once_at_most() {
    for keep in [KeepIdle::Prompt, KeepIdle::Always, KeepIdle::Never] {
        for before in [Before::Working, Before::BrieflyAway, Before::LongAway] {
            let case = format!("{keep:?}, {before:?}");
            let mut rig = Rig::new(policy(keep));
            rig.start();
            rig.work(240);
            rig.away(before.away_seconds());
            let tracked_before_sleep = 240 + before.away_seconds();

            let outcome = rig.sleep_and_wake(TWO_HOURS);
            assert!(
                !rig.state().is_tracking(),
                "{case}: tracking resumed on its own after a long sleep"
            );
            // The member takes half a minute to notice the dialog.
            rig.away(30);
            assert!(rig.prompts.len() <= 1, "{case}: {:?}", rig.prompts);
            assert!(
                rig.harness.events_of("idle.detected").len() <= 1,
                "{case}: one idle period at most"
            );

            let expected_prompt = match (keep, before) {
                (KeepIdle::Never, _) => None,
                (KeepIdle::Prompt, Before::Working) | (KeepIdle::Always, _) => Some("resume"),
                (KeepIdle::Prompt, _) => Some("idle"),
            };
            let shown = rig.prompt().map(|prompt| match prompt {
                Prompt::Idle { paused, .. } => {
                    assert!(paused, "{case}");
                    "idle"
                }
                Prompt::Resume { .. } => "resume",
            });
            assert_eq!(shown, expected_prompt, "{case}");
            if keep == KeepIdle::Never {
                assert_eq!(rig.state(), &TrackerState::Stopped, "{case}");
            }
            // The long-away prompt was raised before the sleep; the others on waking.
            match before {
                Before::LongAway if keep == KeepIdle::Prompt => {
                    assert!(
                        outcome.new_prompt.is_none(),
                        "{case}: asked again on waking"
                    );
                }
                _ => assert_eq!(
                    outcome.new_prompt.is_some(),
                    expected_prompt.is_some(),
                    "{case}"
                ),
            }
            assert_eq!(outcome.slept, Some(TWO_HOURS), "{case}");

            let stops = rig.harness.events_of("timer.stopped");
            let sleep_stops: Vec<_> = stops
                .iter()
                .filter(|stop| stop.payload["reason"] == "sleep")
                .collect();
            let asleep_at = START_WALL_MS + i64::try_from(tracked_before_sleep).unwrap() * 1_000;
            if let (KeepIdle::Never, Before::LongAway) = (keep, before) {
                // The timer had already stopped for idle time.
                assert!(sleep_stops.is_empty(), "{case}");
            } else {
                assert_eq!(sleep_stops.len(), 1, "{case}");
                assert_eq!(sleep_stops[0].time.wall_ms(), asleep_at, "{case}");
            }

            // Nothing is tracked from the sleep onwards: every slot lies before it.
            let tracked = rig.tracked_seconds();
            let expected_tracked = match (keep, before) {
                // `never` stopped the timer at detection, five minutes into the absence.
                (KeepIdle::Never, Before::LongAway) => 240 + 300,
                _ => tracked_before_sleep,
            };
            assert_eq!(tracked, expected_tracked, "{case}");
            assert!(
                rig.slot_starts()
                    .iter()
                    .all(|start| start.as_str() < "2026-09-21T14:20:00Z"),
                "{case}: {:?}",
                rig.slot_starts()
            );
        }
    }
}

#[test]
fn a_short_sleep_resumes_on_its_own() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    let outcome = rig.sleep_and_wake(Duration::from_mins(2));
    assert_eq!(outcome.slept, Some(Duration::from_mins(2)));
    assert!(outcome.new_prompt.is_none());
    assert!(rig.state().is_tracking());
    rig.work(59);

    assert_eq!(
        rig.harness
            .event_types()
            .into_iter()
            .filter(|kind| kind != "slot.closed")
            .collect::<Vec<_>>(),
        ["timer.started", "timer.stopped", "timer.started"]
    );
    let stop = rig.harness.events_of("timer.stopped").remove(0);
    assert_eq!(stop.payload["reason"], "sleep");
    let resumed = rig.harness.events_of("timer.started").remove(1);
    assert_eq!(resumed.payload["trigger"], "resume");
    assert_eq!(resumed.time.wall_ms() - stop.time.wall_ms(), 121_000);
    // Neither the two minutes asleep nor the second around them are tracked.
    assert_eq!(rig.tracked_seconds(), 60 + 59);
    assert!(rig.prompts.is_empty());
}

#[test]
fn time_away_before_a_short_sleep_counts_towards_the_absence() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    // Four minutes away, then two asleep: six minutes away in all, over the timeout.
    rig.away(240);
    let outcome = rig.sleep_and_wake(Duration::from_mins(2));
    assert!(!rig.state().is_tracking());
    let Some(Prompt::Idle { idle, paused: true }) = outcome.new_prompt else {
        panic!("asked about the idle time: {outcome:?}");
    };
    assert_eq!(idle.since.wall_ms, START_WALL_MS + 60_000);
}

#[test]
fn idle_before_a_sleep_begins_no_earlier_than_the_entry() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.away(200);
    // The agent restarts, so a new entry begins now; then the lid closes.
    rig.restart(policy(KeepIdle::Prompt));
    rig.sleep_and_wake(Duration::from_mins(2));

    // Away for 320 s in all, over the timeout, but none of it inside the new entry.
    assert!(rig.harness.events_of("idle.detected").is_empty());
    assert!(matches!(rig.prompt(), Some(Prompt::Resume { .. })));
}

#[test]
fn answering_after_a_sleep_resumes_the_timer() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.away(180);
    rig.sleep_and_wake(TWO_HOURS);
    let Some(Prompt::Idle { idle, paused: true }) = rig.prompt() else {
        panic!("an idle prompt after the sleep: {:?}", rig.prompt());
    };
    let detected = rig.harness.events_of("idle.detected").remove(0);
    assert_eq!(
        detected.time.wall_ms(),
        START_WALL_MS + 60_000,
        "idle began at the last input"
    );
    rig.harness.clear_events();

    rig.resolve(IdleResolution::Discard);
    assert!(rig.state().is_tracking());
    assert_eq!(
        rig.harness.event_types(),
        ["idle.resolved", "timer.started"]
    );
    let resolved = rig.harness.events_of("idle.resolved").remove(0);
    assert_eq!(
        uuid(&resolved.payload["idle_period_id"]),
        idle.idle_period_id
    );
    assert_eq!(resolved.payload["resolution"], "discard");
    assert_eq!(resolved.payload["decided_by"], "member");
    assert_eq!(
        rig.harness.events_of("timer.started")[0].payload["trigger"],
        "resume"
    );
}

#[test]
fn the_resume_prompt_resumes_or_stays_stopped() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.sleep_and_wake(TWO_HOURS);
    assert_eq!(rig.prompt(), Some(Prompt::Resume { target: target(1) }));
    rig.keeper().resume().unwrap();
    assert!(rig.state().is_tracking());

    rig.work(10);
    rig.sleep_and_wake(TWO_HOURS);
    rig.keeper().stop().unwrap();
    assert_eq!(rig.state(), &TrackerState::Stopped);
    assert_eq!(rig.prompt(), None);
}

#[test]
fn a_click_just_after_waking_lands_after_the_sleep() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.clock.sleep(TWO_HOURS);
    rig.idle.input();
    // The member stops the timer before the agent's loop has ticked.
    let outcome = rig.keeper().stop().unwrap();
    assert_eq!(outcome.slept, Some(TWO_HOURS));
    assert_eq!(rig.state(), &TrackerState::Stopped);
    let stops = rig.harness.events_of("timer.stopped");
    assert_eq!(stops.len(), 1, "the entry closed once, at the sleep");
    assert_eq!(stops[0].payload["reason"], "sleep");
    assert_eq!(rig.tracked_seconds(), 60);
}

#[test]
fn nothing_happens_to_a_sleep_while_stopped_or_on_a_break() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.sleep_and_wake(TWO_HOURS);
    assert_eq!(rig.state(), &TrackerState::Stopped);

    rig.keeper()
        .start_break(uuid::Uuid::from_u128(0x0b0b))
        .unwrap();
    rig.sleep_and_wake(TWO_HOURS);
    assert!(matches!(rig.state(), TrackerState::OnBreak { .. }));
    assert!(rig.prompts.is_empty());
    assert!(rig.harness.events_of("idle.detected").is_empty());
}

// ---------------------------------------------------------------------------------------------
// Idle while awake
// ---------------------------------------------------------------------------------------------

#[test]
fn idle_is_detected_at_the_timeout_and_dated_from_the_last_input() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.away(299);
    assert!(rig.harness.events_of("idle.detected").is_empty());
    rig.away(1);
    let detected = rig.harness.events_of("idle.detected");
    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].time.wall_ms(), START_WALL_MS + 60_000);
    assert!(matches!(
        rig.prompts.as_slice(),
        [Prompt::Idle { paused: false, .. }]
    ));

    // Still away an hour later: the same question, still one idle period.
    rig.away(3_600);
    assert_eq!(rig.prompts.len(), 1);
    assert_eq!(rig.harness.events_of("idle.detected").len(), 1);
}

#[test]
fn after_a_decision_idle_counts_from_the_decision() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(10);
    rig.away(300);
    // Decided without touching this device's keyboard, for example on the web (E09-S05).
    rig.resolve(IdleResolution::Keep);
    let decided_at = rig.clock.reading().wall_ms;
    rig.away(299);
    assert_eq!(rig.harness.events_of("idle.detected").len(), 1);
    rig.away(1);
    let detected = rig.harness.events_of("idle.detected");
    assert_eq!(detected.len(), 2);
    assert_eq!(
        detected[1].time.wall_ms(),
        decided_at,
        "not from the input before the first idle period"
    );
}

#[test]
fn always_keeps_idle_time_when_the_member_returns() {
    let mut rig = Rig::new(policy(KeepIdle::Always));
    rig.start();
    rig.work(60);
    rig.away(600);
    assert!(matches!(rig.state(), TrackerState::IdlePending { .. }));
    assert_eq!(rig.prompt(), None, "no dialog");
    rig.work(1);

    assert!(matches!(rig.state(), TrackerState::Running { .. }));
    let resolved = rig.harness.events_of("idle.resolved").remove(0);
    assert_eq!(resolved.payload["resolution"], "keep");
    assert_eq!(resolved.payload["decided_by"], "policy");
    assert_eq!(resolved.time.wall_ms(), START_WALL_MS + 661_000);
    assert!(rig.prompts.is_empty());
}

#[test]
fn never_discards_idle_time_and_stops_the_timer() {
    let mut rig = Rig::new(policy(KeepIdle::Never));
    rig.start();
    rig.work(60);
    rig.away(300);
    assert_eq!(rig.state(), &TrackerState::Stopped);
    let resolved = rig.harness.events_of("idle.resolved").remove(0);
    assert_eq!(resolved.payload["resolution"], "stop");
    assert_eq!(resolved.payload["decided_by"], "policy");
    assert!(rig.prompts.is_empty());
}

#[test]
fn unknown_idle_time_detects_nothing() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.idle.set_error(Some(IdleError::Unsupported));
    rig.away(3_600);
    assert!(rig.harness.events_of("idle.detected").is_empty());
    // A long sleep still waits for the member, counting only the sleep as absence.
    rig.sleep_and_wake(TWO_HOURS);
    assert!(matches!(rig.prompt(), Some(Prompt::Resume { .. })));
}

#[test]
fn without_a_timeout_idle_is_not_detected_and_sleep_still_is_not_tracked() {
    let mut rig = Rig::new(IdlePolicy {
        timeout: None,
        keep: KeepIdle::Prompt,
    });
    rig.start();
    rig.work(60);
    rig.away(3_600);
    assert!(rig.harness.events_of("idle.detected").is_empty());
    rig.sleep_and_wake(TWO_HOURS);
    assert!(
        rig.state().is_tracking(),
        "no timeout, so no absence is long"
    );
    assert_eq!(rig.tracked_seconds(), 60 + 3_600);
    assert!(rig.prompts.is_empty());
}

// ---------------------------------------------------------------------------------------------
// Restarts, policy changes and failures
// ---------------------------------------------------------------------------------------------

#[test]
fn a_restart_keeps_the_question() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.away(300);
    let Some(Prompt::Idle { idle, .. }) = rig.prompt() else {
        panic!("idle prompt");
    };
    rig.restart(policy(KeepIdle::Prompt));
    let Some(Prompt::Idle { idle: after, .. }) = rig.prompt() else {
        panic!("still asked after a restart: {:?}", rig.state());
    };
    assert_eq!(after.idle_period_id, idle.idle_period_id);

    // A restart under a policy that no longer asks applies it.
    rig.restart(policy(KeepIdle::Never));
    assert_eq!(rig.state(), &TrackerState::Stopped);
}

#[test]
fn a_new_policy_applies_from_the_next_call() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.away(300);
    assert!(rig.prompt().is_some());
    rig.keeper().set_policy(policy(KeepIdle::Always));
    assert_eq!(rig.keeper().policy(), policy(KeepIdle::Always));
    rig.work(1);
    assert!(matches!(rig.state(), TrackerState::Running { .. }));
}

#[test]
fn a_failed_clock_read_changes_nothing() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.clock.set_failing(true);
    rig.clock.sleep(TWO_HOURS);
    assert!(rig.keeper().tick(Activity::NONE).is_err());
    assert!(rig.state().is_tracking());
    rig.clock.set_failing(false);
    rig.clock.advance(Duration::from_secs(1));
    let outcome = rig.tick(Activity::NONE);
    assert_eq!(
        outcome.slept,
        Some(TWO_HOURS),
        "the sleep is seen once reads work"
    );
    assert!(matches!(
        rig.state(),
        TrackerState::Paused {
            reason: PauseReason::Sleep,
            ..
        }
    ));
}

#[test]
fn a_failed_save_while_falling_asleep_is_retried() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.start();
    rig.work(60);
    rig.away(180);
    rig.clock.sleep(TWO_HOURS);
    rig.clock.advance(Duration::from_secs(1));
    // The pause fails after the idle time was recorded.
    rig.harness.store().fail_next_writes(1);
    let first = rig.keeper().tick(Activity::NONE);
    assert!(first.is_err());
    rig.clock.advance(Duration::from_secs(1));
    rig.tick(Activity::NONE);

    assert_eq!(rig.harness.events_of("idle.detected").len(), 1);
    assert!(matches!(
        rig.prompt(),
        Some(Prompt::Idle { paused: true, .. })
    ));
    assert_eq!(rig.tracked_seconds(), 240);
}
