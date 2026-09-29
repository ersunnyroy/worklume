//! The tracker's state machine: every action in every state, the events each produces, and
//! that a failed save leaves the tracker unchanged.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use common::{Harness, target, uuid};
use serde_json::json;
use uuid::Uuid;
use worklume_core::tracker::{
    Action, DecidedBy, IdleResolution, PauseReason, StateName, Tracker, TrackerError, TrackerState,
};

const POLICY: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0b0b);

/// Where a test starts: each state, plus a pause that began while idle time awaited a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setup {
    Stopped,
    Running,
    IdlePending,
    Paused,
    PausedAwaitingIdle,
    OnBreak,
}

/// A new tracker brought into `setup`.
fn tracker_in(harness: &mut Harness, setup: Setup) -> Tracker {
    let mut tracker = harness.tracker();
    let now = harness.clock.now();
    let expected = match setup {
        Setup::Stopped => StateName::Stopped,
        Setup::Running => {
            tracker.start(target(1), &now).unwrap();
            StateName::Running
        }
        Setup::IdlePending | Setup::PausedAwaitingIdle => {
            tracker.start(target(1), &now).unwrap();
            let since = harness.pass(&mut tracker, 10);
            let now = harness.pass(&mut tracker, 300);
            tracker.idle_detected(&since, &now).unwrap();
            if setup == Setup::IdlePending {
                StateName::IdlePending
            } else {
                let now = harness.pass(&mut tracker, 5);
                tracker.pause(PauseReason::Sleep, &now).unwrap();
                StateName::Paused
            }
        }
        Setup::Paused => {
            tracker.start(target(1), &now).unwrap();
            let now = harness.clock.advance_secs(5);
            tracker.pause(PauseReason::Sleep, &now).unwrap();
            StateName::Paused
        }
        Setup::OnBreak => {
            tracker.start(target(1), &now).unwrap();
            let now = harness.clock.advance_secs(5);
            tracker.start_break(POLICY, &now).unwrap();
            StateName::OnBreak
        }
    };
    assert_eq!(tracker.state().name(), expected);
    tracker
}

#[derive(Debug, Clone, Copy)]
enum Act {
    Start,
    Switch,
    Stop,
    IdleDetected,
    ResolveIdle,
    Pause,
    Resume,
    StartBreak,
    EndBreak,
}

fn perform(tracker: &mut Tracker, harness: &mut Harness, act: Act) -> Result<(), TrackerError> {
    let since = harness.clock.now();
    let now = harness.clock.advance_secs(1);
    match act {
        Act::Start => tracker.start(target(2), &now),
        Act::Switch => tracker.switch(target(2), &now),
        Act::Stop => tracker.stop(&now),
        Act::IdleDetected => tracker.idle_detected(&since, &now),
        Act::ResolveIdle => tracker.resolve_idle(IdleResolution::Keep, DecidedBy::Member, &now),
        Act::Pause => tracker.pause(PauseReason::SessionLocked, &now),
        Act::Resume => tracker.resume(&now),
        Act::StartBreak => tracker.start_break(POLICY, &now),
        Act::EndBreak => tracker.end_break(&now),
    }
}

/// A starting point, and what each action does from it: `None` refused, `Some(state)` accepted.
type Row = (Setup, [(Act, Option<StateName>); 9]);

/// Every starting point, and what each action does from it.
fn transition_table() -> [Row; 6] {
    use Act::*;
    use StateName::*;
    [
        (
            Setup::Stopped,
            [
                (Start, Some(Running)),
                (Switch, None),
                (Stop, None),
                (IdleDetected, Some(Stopped)),
                (ResolveIdle, None),
                (Pause, Some(Stopped)),
                (Resume, Some(Stopped)),
                (StartBreak, Some(OnBreak)),
                (EndBreak, None),
            ],
        ),
        (
            Setup::Running,
            [
                (Start, None),
                (Switch, Some(Running)),
                (Stop, Some(Stopped)),
                (IdleDetected, Some(IdlePending)),
                (ResolveIdle, None),
                (Pause, Some(Paused)),
                (Resume, Some(Running)),
                (StartBreak, Some(OnBreak)),
                (EndBreak, None),
            ],
        ),
        (
            Setup::IdlePending,
            [
                (Start, None),
                (Switch, None),
                (Stop, None),
                (IdleDetected, Some(IdlePending)),
                (ResolveIdle, Some(Running)),
                (Pause, Some(Paused)),
                (Resume, Some(IdlePending)),
                (StartBreak, None),
                (EndBreak, None),
            ],
        ),
        (
            Setup::Paused,
            [
                (Start, None),
                (Switch, None),
                (Stop, Some(Stopped)),
                (IdleDetected, Some(Paused)),
                (ResolveIdle, None),
                (Pause, Some(Paused)),
                (Resume, Some(Running)),
                (StartBreak, None),
                (EndBreak, None),
            ],
        ),
        (
            Setup::PausedAwaitingIdle,
            [
                (Start, None),
                (Switch, None),
                (Stop, None),
                (IdleDetected, Some(Paused)),
                (ResolveIdle, Some(Paused)),
                (Pause, Some(Paused)),
                (Resume, None),
                (StartBreak, None),
                (EndBreak, None),
            ],
        ),
        (
            Setup::OnBreak,
            [
                (Start, None),
                (Switch, None),
                (Stop, None),
                (IdleDetected, Some(OnBreak)),
                (ResolveIdle, None),
                (Pause, Some(OnBreak)),
                (Resume, Some(OnBreak)),
                (StartBreak, None),
                (EndBreak, Some(Running)),
            ],
        ),
    ]
}

