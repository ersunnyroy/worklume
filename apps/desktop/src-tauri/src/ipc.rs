//! Commands the webview may call, and the event it may listen to.
//!
//! Every command is listed three times, and Tauri's ACL refuses anything missing from any of
//! them: in [`builder`] (registration and TypeScript bindings), in `build.rs` (the app manifest)
//! and in `capabilities/main.json` (the permission granted to the main window).
//!
//! The timer commands (E09-S09) take IDs as text. Each is parsed and then checked against the
//! catalog by the agent: the webview is a client like any other.

#![allow(
    clippy::needless_pass_by_value,
    reason = "Tauri hands command arguments, the managed state included, over by value"
)]

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Runtime, State};
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use uuid::Uuid;
use worklume_core::catalog::{Catalog, CatalogError};
use worklume_core::tracker::PauseReason;
use worklume_core::view::{Question, TimerStatus, TimerView};
use worklume_platform::{LinuxSession, OperatingSystem, PlatformInfo};

use crate::agent::{Agent, AgentError, AgentState, IdleAnswer, Unavailable};

/// Registers the commands and events; also used to generate `frontend/src/ipc/bindings.gen.ts`.
#[must_use]
pub fn builder<R: Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            agent_status,
            agent_state,
            work_catalog,
            timer_start,
            timer_switch,
            timer_stop,
            timer_resume,
            break_start,
            break_end,
            idle_answer,
        ])
        .events(collect_events![AgentChanged])
}

/// What the agent is and where it runs, for the UI's about section and support requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AgentStatus {
    pub version: String,
    pub platform: Platform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Platform {
    pub os: Os,
    /// Present on Linux only.
    pub linux_session: Option<DisplaySession>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DisplaySession {
    X11,
    Wayland,
    Unknown,
}

impl From<PlatformInfo> for Platform {
    fn from(info: PlatformInfo) -> Self {
        Self {
            os: match info.os {
                OperatingSystem::MacOs => Os::Macos,
                OperatingSystem::Windows => Os::Windows,
                OperatingSystem::Linux => Os::Linux,
            },
            linux_session: info.linux_session.map(|session| match session {
                LinuxSession::X11 => DisplaySession::X11,
                LinuxSession::Wayland => DisplaySession::Wayland,
                LinuxSession::Unknown => DisplaySession::Unknown,
            }),
        }
    }
}

/// The agent's version and platform.
#[tauri::command]
#[specta::specta]
#[must_use]
pub fn agent_status() -> AgentStatus {
    AgentStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        platform: PlatformInfo::current().into(),
    }
}

// --- The timer (E09-S09) ------------------------------------------------------------------------

/// What the agent shows: starting, unable to track, or the timer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentStateDto {
    Starting,
    Unavailable { reason: UnavailableReason },
    Ready { timer: TimerViewDto },
}

/// Why the agent can't track time on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    NoDataFolder,
    CredentialStore,
    /// Another agent process still holds the store.
    InUse,
    /// The store was written by a newer version; this one must be updated.
    Outdated,
    Storage,
    Clock,
}

/// The timer as the member sees it. The UI shows it and decides nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct TimerViewDto {
    pub status: TimerStatusDto,
    /// What the timer runs on, or resumes on after a pause or a break.
    pub work: Option<WorkDto>,
    /// Tracked since the start of today in the device's time zone. Idle time awaiting an
    /// answer counts until it is discarded.
    pub today_seconds: u32,
    pub question: Option<QuestionDto>,
    pub allowed: AllowedDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimerStatusDto {
    Stopped,
    Running,
    /// Idle was detected; the member is asked what to do with the time.
    Idle,
    Paused {
        reason: PauseReasonDto,
    },
    OnBreak {
        policy_name: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PauseReasonDto {
    Sleep,
    SessionLocked,
}

/// A project and task, with names when the catalog knows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WorkDto {
    pub project_id: String,
    pub project_name: Option<String>,
    pub task_id: Option<String>,
    pub task_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestionDto {
    /// Idle time awaits an answer: keep, discard, reassign or stop. When `paused`, the device
    /// slept or the session was locked meanwhile, and any answer but stop also resumes the
    /// timer.
    Idle {
        /// Where inactivity began, in milliseconds since the Unix epoch.
        since_ms: f64,
        /// How long ago that was, sleep included; unknown after a reboot.
        idle_seconds: Option<u32>,
        paused: bool,
    },
    /// The device slept, or the session was locked, longer than the idle timeout: resume, or
    /// stop.
    Resume,
}

/// What the member may do now. Answering a question is allowed exactly while it is asked.
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per action the UI offers, nothing to combine"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AllowedDto {
    pub start: bool,
    pub switch: bool,
    pub stop: bool,
    pub start_break: bool,
    pub end_break: bool,
}

/// The projects and tasks the member may track, and the organization's break policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CatalogDto {
    pub projects: Vec<ProjectDto>,
    pub break_policies: Vec<NamedDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub tasks: Vec<NamedDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct NamedDto {
    pub id: String,
    pub name: String,
}

/// The member's answer to the idle question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IdleAnswerDto {
    Keep,
    Discard,
    /// Move the idle time to another project from the catalog.
    Reassign {
        project_id: String,
    },
    /// Discard the idle time and stop the timer where it began.
    Stop,
}

