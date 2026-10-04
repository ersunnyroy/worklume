//! The running agent (E09-S09): the encrypted store and the timekeeper, advanced once a second
//! on a background thread, with the catalog the member picks work from.
//!
//! The timer's rules live in `worklume-core`. This module serialises access to them, checks
//! every request from the UI against the catalog, and reports what the member sees. The store
//! opens on the ticking thread, so a slow or prompting credential store never holds up the
//! window; until then the agent is [`AgentState::Starting`].
//!
//! It also keeps what the device can measure ([`Agent::check_capabilities`], E13-S02), which
//! a thread of its own checks once a minute: the readers' answers may take a second.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tracing::{debug, info, warn};
use uuid::Uuid;
use worklume_core::catalog::{Catalog, CatalogError};
use worklume_core::timekeeper::{
    IdlePolicy, KeepIdle, Outcome, Sensors, Timekeeper, TimekeeperError,
};
use worklume_core::tracker::{IdleResolution, InputMethod, TrackerConfig, TrackerError};
use worklume_core::view::{TimerView, timer_view};
use worklume_platform::{
    CapabilityReport, Clock, DeviceSensors, OsClock, OsSecretStore, SecretStore,
};
use worklume_storage::{StorageError, Store};

use crate::diagnostics::APP_IDENTIFIER;

/// How often the timekeeper is advanced.
pub const TICK: Duration = Duration::from_secs(1);

/// Gives the start of the member's current day, in UTC milliseconds.
pub type DayStart = Box<dyn Fn() -> i64 + Send + Sync>;

/// What the agent can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    /// The store is opening.
    Starting,
    /// The agent cannot track time on this device, and why.
    Unavailable(Unavailable),
    /// The timer.
    Ready(TimerView),
}

/// Why the agent cannot track time. Each is logged with its detail when it happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// The OS gives no folder for the agent's data.
    NoDataFolder,
    /// The OS credential store is locked, missing or unreachable.
    CredentialStore,
    /// Another process holds the store: an agent that hasn't exited yet.
    InUse,
    /// The store was written by a newer version of the agent.
    Outdated,
    /// The encrypted store or the timer's saved state could not be opened.
    Storage,
    /// The device's clock could not be read.
    Clock,
}

/// A refused or failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error("the agent cannot track time on this device")]
    Unavailable,
    #[error("that is not possible in the timer's current state")]
    NotNow,
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("the request failed; details are in the log")]
    Failed,
}

/// The member's answer to the idle question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAnswer {
    Keep,
    Discard,
    Reassign { project_id: Uuid },
    Stop,
}

/// What a tick found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticked {
    pub state: AgentState,
    /// The agent began asking the member something, so its window should come forward.
    pub new_question: bool,
}

enum Inner {
    Starting,
    Unavailable(Unavailable),
    Ready(Box<Running>),
}

struct Running {
    keeper: Timekeeper,
    catalog: Catalog,
    /// The last tick failed; logged once until one succeeds.
    failing: bool,
}

/// Reports what the device can measure now (`DeviceSensors::report`).
pub type CapabilitySource = Arc<dyn Fn() -> CapabilityReport + Send + Sync>;

/// The agent. Shared between the IPC commands and the ticking thread.
pub struct Agent {
    inner: Mutex<Inner>,
    day_start: DayStart,
    /// Why the last attempt to open failed, so a retry that fails the same way isn't logged
    /// again.
    open_failure: Mutex<Option<String>>,
    /// The sensors' report, once they started.
    capability_source: Mutex<Option<CapabilitySource>>,
    /// The last report, for the window and, with sync (E09-S05), for the server.
    capabilities: Mutex<Option<CapabilityReport>>,
}

/// Why opening failed: what the member is told, and what the log says.
struct OpenFailure {
    reason: Unavailable,
    detail: String,
}

impl OpenFailure {
    fn new(reason: Unavailable, context: &str, error: impl std::fmt::Display) -> Self {
        Self {
            reason,
            detail: format!("{context}: {error}"),
        }
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent").finish_non_exhaustive()
    }
}

impl Agent {
    /// An agent whose store hasn't opened yet.
    #[must_use]
    pub fn new(day_start: DayStart) -> Self {
        Self {
            inner: Mutex::new(Inner::Starting),
            day_start,
            open_failure: Mutex::new(None),
            capability_source: Mutex::new(None),
            capabilities: Mutex::new(None),
        }
    }

    /// Where reports of what the device can measure come from: this device's sensors once
    /// they start, or a stand-in in tests.
    pub fn set_capability_source(&self, source: CapabilitySource) {
        *guard(&self.capability_source) = Some(source);
    }

