//! The agent runtime (E09-S09) on fake sensors: timer requests are checked against the catalog,
//! the timer runs through it, a question brings the window forward, and a store that can't
//! open leaves it unavailable rather than broken.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test setup failures should fail loudly"
)]

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use uuid::Uuid;
use worklume_core::catalog::{BreakPolicy, Catalog, CatalogError, Project, Task};
use worklume_core::timekeeper::{IdlePolicy, KeepIdle, Sensors};
use worklume_core::view::{Question, TimerStatus};
use worklume_desktop::agent::{Agent, AgentError, AgentState, IdleAnswer, Unavailable};
use worklume_platform::fake::{
    FakeBrowser, FakeClock, FakeIdle, FakeInput, FakeWindow, MemorySecretStore,
};

/// 2026-09-21 14:03:00 UTC.
const START_WALL_MS: i64 = 1_789_999_380_000;
const MIDNIGHT: i64 = START_WALL_MS - (14 * 60 + 3) * 60_000;
const WEBSITE: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0001);
const SUPPORT: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0002);
const CHECKOUT: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0011);
const LUNCH: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0021);
const ELSEWHERE: Uuid = Uuid::from_u128(0x0192_0000_0000_7000_8000_0000_0000_0099);

struct Device {
    dir: TempDir,
    secrets: MemorySecretStore,
    clock: Arc<FakeClock>,
    idle: Arc<FakeIdle>,
    agent: Agent,
}

impl Device {
    fn new() -> Self {
        let clock = Arc::new(FakeClock::new(START_WALL_MS));
        let device = Self {
            dir: TempDir::new().expect("temporary folder"),
            secrets: MemorySecretStore::default(),
            idle: Arc::new(FakeIdle::new(Arc::clone(&clock))),
            clock,
            agent: Agent::new(Box::new(|| MIDNIGHT)),
        };
        assert_eq!(device.agent.state(), AgentState::Starting);
        device
    }

    fn sensors(&self) -> Sensors {
        Sensors {
            clock: Arc::clone(&self.clock) as _,
            idle: Arc::clone(&self.idle) as _,
            window: Arc::new(FakeWindow::default()),
            browser: Arc::new(FakeBrowser::default()),
            input: Arc::new(FakeInput::default()),
        }
    }

    fn opened() -> Self {
        let device = Self::new();
        device
            .agent
            .open(device.dir.path(), &device.secrets, device.sensors())
            .expect("the store opens");
        device
            .agent
            .set_catalog(catalog())
            .expect("the agent is ready");
        device
    }

    /// The member works for `seconds`, the agent ticking every second.
    fn work(&self, seconds: u64) {
        for _ in 0..seconds {
            self.clock.advance(Duration::from_secs(1));
            self.idle.input();
            let _ = self.agent.tick();
        }
    }

    fn timer(&self) -> worklume_core::view::TimerView {
        match self.agent.state() {
            AgentState::Ready(timer) => timer,
            other => panic!("the agent isn't ready: {other:?}"),
        }
    }
}