#[test]
fn every_action_in_every_state() {
    for (from, row) in transition_table() {
        for (act, expected) in row {
            let mut harness = Harness::new();
            let mut tracker = tracker_in(&mut harness, from);
            let before = tracker.state().clone();
            let events_before = harness.events().len();
            let result = perform(&mut tracker, &mut harness, act);
            if let Some(state) = expected {
                assert!(result.is_ok(), "{act:?} from {from:?}: {result:?}");
                assert_eq!(tracker.state().name(), state, "{act:?} from {from:?}");
            } else {
                assert!(
                    matches!(result, Err(TrackerError::InvalidTransition { .. })),
                    "{act:?} from {from:?} should be refused: {result:?}"
                );
                assert_eq!(tracker.state(), &before, "{act:?} from {from:?}");
                assert_eq!(harness.events().len(), events_before, "nothing written");
            }
        }
    }
}

#[test]
fn start_switch_and_stop_describe_the_time_entries() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let first = tracker.state().clone();
    let switched_at = harness.pass(&mut tracker, 90);
    tracker.switch(target(2), &switched_at).unwrap();
    let stopped_at = harness.pass(&mut tracker, 30);
    tracker.stop(&stopped_at).unwrap();

    assert_eq!(
        harness.event_types(),
        [
            "timer.started",
            "timer.stopped",
            "timer.started",
            "timer.stopped"
        ]
    );
    let events = harness.events();
    let TrackerState::Running { entry } = first else {
        panic!("running after start");
    };
    assert_eq!(*events[0].event_id.as_uuid(), entry.time_entry_id);
    assert_eq!(
        events[0].payload,
        common_payload(json!({
            "project_id": target(1).project_id,
            "task_id": null,
            "trigger": "user",
        }))
    );
    assert_eq!(events[1].payload["reason"], "switch");
    assert_eq!(
        uuid(&events[1].payload["time_entry_id"]),
        entry.time_entry_id
    );
    assert_eq!(events[2].payload["trigger"], "switch");
    assert_eq!(events[1].time, events[2].time, "the switch is one moment");
    assert_eq!(events[1].time.wall_ms(), switched_at.wall_ms());
    assert_eq!(events[3].payload["reason"], "user");
    assert_eq!(
        uuid(&events[3].payload["time_entry_id"]),
        *events[2].event_id.as_uuid()
    );
}

fn common_payload(value: serde_json::Value) -> worklume_storage::Payload {
    match value {
        serde_json::Value::Object(map) => map,
        _ => panic!("an object"),
    }
}

#[test]
fn pausing_is_safe_to_repeat_and_resumes_in_a_new_entry() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let paused_at = harness.pass(&mut tracker, 60);
    tracker.pause(PauseReason::Sleep, &paused_at).unwrap();
    tracker
        .pause(PauseReason::Sleep, &harness.clock.advance_secs(60))
        .unwrap();
    tracker.resume(&harness.clock.advance_secs(60)).unwrap();
    tracker.resume(&harness.clock.advance_secs(1)).unwrap();

    assert_eq!(
        harness.event_types(),
        ["timer.started", "timer.stopped", "timer.started"]
    );
    let events = harness.events();
    assert_eq!(events[1].payload["reason"], "sleep");
    assert_eq!(events[2].payload["trigger"], "resume");
    assert_eq!(events[2].payload["project_id"], json!(target(1).project_id));
    assert_ne!(events[2].event_id, events[0].event_id);

    // Stopping while paused: the entry is already closed, so nothing more is sent.
    tracker
        .pause(PauseReason::SessionLocked, &harness.clock.advance_secs(1))
        .unwrap();
    harness.clear_events();
    tracker.stop(&harness.clock.advance_secs(1)).unwrap();
    assert!(harness.events().is_empty());
    assert_eq!(tracker.state(), &TrackerState::Stopped);
}