    /// Asks the sensors what the device can measure now, and keeps the answer. Returns it when
    /// it differs from the last one (the first one included), for the window, the log and the
    /// server; `None` when unchanged or when no sensors started yet. It may take a second or
    /// more (GNOME Shell, the portal), so it never runs on the ticking thread.
    #[must_use]
    pub fn check_capabilities(&self) -> Option<CapabilityReport> {
        let source = guard(&self.capability_source).clone()?;
        // Asked without any lock held: the window's commands must not wait for the OS.
        let report = source();
        let mut last = guard(&self.capabilities);
        if last.as_ref() == Some(&report) {
            return None;
        }
        *last = Some(report.clone());
        Some(report)
    }

    /// The last report of what the device can measure, if one was made.
    #[must_use]
    pub fn capabilities(&self) -> Option<CapabilityReport> {
        guard(&self.capabilities).clone()
    }

    /// Opens the store in `dir` and the timekeeper on `sensors`, whose input is measured as
    /// `input_method` says. The idle policy, the app and URL policy and the catalog stay empty
    /// until the organization's configuration arrives (E09-S06).
    ///
    /// # Errors
    ///
    /// Returns why the agent cannot track, after logging the detail; it stays unavailable.
    pub fn open(
        &self,
        dir: &Path,
        secrets: &dyn SecretStore,
        sensors: Sensors,
        input_method: InputMethod,
    ) -> Result<(), Unavailable> {
        let result = open_keeper(dir, secrets, sensors, input_method);
        self.settle(result.map(|keeper| {
            Inner::Ready(Box::new(Running {
                keeper,
                catalog: Catalog::default(),
                failing: false,
            }))
        }))
    }

    /// Opens the store in the agent's data folder, on this device's clock, credential store and
    /// sensors (`worklume_platform::DeviceSensors`). Sensors without an adapter on this OS yet
    /// read as unsupported.
    ///
    /// # Errors
    ///
    /// As [`Agent::open`].
    pub fn open_on_this_device(&self) -> Result<(), Unavailable> {
        match this_device() {
            Ok((dir, secrets, device, input_method)) => {
                let sensors = Sensors {
                    clock: Arc::clone(&device.clock),
                    idle: Arc::clone(&device.sensors.idle),
                    window: Arc::clone(&device.sensors.window),
                    browser: Arc::clone(&device.sensors.browser),
                    input: Arc::clone(&device.sensors.input),
                    spans: Arc::clone(&device.sensors.spans),
                    lock: Arc::clone(&device.sensors.lock),
                };
                // The newest sensors report, also while the store can't open: what the device
                // can measure doesn't depend on it.
                self.set_capability_source(Arc::new(move || device.sensors.report()));
                self.open(&dir, &*secrets, sensors, input_method)
            }
            Err(failure) => self.settle(Err(failure)),
        }
    }

    /// Takes the outcome of an attempt to open. A failure is logged as a warning the first
    /// time it happens, and quietly when a retry fails the same way.
    fn settle(&self, outcome: Result<Inner, OpenFailure>) -> Result<(), Unavailable> {
        let mut last = self
            .open_failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (inner, result) = match outcome {
            Ok(inner) => {
                if last.take().is_some() {
                    info!("the agent opened its store and can track time");
                }
                (inner, Ok(()))
            }
            Err(failure) => {
                if last.as_deref() == Some(failure.detail.as_str()) {
                    debug!(detail = %failure.detail, "the agent still can't open its store");
                } else {
                    warn!(detail = %failure.detail, "the agent can't track time");
                }
                *last = Some(failure.detail);
                (Inner::Unavailable(failure.reason), Err(failure.reason))
            }
        };
        *self.lock() = inner;
        result
    }

    /// What the agent shows now.
    #[must_use]
    pub fn state(&self) -> AgentState {
        let day_start = (self.day_start)();
        match &*self.lock() {
            Inner::Starting => AgentState::Starting,
            Inner::Unavailable(reason) => AgentState::Unavailable(*reason),
            Inner::Ready(running) => AgentState::Ready(running.view(day_start)),
        }
    }

    /// The catalog the member picks work from.
    #[must_use]
    pub fn catalog(&self) -> Catalog {
        match &*self.lock() {
            Inner::Ready(running) => running.catalog.clone(),
            _ => Catalog::default(),
        }
    }