/// Why a timer command did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CommandError {
    /// The agent can't track time on this device.
    Unavailable,
    /// Not possible in the timer's current state.
    NotNow,
    /// The project isn't one the member may track.
    UnknownProject,
    /// The task doesn't belong to the project.
    UnknownTask,
    /// The break policy isn't one of the organization's.
    UnknownBreakPolicy,
    /// An ID isn't a UUID.
    InvalidId,
    /// Recording failed; the agent's log has the detail.
    Failed,
}

/// Sent whenever what the agent shows changes, at most once a second.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Event)]
pub struct AgentChanged(pub AgentStateDto);

impl From<AgentState> for AgentStateDto {
    fn from(state: AgentState) -> Self {
        match state {
            AgentState::Starting => Self::Starting,
            AgentState::Unavailable(reason) => Self::Unavailable {
                reason: match reason {
                    Unavailable::NoDataFolder => UnavailableReason::NoDataFolder,
                    Unavailable::CredentialStore => UnavailableReason::CredentialStore,
                    Unavailable::InUse => UnavailableReason::InUse,
                    Unavailable::Outdated => UnavailableReason::Outdated,
                    Unavailable::Storage => UnavailableReason::Storage,
                    Unavailable::Clock => UnavailableReason::Clock,
                },
            },
            AgentState::Ready(view) => Self::Ready { timer: view.into() },
        }
    }
}

impl From<TimerView> for TimerViewDto {
    fn from(view: TimerView) -> Self {
        Self {
            status: match view.status {
                TimerStatus::Stopped => TimerStatusDto::Stopped,
                TimerStatus::Running => TimerStatusDto::Running,
                TimerStatus::Idle => TimerStatusDto::Idle,
                TimerStatus::Paused(reason) => TimerStatusDto::Paused {
                    reason: match reason {
                        PauseReason::Sleep => PauseReasonDto::Sleep,
                        PauseReason::SessionLocked => PauseReasonDto::SessionLocked,
                    },
                },
                TimerStatus::OnBreak { policy } => TimerStatusDto::OnBreak {
                    policy_name: policy,
                },
            },
            work: view.work.map(|work| WorkDto {
                project_id: work.project_id.to_string(),
                project_name: work.project_name,
                task_id: work.task_id.map(|id| id.to_string()),
                task_name: work.task_name,
            }),
            today_seconds: seconds(view.today),
            question: view.question.map(|question| match question {
                Question::Idle {
                    since_wall_ms,
                    idle_for,
                    paused,
                } => QuestionDto::Idle {
                    since_ms: millis_f64(since_wall_ms),
                    idle_seconds: idle_for.map(seconds),
                    paused,
                },
                Question::Resume => QuestionDto::Resume,
            }),
            allowed: AllowedDto {
                start: view.allowed.start,
                switch: view.allowed.switch,
                stop: view.allowed.stop,
                start_break: view.allowed.start_break,
                end_break: view.allowed.end_break,
            },
        }
    }
}