#[test]
fn a_pause_keeps_an_idle_decision_waiting() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let since = harness.pass(&mut tracker, 60);
    let detected = harness.pass(&mut tracker, 300);
    tracker.idle_detected(&since, &detected).unwrap();
    let TrackerState::IdlePending { entry, idle } = tracker.state().clone() else {
        panic!("idle pending");
    };
    let asleep = harness.pass(&mut tracker, 30);
    harness.clear_events();
    tracker.pause(PauseReason::Sleep, &asleep).unwrap();

    assert_eq!(
        tracker.state(),
        &TrackerState::Paused {
            target: target(1),
            reason: PauseReason::Sleep,
            idle: Some(idle.clone()),
        }
    );
    // The entry closes, and the idle period ends with it.
    assert_eq!(harness.event_types(), ["timer.stopped"]);
    let stop = harness.events_of("timer.stopped").remove(0);
    assert_eq!(stop.payload["reason"], "sleep");
    assert_eq!(uuid(&stop.payload["time_entry_id"]), entry.time_entry_id);

    // Resuming or stopping now would drop the decision.
    let woke = harness.clock.advance_secs(7_200);
    let refused = |result: Result<(), TrackerError>, expected: Action| {
        assert!(
            matches!(result, Err(TrackerError::InvalidTransition { action, .. }) if action == expected),
            "{expected:?}"
        );
    };
    refused(tracker.resume(&woke), Action::Resume);
    refused(tracker.stop(&woke), Action::Stop);

    harness.clear_events();
    tracker
        .resolve_idle(IdleResolution::Discard, DecidedBy::Member, &woke)
        .unwrap();
    assert_eq!(
        tracker.state(),
        &TrackerState::Paused {
            target: target(1),
            reason: PauseReason::Sleep,
            idle: None,
        }
    );
    let resolved = harness.events_of("idle.resolved").remove(0);
    assert_eq!(
        uuid(&resolved.payload["idle_period_id"]),
        idle.idle_period_id
    );
    assert_eq!(
        uuid(&resolved.payload["time_entry_id"]),
        entry.time_entry_id
    );
    assert_eq!(resolved.payload["resolution"], "discard");
    assert_eq!(resolved.payload["decided_by"], "member");
    tracker.resume(&woke).unwrap();
    assert!(tracker.state().is_tracking());
}

#[test]
fn the_stop_resolution_stops_a_paused_timer() {
    let mut harness = Harness::new();
    let mut tracker = tracker_in(&mut harness, Setup::PausedAwaitingIdle);
    harness.clear_events();
    tracker
        .resolve_idle(
            IdleResolution::Stop,
            DecidedBy::Policy,
            &harness.clock.advance_secs(60),
        )
        .unwrap();
    assert_eq!(tracker.state(), &TrackerState::Stopped);
    assert_eq!(
        harness.event_types(),
        ["idle.resolved"],
        "the entry closed at the pause"
    );
    let resolved = harness.events_of("idle.resolved").remove(0);
    assert_eq!(resolved.payload["resolution"], "stop");
    assert_eq!(resolved.payload["decided_by"], "policy");
}

#[test]
fn a_break_closes_the_entry_and_resumes_afterwards() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(3), &harness.clock.now()).unwrap();
    let break_at = harness.pass(&mut tracker, 600);
    harness.clear_events();
    tracker.start_break(POLICY, &break_at).unwrap();
    tracker.end_break(&harness.clock.advance_secs(900)).unwrap();

    let events: Vec<_> = harness
        .events()
        .into_iter()
        .filter(|event| event.event_type.as_str() != "slot.closed")
        .collect();
    let types: Vec<_> = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            "timer.stopped",
            "break.started",
            "break.ended",
            "timer.started"
        ]
    );
    assert_eq!(events[0].payload["reason"], "break");
    assert_eq!(events[1].payload["break_policy_id"], json!(POLICY));
    assert_eq!(
        uuid(&events[2].payload["break_id"]),
        *events[1].event_id.as_uuid()
    );
    assert_eq!(events[3].payload["trigger"], "break_end");
    assert_eq!(events[3].payload["project_id"], json!(target(3).project_id));

    // A break from a stopped timer ends stopped.
    tracker.stop(&harness.clock.advance_secs(1)).unwrap();
    harness.clear_events();
    tracker
        .start_break(POLICY, &harness.clock.advance_secs(1))
        .unwrap();
    tracker.end_break(&harness.clock.advance_secs(60)).unwrap();
    assert_eq!(harness.event_types(), ["break.started", "break.ended"]);
    assert_eq!(tracker.state(), &TrackerState::Stopped);
}