    /// Replaces the catalog, as the configuration does (E09-S06).
    ///
    /// # Errors
    ///
    /// Fails while the agent is unavailable.
    pub fn set_catalog(&self, catalog: Catalog) -> Result<(), AgentError> {
        match &mut *self.lock() {
            Inner::Ready(running) => {
                running.catalog = catalog;
                Ok(())
            }
            _ => Err(AgentError::Unavailable),
        }
    }

    /// Replaces the idle policy, as the configuration does (E09-S06).
    ///
    /// # Errors
    ///
    /// Fails while the agent is unavailable.
    pub fn set_idle_policy(&self, policy: IdlePolicy) -> Result<(), AgentError> {
        match &mut *self.lock() {
            Inner::Ready(running) => {
                running.keeper.set_policy(policy);
                Ok(())
            }
            _ => Err(AgentError::Unavailable),
        }
    }

    /// Advances the timekeeper to now. A failure is logged once and retried next tick, which
    /// catches up.
    #[must_use]
    pub fn tick(&self) -> Ticked {
        let day_start = (self.day_start)();
        let mut inner = self.lock();
        let Inner::Ready(running) = &mut *inner else {
            drop(inner);
            return Ticked {
                state: self.state(),
                new_question: false,
            };
        };
        let new_question = match running.keeper.tick_measured() {
            Ok(outcome) => {
                if running.failing {
                    info!("the timer is being recorded again");
                    running.failing = false;
                }
                outcome.new_prompt.is_some()
            }
            Err(error) => {
                if !running.failing {
                    warn!(%error, "recording the timer failed; retrying every tick");
                    running.failing = true;
                }
                false
            }
        };
        Ticked {
            state: AgentState::Ready(running.view(day_start)),
            new_question,
        }
    }

    /// Starts the timer on a project, and optionally one of its tasks, from the catalog.
    ///
    /// # Errors
    ///
    /// Fails when the target isn't in the catalog, the timer isn't stopped, or recording fails.
    pub fn start(&self, project_id: Uuid, task_id: Option<Uuid>) -> Result<TimerView, AgentError> {
        self.act(|running| {
            let target = running.catalog.target(project_id, task_id)?;
            running.keeper.start(target).map_err(refused)
        })
    }

    /// Switches the running timer to another project or task from the catalog.
    ///
    /// # Errors
    ///
    /// As [`Agent::start`], but the timer must be running.
    pub fn switch(&self, project_id: Uuid, task_id: Option<Uuid>) -> Result<TimerView, AgentError> {
        self.act(|running| {
            let target = running.catalog.target(project_id, task_id)?;
            running.keeper.switch(target).map_err(refused)
        })
    }

    /// Stops the timer.
    ///
    /// # Errors
    ///
    /// Fails while a question is pending or on a break, or when recording fails.
    pub fn stop(&self) -> Result<TimerView, AgentError> {
        self.act(|running| running.keeper.stop().map_err(refused))
    }

    /// Resumes the timer after a long sleep.
    ///
    /// # Errors
    ///
    /// Fails unless the timer is paused with no idle question pending, or when recording fails.
    pub fn resume(&self) -> Result<TimerView, AgentError> {
        self.act(|running| running.keeper.resume().map_err(refused))
    }

    /// Starts a break under one of the organization's break policies.
    ///
    /// # Errors
    ///
    /// Fails when the policy isn't in the catalog, the timer is paused or already on a break,
    /// or recording fails.
    pub fn start_break(&self, break_policy_id: Uuid) -> Result<TimerView, AgentError> {
        self.act(|running| {
            let policy = running.catalog.break_policy(break_policy_id)?.id;
            running.keeper.start_break(policy).map_err(refused)
        })
    }

    /// Ends the break.
    ///
    /// # Errors
    ///
    /// Fails unless on a break, or when recording fails.
    pub fn end_break(&self) -> Result<TimerView, AgentError> {
        self.act(|running| running.keeper.end_break().map_err(refused))
    }

    /// The member's answer to the idle question.
    ///
    /// # Errors
    ///
    /// Fails when a reassignment names a project outside the catalog, no idle question is
    /// being asked, or recording fails.
    pub fn answer_idle(&self, answer: IdleAnswer) -> Result<TimerView, AgentError> {
        self.act(|running| {
            let resolution = match answer {
                IdleAnswer::Keep => IdleResolution::Keep,
                IdleAnswer::Discard => IdleResolution::Discard,
                IdleAnswer::Stop => IdleResolution::Stop,
                IdleAnswer::Reassign { project_id } => IdleResolution::Reassign {
                    project_id: running.catalog.reassign_target(project_id)?,
                },
            };
            running.keeper.resolve_idle(resolution).map_err(refused)
        })
    }