fn catalog() -> Catalog {
    Catalog {
        projects: vec![
            Project {
                id: WEBSITE,
                name: "Website".to_owned(),
                tasks: vec![Task {
                    id: CHECKOUT,
                    name: "Checkout".to_owned(),
                }],
            },
            Project {
                id: SUPPORT,
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

#[test]
fn requests_outside_the_catalog_are_refused_and_change_nothing() {
    let device = Device::opened();
    let refused = [
        device.agent.start(ELSEWHERE, None),
        device.agent.start(SUPPORT, Some(CHECKOUT)),
        device.agent.start(WEBSITE, Some(ELSEWHERE)),
        device.agent.start_break(ELSEWHERE),
    ];
    assert_eq!(
        refused.map(|result| result.expect_err("refused")),
        [
            AgentError::Catalog(CatalogError::UnknownProject),
            AgentError::Catalog(CatalogError::UnknownTask),
            AgentError::Catalog(CatalogError::UnknownTask),
            AgentError::Catalog(CatalogError::UnknownBreakPolicy),
        ]
    );
    assert_eq!(device.timer().status, TimerStatus::Stopped);

    device
        .agent
        .start(WEBSITE, Some(CHECKOUT))
        .expect("a catalogued target starts");
    assert_eq!(
        device.agent.switch(ELSEWHERE, None),
        Err(AgentError::Catalog(CatalogError::UnknownProject))
    );
    assert_eq!(
        device.timer().work.expect("running").project_id,
        WEBSITE,
        "a refused switch leaves the timer where it was"
    );
}

#[test]
fn the_timer_runs_through_the_agent() {
    let device = Device::opened();
    let started = device.agent.start(WEBSITE, None).expect("starts");
    assert_eq!(started.status, TimerStatus::Running);
    device.work(90);
    assert_eq!(device.timer().today, Duration::from_secs(90));

    let switched = device.agent.switch(SUPPORT, None).expect("switches");
    assert_eq!(
        switched.work.expect("running").project_name.as_deref(),
        Some("Support")
    );
    assert_eq!(device.agent.end_break(), Err(AgentError::NotNow));

    let on_break = device.agent.start_break(LUNCH).expect("breaks");
    assert_eq!(
        on_break.status,
        TimerStatus::OnBreak {
            policy: Some("Lunch".to_owned())
        }
    );
    device.work(60);
    device.agent.end_break().expect("ends the break");
    device.work(30);
    let stopped = device.agent.stop().expect("stops");
    assert_eq!(stopped.status, TimerStatus::Stopped);
    assert_eq!(
        stopped.today,
        Duration::from_mins(2),
        "the break isn't tracked"
    );
}

#[test]
fn an_idle_question_brings_the_window_forward_once() {
    let device = Device::opened();
    device
        .agent
        .set_idle_policy(IdlePolicy {
            timeout: Some(Duration::from_mins(5)),
            keep: KeepIdle::Prompt,
        })
        .expect("ready");
    device.agent.start(WEBSITE, None).expect("starts");
    device.work(60);
    let mut questions = 0;
    for _ in 0..400 {
        device.clock.advance(Duration::from_secs(1));
        if device.agent.tick().new_question {
            questions += 1;
        }
    }
    assert_eq!(questions, 1);
    let Some(Question::Idle { paused, .. }) = device.timer().question else {
        panic!("expected the idle question");
    };
    assert!(!paused);
    assert_eq!(
        device.agent.answer_idle(IdleAnswer::Reassign {
            project_id: ELSEWHERE
        }),
        Err(AgentError::Catalog(CatalogError::UnknownProject)),
        "idle time moves only to a catalogued project"
    );
    let answered = device
        .agent
        .answer_idle(IdleAnswer::Discard)
        .expect("answers");
    assert_eq!(answered.question, None);
    assert_eq!(answered.today, Duration::from_mins(1));
}

#[test]
fn a_store_that_cannot_open_leaves_the_agent_unavailable() {
    let device = Device::new();
    device.secrets.set_unavailable(true);
    assert_eq!(
        device
            .agent
            .open(device.dir.path(), &device.secrets, device.sensors()),
        Err(Unavailable::CredentialStore)
    );
    assert_eq!(
        device.agent.state(),
        AgentState::Unavailable(Unavailable::CredentialStore)
    );
    assert_eq!(
        device.agent.start(WEBSITE, None),
        Err(AgentError::Unavailable)
    );
    assert_eq!(
        device.agent.set_catalog(catalog()),
        Err(AgentError::Unavailable)
    );
    assert!(!device.agent.tick().new_question);

    // It opens once the keychain is back.
    device.secrets.set_unavailable(false);
    device
        .agent
        .open(device.dir.path(), &device.secrets, device.sensors())
        .expect("the store opens");
    assert!(matches!(device.agent.state(), AgentState::Ready(_)));
}

#[test]
fn a_second_agent_cannot_open_the_same_store() {
    let first = Device::opened();
    let second = Agent::new(Box::new(|| MIDNIGHT));
    assert_eq!(
        second.open(first.dir.path(), &first.secrets, first.sensors()),
        Err(Unavailable::InUse)
    );
}

#[test]
fn the_timer_carries_on_after_the_agent_restarts() {
    let device = Device::opened();
    device.agent.start(WEBSITE, Some(CHECKOUT)).expect("starts");
    device.work(45);
    let restarted = Agent::new(Box::new(|| MIDNIGHT));
    let sensors = device.sensors();
    drop(device.agent);
    restarted
        .open(device.dir.path(), &device.secrets, sensors)
        .expect("reopens");
    restarted.set_catalog(catalog()).expect("ready");
    let AgentState::Ready(timer) = restarted.state() else {
        panic!("not ready");
    };
    assert_eq!(timer.status, TimerStatus::Running);
    assert_eq!(
        timer.work.expect("running").task_name.as_deref(),
        Some("Checkout")
    );
    assert!(
        timer.today >= Duration::from_secs(30),
        "saved at the 30-second heartbeat"
    );
}