#[test]
fn idle_is_dated_when_it_began_and_resolved_by_the_server() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let since = harness.pass(&mut tracker, 120);
    let detected = harness.pass(&mut tracker, 300);
    tracker.idle_detected(&since, &detected).unwrap();

    let idle = harness.events_of("idle.detected").remove(0);
    assert_eq!(idle.time.wall_ms(), since.wall_ms());
    let TrackerState::IdlePending {
        entry,
        idle: period,
    } = tracker.state().clone()
    else {
        panic!("idle pending");
    };
    assert_eq!(*idle.event_id.as_uuid(), period.idle_period_id);
    assert_eq!(uuid(&idle.payload["time_entry_id"]), entry.time_entry_id);

    // Reassigning keeps the timer on its own entry; the server moves the idle time.
    let other = target(9).project_id;
    tracker
        .resolve_idle(
            IdleResolution::Reassign { project_id: other },
            DecidedBy::Member,
            &harness.clock.advance_secs(20),
        )
        .unwrap();
    let resolved = harness.events_of("idle.resolved").remove(0);
    assert_eq!(resolved.payload["resolution"], "reassign");
    assert_eq!(resolved.payload["reassigned_project_id"], json!(other));
    assert_eq!(
        uuid(&resolved.payload["idle_period_id"]),
        period.idle_period_id
    );
    assert_eq!(
        tracker.state(),
        &TrackerState::Running {
            entry: entry.clone()
        }
    );

    // "Stop" ends the timer; idle.resolved alone tells the server where the entry ends.
    let since = harness.pass(&mut tracker, 60);
    let detected = harness.pass(&mut tracker, 300);
    tracker.idle_detected(&since, &detected).unwrap();
    harness.clear_events();
    tracker
        .resolve_idle(
            IdleResolution::Stop,
            DecidedBy::Member,
            &harness.clock.advance_secs(5),
        )
        .unwrap();
    assert_eq!(harness.event_types(), ["idle.resolved"]);
    assert_eq!(harness.events()[0].payload["resolution"], "stop");
    assert_eq!(tracker.state(), &TrackerState::Stopped);
}

#[test]
fn idle_cannot_begin_before_the_entry_or_after_now() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    let before_start = harness.clock.now();
    let started = harness.clock.advance_secs(30);
    tracker.start(target(1), &started).unwrap();
    let detected = harness.pass(&mut tracker, 300);
    tracker.idle_detected(&before_start, &detected).unwrap();
    assert_eq!(
        harness.events_of("idle.detected")[0].time.wall_ms(),
        started.wall_ms()
    );

    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let now = harness.clock.advance_secs(60);
    let future = harness.clock.advance_secs(60);
    tracker.idle_detected(&future, &now).unwrap();
    assert_eq!(
        harness.events_of("idle.detected")[0].time.wall_ms(),
        now.wall_ms()
    );
}

#[test]
fn a_failed_save_leaves_the_tracker_unchanged() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    harness.store().fail_next_writes(1);
    let result = tracker.start(target(1), &harness.clock.now());
    assert!(matches!(result, Err(TrackerError::Storage(_))));
    assert_eq!(tracker.state(), &TrackerState::Stopped);
    assert!(harness.events().is_empty());

    tracker
        .start(target(1), &harness.clock.advance_secs(1))
        .unwrap();
    assert_eq!(harness.event_types(), ["timer.started"]);

    // A failed tick loses no time: the next one accounts it. The 14:00 window ends 419 s after
    // this start at 14:03:01.
    harness.pass(&mut tracker, 418);
    harness.store().fail_next_writes(1);
    let crossing = harness.clock.advance_secs(1);
    assert!(
        tracker
            .tick(&crossing, worklume_core::tracker::Activity::NONE)
            .is_err()
    );
    tracker
        .tick(
            &harness.clock.advance_secs(1),
            worklume_core::tracker::Activity::NONE,
        )
        .unwrap();
    let slots = harness.events_of("slot.closed");
    assert_eq!(slots.len(), 1, "the slot closed once, on the retry");
    assert_eq!(slots[0].payload["tracked_seconds"], 419);
}

#[test]
fn the_journal_keeps_a_single_snapshot() {
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    tracker.start(target(1), &harness.clock.now()).unwrap();
    harness.pass(&mut tracker, 200);
    tracker.stop(&harness.clock.advance_secs(1)).unwrap();
    let journal = harness.store().journal_after(None, 100).unwrap();
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].kind.as_str(), "tracker.snapshot");
    assert_eq!(journal[0].payload["state"]["state"], "stopped");
}