impl From<&Catalog> for CatalogDto {
    fn from(catalog: &Catalog) -> Self {
        Self {
            projects: catalog
                .projects
                .iter()
                .map(|project| ProjectDto {
                    id: project.id.to_string(),
                    name: project.name.clone(),
                    tasks: project
                        .tasks
                        .iter()
                        .map(|task| NamedDto {
                            id: task.id.to_string(),
                            name: task.name.clone(),
                        })
                        .collect(),
                })
                .collect(),
            break_policies: catalog
                .break_policies
                .iter()
                .map(|policy| NamedDto {
                    id: policy.id.to_string(),
                    name: policy.name.clone(),
                })
                .collect(),
        }
    }
}

impl From<AgentError> for CommandError {
    fn from(error: AgentError) -> Self {
        match error {
            AgentError::Unavailable => Self::Unavailable,
            AgentError::NotNow => Self::NotNow,
            AgentError::Catalog(CatalogError::UnknownProject) => Self::UnknownProject,
            AgentError::Catalog(CatalogError::UnknownTask) => Self::UnknownTask,
            AgentError::Catalog(CatalogError::UnknownBreakPolicy) => Self::UnknownBreakPolicy,
            AgentError::Failed => Self::Failed,
        }
    }
}

fn seconds(duration: std::time::Duration) -> u32 {
    u32::try_from(duration.as_secs()).unwrap_or(u32::MAX)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "wall times in milliseconds stay far below 2^53 until the year 287,396"
)]
const fn millis_f64(ms: i64) -> f64 {
    ms as f64
}

fn uuid(text: &str) -> Result<Uuid, CommandError> {
    Uuid::parse_str(text).map_err(|_| CommandError::InvalidId)
}

fn optional_uuid(text: Option<&str>) -> Result<Option<Uuid>, CommandError> {
    text.map(uuid).transpose()
}

type TimerResult = Result<TimerViewDto, CommandError>;

fn done(result: Result<TimerView, AgentError>) -> TimerResult {
    result.map(TimerViewDto::from).map_err(CommandError::from)
}

/// What the agent shows now.
#[tauri::command]
#[specta::specta]
#[must_use]
pub fn agent_state(agent: State<'_, Agent>) -> AgentStateDto {
    agent.state().into()
}

/// The projects, tasks and break policies the member may pick from.
#[tauri::command]
#[specta::specta]
#[must_use]
pub fn work_catalog(agent: State<'_, Agent>) -> CatalogDto {
    CatalogDto::from(&agent.catalog())
}

/// Starts the timer on a project from the catalog, and optionally one of its tasks.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn timer_start(
    agent: State<'_, Agent>,
    project_id: String,
    task_id: Option<String>,
) -> TimerResult {
    let (project, task) = (uuid(&project_id)?, optional_uuid(task_id.as_deref())?);
    done(agent.start(project, task))
}

/// Switches the running timer to another project or task from the catalog.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn timer_switch(
    agent: State<'_, Agent>,
    project_id: String,
    task_id: Option<String>,
) -> TimerResult {
    let (project, task) = (uuid(&project_id)?, optional_uuid(task_id.as_deref())?);
    done(agent.switch(project, task))
}

/// Stops the timer.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn timer_stop(agent: State<'_, Agent>) -> TimerResult {
    done(agent.stop())
}

/// Resumes the timer after a long sleep.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn timer_resume(agent: State<'_, Agent>) -> TimerResult {
    done(agent.resume())
}

/// Starts a break under one of the organization's break policies.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn break_start(agent: State<'_, Agent>, break_policy_id: String) -> TimerResult {
    done(agent.start_break(uuid(&break_policy_id)?))
}

/// Ends the break, resuming the timer if it ran before.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn break_end(agent: State<'_, Agent>) -> TimerResult {
    done(agent.end_break())
}

/// Answers the idle question.
///
/// # Errors
///
/// See [`CommandError`].
#[tauri::command]
#[specta::specta]
pub fn idle_answer(agent: State<'_, Agent>, answer: IdleAnswerDto) -> TimerResult {
    let answer = match answer {
        IdleAnswerDto::Keep => IdleAnswer::Keep,
        IdleAnswerDto::Discard => IdleAnswer::Discard,
        IdleAnswerDto::Stop => IdleAnswer::Stop,
        IdleAnswerDto::Reassign { project_id } => IdleAnswer::Reassign {
            project_id: uuid(&project_id)?,
        },
    };
    done(agent.answer_idle(answer))
}
