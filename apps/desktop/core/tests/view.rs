//! The timer view (E09-S09): what the agent's UI shows, today's total, the question asked, and
//! what the member may do in each state.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::time::Duration;

use common::START_WALL_MS;
use common::rig::{Rig, policy};
use uuid::Uuid;
use worklume_core::catalog::{BreakPolicy, Catalog, Project, Task};
use worklume_core::timekeeper::KeepIdle;
use worklume_core::tracker::{IdleResolution, PauseReason, WorkTarget};
use worklume_core::view::{Allowed, Question, TimerStatus, TimerView, WorkView, timer_view};

/// Midnight UTC of the rig's day, 2026-09-21.
const MIDNIGHT: i64 = START_WALL_MS - (14 * 60 + 3) * 60_000;
const LUNCH: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0b01);
const TASK: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0c01);

fn project_id(n: u128) -> Uuid {
    common::target(n).project_id
}

fn catalog() -> Catalog {
    Catalog {
        projects: vec![
            Project {
                id: project_id(1),
                name: "Website".to_owned(),
                tasks: vec![Task {
                    id: TASK,
                    name: "Checkout".to_owned(),
                }],
            },
            Project {
                id: project_id(2),
                name: "Support".to_owned(),
                tasks: Vec::new(),
            },
        ],
        break_policies: vec![BreakPolicy {
            id: LUNCH,
            name: "Lunch".to_owned(),
        }],
    }
}

fn view(rig: &mut Rig, catalog: &Catalog) -> TimerView {
    timer_view(rig.keeper(), catalog, MIDNIGHT)
}

fn minutes(n: u64) -> Duration {
    Duration::from_secs(n * 60)
}

const NOTHING: Allowed = Allowed {
    start: false,
    switch: false,
    stop: false,
    start_break: false,
    end_break: false,
};

#[test]
fn the_view_follows_the_timer() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let catalog = catalog();
    let stopped = view(&mut rig, &catalog);
    assert_eq!(stopped.status, TimerStatus::Stopped);
    assert_eq!(stopped.work, None);
    assert_eq!(stopped.today, Duration::ZERO);
    assert_eq!(
        stopped.allowed,
        Allowed {
            start: true,
            start_break: true,
            ..NOTHING
        }
    );

    rig.keeper()
        .start(WorkTarget {
            project_id: project_id(1),
            task_id: Some(TASK),
        })
        .unwrap();
    rig.work(90);
    let running = view(&mut rig, &catalog);
    assert_eq!(running.status, TimerStatus::Running);
    assert_eq!(
        running.work,
        Some(WorkView {
            project_id: project_id(1),
            project_name: Some("Website".to_owned()),
            task_id: Some(TASK),
            task_name: Some("Checkout".to_owned()),
        })
    );
    assert_eq!(running.today, Duration::from_secs(90));
    assert_eq!(
        running.allowed,
        Allowed {
            switch: true,
            stop: true,
            start_break: true,
            ..NOTHING
        }
    );

    rig.keeper().start_break(LUNCH).unwrap();
    rig.away(60);
    let on_break = view(&mut rig, &catalog);
    assert_eq!(
        on_break.status,
        TimerStatus::OnBreak {
            policy: Some("Lunch".to_owned())
        }
    );
    assert_eq!(
        on_break.work.unwrap().project_id,
        project_id(1),
        "resumes on it"
    );
    assert_eq!(
        on_break.today,
        Duration::from_secs(90),
        "breaks aren't tracked"
    );
    assert_eq!(
        on_break.allowed,
        Allowed {
            end_break: true,
            ..NOTHING
        }
    );

    rig.keeper().end_break().unwrap();
    rig.work(30);
    rig.keeper().stop().unwrap();
    assert_eq!(view(&mut rig, &catalog).today, Duration::from_mins(2));
}

