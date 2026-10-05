//! The window in front on KDE Plasma under Wayland (E13-S06), from KWin's Plasma window
//! management protocol (`org_kde_plasma_window_management`), as the spike recommended
//! (`docs/spikes/E13-S01-wayland.md`).
//!
//! - **The grant.** KWin offers the protocol only to a client whose executable has a desktop
//!   entry listing it in `X-KDE-Wayland-Interfaces`; the Linux packages install one for
//!   `/usr/bin/worklume-desktop`. Without it the global is missing and connecting reads
//!   [`SensorError::Unsupported`]; on KDE the sensors report that as a refused permission.
//! - **No polling.** KWin announces each window and sends its app ID, title, process ID and state
//!   as they change; a reader thread of its own keeps them, and a reading only looks up the one
//!   flagged active. Nothing active (the desktop, the lock screen) reads as nothing in front.
//! - **The app** is resolved from the desktop entries as on GNOME (`super::apps`): KWin's app ID
//!   for a window is its desktop-file name when it knows one, else its resource class.
//! - **Titles.** The protocol pushes every window's title, so the reader holds them in memory,
//!   as the compositor does; they are never logged, and a reading returns the active window's
//!   title only when asked for.
//! - A compositor that goes away is reported on the next reading, as `Failed`.

use std::collections::HashMap;
use std::fmt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_plasma::plasma_window_management::client::{
    org_kde_plasma_window, org_kde_plasma_window_management,
};

use super::apps::AppResolver;
use super::desktop::DesktopFiles;
use super::wayland::pump;
use crate::{ActiveWindow, ForegroundApp, SensorError};

/// The newest version used: 16 is KWin 5.27's. Later versions change nothing read here.
const MAX_VERSION: u32 = 16;
/// The first version that names windows by UUID.
const UUIDS_SINCE: u32 = 13;
/// `org_kde_plasma_window_management.state.active`.
const ACTIVE: u32 = 0x1;
/// Windows kept; a compositor announcing more is misbehaving, and the oldest are dropped.
const MAX_WINDOWS: usize = 4096;

/// The window in front, from KWin. See the module documentation.
pub struct PlasmaWindow {
    shared: Arc<Shared>,
    apps: AppResolver,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    windows: Mutex<Windows>,
    /// Why the reader stopped, once it has.
    failure: Mutex<Option<String>>,
    stop: AtomicBool,
}

#[derive(Default)]
struct Windows {
    /// Each mapped window, by its protocol object.
    known: HashMap<u32, Window>,
    /// The window flagged active, if any.
    active: Option<u32>,
}

#[derive(Default, Clone)]
struct Window {
    app_id: String,
    title: String,
    pid: u32,
}

/// The reader thread's state, which the event handlers see.
struct Reader {
    shared: Arc<Shared>,
    /// The manager's version: from 13, windows come by UUID.
    version: u32,
}

impl PlasmaWindow {
    /// Connects to the compositor named by `WAYLAND_DISPLAY`, with apps from the XDG desktop
    /// entries, and starts following its windows.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] when the compositor doesn't offer the protocol to this
    /// executable (not KWin, or no grant); [`SensorError::Failed`] without a compositor.
    pub fn connect() -> Result<Self, SensorError> {
        let conn = Connection::connect_to_env().map_err(|error| {
            SensorError::Failed(format!("connecting to the Wayland compositor: {error}"))
        })?;
        Self::start(&conn, DesktopFiles::default_dirs())
    }

    /// As [`PlasmaWindow::connect`], to the compositor listening at `socket`, with apps from the
    /// entries in `application_dirs`.
    ///
    /// # Errors
    ///
    /// As [`PlasmaWindow::connect`].
    pub fn connect_to(socket: &Path, application_dirs: Vec<PathBuf>) -> Result<Self, SensorError> {
        let stream = UnixStream::connect(socket).map_err(failed)?;
        let conn = Connection::from_socket(stream).map_err(failed)?;
        Self::start(&conn, application_dirs)
    }