    fn act(
        &self,
        action: impl FnOnce(&mut Running) -> Result<Outcome, AgentError>,
    ) -> Result<TimerView, AgentError> {
        let day_start = (self.day_start)();
        let mut inner = self.lock();
        let Inner::Ready(running) = &mut *inner else {
            return Err(AgentError::Unavailable);
        };
        action(running)?;
        Ok(running.view(day_start))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Every timekeeper call leaves it consistent, even one that panicked midway: changes
        // apply only once saved.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Running {
    fn view(&self, day_start: i64) -> TimerView {
        timer_view(&self.keeper, &self.catalog, day_start)
    }
}

/// This device's clock and started sensors.
struct Device {
    clock: Arc<dyn Clock>,
    sensors: DeviceSensors,
}

/// This device's store folder, credential store and sensors, and how its input is measured.
fn this_device() -> Result<(PathBuf, Arc<OsSecretStore>, Device, InputMethod), OpenFailure> {
    let dir = store_dir().ok_or_else(|| {
        OpenFailure::new(
            Unavailable::NoDataFolder,
            "finding the data folder",
            "the OS gave none",
        )
    })?;
    let secrets = Arc::new(OsSecretStore::new(APP_IDENTIFIER).map_err(|error| {
        OpenFailure::new(
            Unavailable::CredentialStore,
            "reaching the credential store",
            error,
        )
    })?);
    let clock: Arc<dyn Clock> = Arc::new(
        OsClock::new()
            .map_err(|error| OpenFailure::new(Unavailable::Clock, "reading the clock", error))?,
    );
    let device = DeviceSensors::start(&clock, &(Arc::clone(&secrets) as Arc<dyn SecretStore>));
    for problem in &device.problems {
        warn!(%problem, "a sensor couldn't start; the agent tracks without it");
    }
    for limit in &device.limits {
        info!(%limit, "a sensor has a limit on this desktop");
    }
    let input_method = if device.input_hooks {
        InputMethod::Hooks
    } else if device.activity_spans {
        InputMethod::Portal
    } else {
        InputMethod::IdlePoll
    };
    let device = Device {
        clock,
        sensors: device,
    };
    Ok((dir, secrets, device, input_method))
}

fn open_keeper(
    dir: &Path,
    secrets: &dyn SecretStore,
    sensors: Sensors,
    input_method: InputMethod,
) -> Result<Timekeeper, OpenFailure> {
    let (store, report) = Store::open(dir, secrets).map_err(|error| {
        let reason = match error {
            StorageError::KeyUnavailable(_) | StorageError::KeyNotPersisted => {
                Unavailable::CredentialStore
            }
            StorageError::InUse => Unavailable::InUse,
            StorageError::SchemaTooNew { .. } => Unavailable::Outdated,
            _ => Unavailable::Storage,
        };
        OpenFailure::new(reason, "opening the local store", error)
    })?;
    if let Some((reason, _)) = &report.recovered {
        warn!(
            ?reason,
            "the local store could not be read and was set aside; a new one started"
        );
    }
    let config = TrackerConfig::new(input_method);
    // Idle detection waits for the organization's policy (E09-S06).
    let policy = IdlePolicy {
        timeout: None,
        keep: KeepIdle::Prompt,
    };
    let (keeper, recovery) = Timekeeper::open(Arc::new(store), config, sensors, policy)
        .map_err(|error| OpenFailure::new(Unavailable::Storage, "restoring the timer", error))?;
    if recovery.resumed_tracking {
        info!(
            unaccounted_ms = recovery.unaccounted_ms,
            "the timer was running when the agent stopped; it carries on"
        );
    }
    Ok(keeper)
}

fn guard<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Refused as not possible now when the tracker says so; anything else failed, and is logged.
fn refused(error: TimekeeperError) -> AgentError {
    match error {
        TimekeeperError::Tracker(TrackerError::InvalidTransition { .. }) => AgentError::NotNow,
        error => {
            warn!(%error, "a timer request failed");
            AgentError::Failed
        }
    }
}

/// Where the encrypted store lives: `<local data>/<identifier>/store`, next to the logs.
#[must_use]
pub fn store_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join(APP_IDENTIFIER).join("store"))
}

/// The start of today in the device's time zone, in UTC milliseconds. Falls back to UTC
/// midnight when the time zone can't be read.
#[must_use]
pub fn local_day_start() -> i64 {
    let now = jiff::Zoned::now();
    now.start_of_day().map_or_else(
        |_| {
            let ms = now.timestamp().as_millisecond();
            ms - ms.rem_euclid(86_400_000)
        },
        |start| start.timestamp().as_millisecond(),
    )
}