#[test]
fn idle_time_counts_until_the_member_discards_it() {
    for (resolution, today) in [
        (IdleResolution::Keep, minutes(8)),
        (
            IdleResolution::Reassign {
                project_id: project_id(2),
            },
            minutes(8),
        ),
        (IdleResolution::Discard, minutes(2)),
        (IdleResolution::Stop, minutes(2)),
    ] {
        let mut rig = Rig::new(policy(KeepIdle::Prompt));
        let catalog = catalog();
        rig.start();
        // 14:03 to 14:05 working; idle is detected at 14:10, from 14:05.
        rig.work(120);
        rig.away(300);
        rig.away(60);
        let idle = view(&mut rig, &catalog);
        assert_eq!(idle.status, TimerStatus::Idle);
        assert_eq!(
            idle.question,
            Some(Question::Idle {
                since_wall_ms: START_WALL_MS + 120_000,
                idle_for: Some(minutes(6)),
                paused: false,
            })
        );
        assert_eq!(idle.today, minutes(8), "pending idle time counts");
        assert_eq!(idle.allowed, NOTHING, "the question comes first");

        rig.resolve(resolution);
        assert_eq!(view(&mut rig, &catalog).today, today, "{resolution:?}");
        // Later time counts as usual, and the decision stays made.
        if resolution != IdleResolution::Stop {
            rig.work(60);
            assert_eq!(view(&mut rig, &catalog).today, today + minutes(1));
        }
    }
}

#[test]
fn a_long_sleep_asks_to_resume() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let catalog = catalog();
    rig.start();
    rig.work(60);
    rig.sleep_and_wake(Duration::from_hours(2));
    let asleep = view(&mut rig, &catalog);
    assert_eq!(asleep.status, TimerStatus::Paused(PauseReason::Sleep));
    assert_eq!(asleep.question, Some(Question::Resume));
    assert_eq!(asleep.today, minutes(1), "sleep is never tracked");
    assert_eq!(
        asleep.allowed,
        Allowed {
            stop: true,
            ..NOTHING
        }
    );
    rig.keeper().resume().unwrap();
    assert_eq!(view(&mut rig, &catalog).status, TimerStatus::Running);
}

#[test]
fn idle_before_a_sleep_is_asked_about_with_its_length() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let catalog = catalog();
    rig.start();
    rig.work(60);
    rig.away(120);
    rig.sleep_and_wake(Duration::from_hours(1));
    let asked = view(&mut rig, &catalog);
    let Some(Question::Idle {
        since_wall_ms,
        idle_for,
        paused,
    }) = asked.question
    else {
        panic!("expected the idle question, got {asked:?}");
    };
    assert_eq!(since_wall_ms, START_WALL_MS + 60_000);
    assert!(paused);
    assert!(
        idle_for.unwrap() >= Duration::from_mins(62),
        "the sleep is part of it"
    );
    assert_eq!(
        asked.today,
        minutes(3),
        "the idle minutes before the sleep are pending"
    );
    rig.resolve(IdleResolution::Discard);
    assert_eq!(view(&mut rig, &catalog).today, minutes(1));
}

#[test]
fn today_starts_at_the_day_start_and_survives_a_restart() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let catalog = catalog();
    rig.start();
    // 14:03 to 14:23: three windows.
    rig.work(20 * 60);
    rig.restart(policy(KeepIdle::Prompt));
    let restarted = timer_view(rig.keeper(), &catalog, MIDNIGHT);
    assert!(restarted.today + Duration::from_secs(30) >= minutes(20));
    // A day that began at 14:10 counts the windows from 14:10 on.
    let later_day = timer_view(rig.keeper(), &catalog, MIDNIGHT + (14 * 60 + 10) * 60_000);
    assert!(later_day.today < restarted.today);
    assert!(later_day.today + Duration::from_secs(30) >= minutes(13));
}

#[test]
fn without_a_catalog_nothing_can_start() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    let empty = Catalog::default();
    assert_eq!(view(&mut rig, &empty).allowed, NOTHING);
    // A timer already running on a project the catalog no longer lists can still stop.
    rig.start();
    let running = view(&mut rig, &empty);
    assert_eq!(running.work.unwrap().project_name, None);
    assert_eq!(
        running.allowed,
        Allowed {
            stop: true,
            ..NOTHING
        }
    );
}