    fn start(conn: &Connection, application_dirs: Vec<PathBuf>) -> Result<Self, SensorError> {
        let (globals, mut queue) = registry_queue_init::<Reader>(conn).map_err(failed)?;
        let qh = queue.handle();
        let manager: org_kde_plasma_window_management::OrgKdePlasmaWindowManagement = globals
            .bind(&qh, 1..=MAX_VERSION, ())
            .map_err(|_| SensorError::Unsupported)?;
        let shared = Arc::new(Shared::default());
        let mut reader = Reader {
            shared: Arc::clone(&shared),
            version: manager.version(),
        };
        // The windows already mapped, then their state.
        queue.roundtrip(&mut reader).map_err(failed)?;
        queue.roundtrip(&mut reader).map_err(failed)?;
        let thread = std::thread::Builder::new()
            .name("worklume-plasma-windows".to_owned())
            .spawn({
                let conn = conn.clone();
                move || {
                    let stop = Arc::clone(&reader.shared);
                    let result = pump(&conn, &mut queue, &mut reader, &stop.stop);
                    drop(conn.flush());
                    if let Err(error) = result {
                        *lock(&reader.shared.failure) = Some(error);
                    }
                    drop(manager);
                }
            })
            .map_err(failed)?;
        Ok(Self {
            shared,
            apps: AppResolver::new(application_dirs),
            thread: Some(thread),
        })
    }
}

impl ActiveWindow for PlasmaWindow {
    fn foreground(&self, with_title: bool) -> Result<Option<ForegroundApp>, SensorError> {
        if let Some(error) = lock(&self.shared.failure).clone() {
            return Err(SensorError::Failed(error));
        }
        let front = {
            let windows = lock(&self.shared.windows);
            windows
                .active
                .and_then(|active| windows.known.get(&active).cloned())
        };
        let Some(window) = front else {
            return Ok(None);
        };
        if window.app_id.is_empty() && window.pid == 0 {
            // Announced, but its details haven't arrived yet.
            return Ok(None);
        }
        let Some(app) = self
            .apps
            .identify(&window.app_id, &window.app_id, window.pid)
        else {
            return Err(SensorError::Failed(
                "the window in front names no application".to_owned(),
            ));
        };
        Ok(Some(ForegroundApp {
            identifier: app.identifier,
            name: app.name,
            title: (with_title && !window.title.is_empty()).then_some(window.title),
        }))
    }
}

impl Drop for PlasmaWindow {
    fn drop(&mut self) {
        self.shared
            .stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // The reader wakes within half a second and sees the flag.
            drop(thread.join());
        }
    }
}

impl fmt::Debug for PlasmaWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the titles.
        f.debug_struct("PlasmaWindow")
            .field("windows", &lock(&self.shared.windows).known.len())
            .finish_non_exhaustive()
    }
}

impl Dispatch<org_kde_plasma_window_management::OrgKdePlasmaWindowManagement, ()> for Reader {
    fn event(
        reader: &mut Self,
        manager: &org_kde_plasma_window_management::OrgKdePlasmaWindowManagement,
        event: org_kde_plasma_window_management::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        use org_kde_plasma_window_management::Event;
        let window = match event {
            Event::WindowWithUuid { uuid, .. } => manager.get_window_by_uuid(uuid, qh, ()),
            // Before version 13 windows come by number; from it, both events come.
            Event::Window { id } if reader.version < UUIDS_SINCE => manager.get_window(id, qh, ()),
            _ => return,
        };
        let mut windows = lock(&reader.shared.windows);
        if windows.known.len() >= MAX_WINDOWS {
            windows.known.clear();
            windows.active = None;
        }
        windows
            .known
            .insert(window.id().protocol_id(), Window::default());
    }
}

impl Dispatch<org_kde_plasma_window::OrgKdePlasmaWindow, ()> for Reader {
    fn event(
        reader: &mut Self,
        window: &org_kde_plasma_window::OrgKdePlasmaWindow,
        event: org_kde_plasma_window::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use org_kde_plasma_window::Event;
        let id = window.id().protocol_id();
        let mut windows = lock(&reader.shared.windows);
        if let Event::Unmapped = event {
            windows.known.remove(&id);
            if windows.active == Some(id) {
                windows.active = None;
            }
            drop(windows);
            if window.version() >= 4 {
                window.destroy();
            }
            return;
        }
        if let Event::StateChanged { flags } = event {
            if flags & ACTIVE != 0 {
                windows.active = Some(id);
            } else if windows.active == Some(id) {
                windows.active = None;
            }
            return;
        }
        let Some(known) = windows.known.get_mut(&id) else {
            return;
        };
        match event {
            Event::AppIdChanged { app_id } => known.app_id = app_id,
            Event::TitleChanged { title } => known.title = title,
            Event::PidChanged { pid } => known.pid = pid,
            _ => {}
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Reader {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn failed(error: impl fmt::Display) -> SensorError {
    SensorError::Failed(error.to_string())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
