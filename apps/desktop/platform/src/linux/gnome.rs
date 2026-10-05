//! The window in front on GNOME under Wayland (E13-S05), from the Worklume GNOME Shell extension
//! (`src-tauri/linux/gnome-extension`), which the Linux packages install. Wayland gives apps no
//! way to know which window is in front, and GNOME Shell's own interface for it answers only
//! its allowlisted callers.
//!
//! - **One call a reading:** `com.worklume.ActiveWindow1.Get(with_title)` gives the Shell's
//!   app ID for the window (a desktop-file ID, when the Shell knows one), its Wayland app ID or
//!   X11 class, its process ID and, only when asked for, its title. Empty values mean nothing
//!   is in front.
//! - **The app** is resolved from the desktop entries as on X11 (`linux::desktop`): the Shell's
//!   app ID, then the window's app ID or class, then the executable from `/proc`. Without an
//!   entry, the class names the app, then the executable.
//! - **When the extension isn't running** the reading is `Unsupported`. GNOME Shell finds a newly
//!   installed extension at the member's next login. The reader then turns it on, once per run
//!   of the agent, as the Extensions app would; if the member turns it off again, it stays off.
//!   Until GNOME Shell answers (it may still be starting), it asks again every 30 seconds.
//!   [`GnomeWindow::extension`] says where the extension stands; the capability report
//!   (`CapabilityProbe`) explains a reading that isn't there with it.
//! - The extension answers only the installed agent: anything else reads `PermissionDenied`.
//! - Each reading is one D-Bus call with a 250 ms timeout. A failed call reconnects on the next.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use dbus::arg::{PropMap, RefArg};
use dbus::blocking::Connection;

use super::bus::{ABSENT, Bus};
use crate::linux::desktop::{App, DesktopFiles, Hints};
use crate::{
    ActiveWindow, Capability, CapabilityProbe, CapabilityReason, CapabilityStatus, ForegroundApp,
    SensorError,
};

/// The extension's UUID, as GNOME Shell knows it.
pub const EXTENSION_UUID: &str = "active-window@worklume.com";

const SERVICE: &str = "com.worklume.ActiveWindow";
const PATH: &str = "/com/worklume/ActiveWindow";
const INTERFACE: &str = "com.worklume.ActiveWindow1";
const SHELL: &str = "org.gnome.Shell";
const SHELL_PATH: &str = "/org/gnome/Shell";
const EXTENSIONS: &str = "org.gnome.Shell.Extensions";
/// How long a reading may wait. A tick must not stall behind a busy Shell.
const TIMEOUT: Duration = Duration::from_millis(250);
/// How long asking about the extension, or turning it on, may wait: once, during a reading.
const ENABLE_TIMEOUT: Duration = Duration::from_secs(1);
/// How often to ask GNOME Shell about the extension again while it doesn't answer.
const RETRY_EVERY: Duration = Duration::from_secs(30);
/// How long the extension may take to answer once GNOME Shell turned it on: the Shell reports
/// the new state, and the extension takes its bus name, a moment after saying yes.
const STARTING_WITHIN: Duration = Duration::from_secs(3);
/// How often to ask again meanwhile.
const STARTING_POLL: Duration = Duration::from_millis(100);
/// How often the entries may be read again for an app no entry matched.
const RELOAD_EVERY: Duration = Duration::from_mins(5);
/// Apps kept; beyond it the cache starts again.
const MAX_CACHED: usize = 512;

/// Where the extension stands, as GNOME Shell says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionState {
    /// Loaded and enabled.
    Enabled,
    /// Known to the Shell but turned off.
    Disabled,
    /// Installed since the Shell started: it loads at the next login.
    NotLoaded,
    /// Known but broken, out of date for this Shell, or blocked by the desktop's settings.
    Failed,
}

/// The window in front, from the Worklume GNOME Shell extension. See the module documentation.
pub struct GnomeWindow {
    bus: Mutex<Bus>,
    apps: Mutex<Apps>,
    enabling: Mutex<Enabling>,
}

/// Turning the extension on, once per run.
#[derive(Default)]
struct Enabling {
    done: bool,
    tried: Option<Instant>,
    /// When GNOME Shell said it turned the extension on.
    enabled: Option<Instant>,
}

struct Apps {
    files: DesktopFiles,
    dirs: Vec<PathBuf>,
    loaded: Instant,
    /// Each app found, by what the extension said and the executable.
    known: HashMap<(String, String, Option<String>), App>,
}

impl GnomeWindow {
    /// Asks the extension on the session bus, with apps from the XDG desktop entries. Nothing
    /// connects until the first reading.
    #[must_use]
    pub fn new() -> Self {
        Self::with_dirs(DesktopFiles::default_dirs())
    }

    /// As [`GnomeWindow::new`], with apps from the entries in `application_dirs`.
    #[must_use]
    pub fn with_dirs(application_dirs: Vec<PathBuf>) -> Self {
        Self {
            bus: Mutex::new(Bus::new(Box::new(Connection::new_session))),
            apps: Mutex::new(Apps {
                files: DesktopFiles::load(&application_dirs),
                dirs: application_dirs,
                loaded: Instant::now(),
                known: HashMap::new(),
            }),
            enabling: Mutex::new(Enabling::default()),
        }
    }

    /// Where the extension stands.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] without GNOME Shell; [`SensorError::Failed`] when the Shell
    /// doesn't answer.
    pub fn extension(&self) -> Result<ExtensionState, SensorError> {
        let (info,): (PropMap,) = lock(&self.bus)
            .call(|conn| {
                conn.with_proxy(SHELL, SHELL_PATH, ENABLE_TIMEOUT)
                    .method_call(EXTENSIONS, "GetExtensionInfo", (EXTENSION_UUID,))
            })
            .map_err(|error| sensor_error(&error))?;
        // GNOME Shell's ExtensionState: 1 enabled, 2 disabled, 6 initialized (loaded, never
        // enabled); an empty answer means it doesn't know the extension.
        let state = info.get("state").and_then(|state| state.0.as_f64());
        Ok(match state {
            None => ExtensionState::NotLoaded,
            Some(state) if (state - 1.0).abs() < f64::EPSILON => ExtensionState::Enabled,
            Some(state)
                if (state - 2.0).abs() < f64::EPSILON || (state - 6.0).abs() < f64::EPSILON =>
            {
                ExtensionState::Disabled
            }
            Some(_) => ExtensionState::Failed,
        })
    }

    /// Turns the extension on for the member, as the Extensions app does. It works only once
    /// GNOME Shell knows the extension, from the login after it was installed.
    ///
    /// # Errors
    ///
    /// [`SensorError::Failed`] when the Shell refuses or doesn't answer.
    pub fn enable_extension(&self) -> Result<(), SensorError> {
        let (enabled,): (bool,) = lock(&self.bus)
            .call(|conn| {
                conn.with_proxy(SHELL, SHELL_PATH, ENABLE_TIMEOUT)
                    .method_call(EXTENSIONS, "EnableExtension", (EXTENSION_UUID,))
            })
            .map_err(|error| sensor_error(&error))?;
        if enabled {
            Ok(())
        } else {
            Err(SensorError::Failed(
                "GNOME Shell didn't turn the Worklume extension on".to_owned(),
            ))
        }
    }

    /// Turns the extension on if GNOME Shell knows it and it is off: once per run.
    fn turn_on_once(&self) {
        let mut enabling = lock(&self.enabling);
        if enabling.done || enabling.tried.is_some_and(|at| at.elapsed() < RETRY_EVERY) {
            return;
        }
        enabling.tried = Some(Instant::now());
        match self.extension() {
            Ok(ExtensionState::Disabled) => {
                enabling.done = true;
                // A refusal shows as the extension still not answering.
                if self.enable_extension().is_ok() {
                    enabling.enabled = Some(Instant::now());
                }
            }
            Ok(_) => enabling.done = true,
            // GNOME Shell may still be starting.
            Err(_) => {}
        }
    }

    /// The app of a window the extension described.
    fn identify(&self, shell_id: &str, wm_class: &str, pid: u32) -> Option<App> {
        let program = (pid > 0)
            .then(|| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
            .flatten()
            .and_then(|exe| Some(exe.file_name()?.to_string_lossy().into_owned()));
        let key = (shell_id.to_owned(), wm_class.to_owned(), program.clone());
        let mut apps = lock(&self.apps);
        if let Some(app) = apps.known.get(&key) {
            return Some(app.clone());
        }
        let nonempty = |text: &str| (!text.is_empty()).then(|| text.to_owned());
        let (shell_id, wm_class) = (nonempty(shell_id), nonempty(wm_class));
        // On Wayland the window's app ID is meant to be its desktop-file ID.
        let hints = Hints {
            app_id: shell_id.as_deref().or(wm_class.as_deref()),
            class: wm_class.as_deref(),
            instance: None,
            program: program.as_deref(),
        };
        let mut app = apps.files.resolve(&hints);
        if app.is_none() && apps.loaded.elapsed() >= RELOAD_EVERY {
            apps.files = DesktopFiles::load(&apps.dirs);
            apps.loaded = Instant::now();
            app = apps.files.resolve(&hints);
        }
        let app = app
            .or_else(|| {
                wm_class.map(|class| App {
                    identifier: class.to_lowercase(),
                    name: class,
                })
            })
            .or_else(|| {
                program.map(|program| App {
                    identifier: program.to_lowercase(),
                    name: program,
                })
            })?;
        if apps.known.len() >= MAX_CACHED {
            apps.known.clear();
        }
        apps.known.insert(key, app.clone());
        Some(app)
    }
}

impl Default for GnomeWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveWindow for GnomeWindow {
    fn foreground(&self, with_title: bool) -> Result<Option<ForegroundApp>, SensorError> {
        let answer: Result<(String, String, u32, String), _> = lock(&self.bus).call(|conn| {
            conn.with_proxy(SERVICE, PATH, TIMEOUT)
                .method_call(INTERFACE, "Get", (with_title,))
        });
        let (shell_id, wm_class, pid, title) = answer.map_err(|error| {
            let error = sensor_error(&error);
            if error == SensorError::Unsupported {
                // The bus is free again here.
                self.turn_on_once();
            }
            error
        })?;
        if shell_id.is_empty() && wm_class.is_empty() && pid == 0 {
            return Ok(None);
        }
        let Some(app) = self.identify(&shell_id, &wm_class, pid) else {
            return Err(SensorError::Failed(
                "the window in front names no application".to_owned(),
            ));
        };
        Ok(Some(ForegroundApp {
            identifier: app.identifier,
            name: app.name,
            title: (with_title && !title.is_empty()).then_some(title),
        }))
    }
}

impl CapabilityProbe for GnomeWindow {
    /// Reads the app in front, without its title; when the extension doesn't answer, asks
    /// GNOME Shell why. Reading may turn the extension on, so a reading that failed is tried
    /// again once the Shell says it is on.
    fn capability(&self) -> Capability {
        match self.foreground(false) {
            Err(SensorError::Unsupported) => {}
            reading => return reading_capability(&reading),
        }
        // Just turned on, by this reading or a recent one: give it a moment to start.
        let enabled = lock(&self.enabling).enabled;
        if let Some(enabled) = enabled {
            while enabled.elapsed() < STARTING_WITHIN {
                match self.foreground(false) {
                    Err(SensorError::Unsupported) => std::thread::sleep(STARTING_POLL),
                    reading => return reading_capability(&reading),
                }
            }
        }
        match self.extension() {
            Ok(ExtensionState::Enabled) => match self.foreground(false) {
                Err(SensorError::Unsupported) => extension_capability(&Err(SensorError::Failed(
                    "enabled but not answering".to_owned(),
                ))),
                reading => reading_capability(&reading),
            },
            state => extension_capability(&state),
        }
    }
}

/// A reading's capability, the extension having answered.
fn reading_capability(reading: &Result<Option<ForegroundApp>, SensorError>) -> Capability {
    use CapabilityReason as Why;
    use CapabilityStatus as Status;
    match reading {
        Ok(_) => Capability::granted(),
        // The extension answers only the installed agent.
        Err(SensorError::PermissionDenied) => {
            Capability::limited(Status::Denied, Why::PermissionDenied, &[])
        }
        Err(SensorError::Unsupported) => {
            Capability::limited(Status::Unsupported, Why::DesktopUnsupported, &[])
        }
        Err(SensorError::Failed(_)) => Capability::limited(Status::Failed, Why::AdapterFailed, &[]),
    }
}

/// The capability while the extension doesn't answer, from where GNOME Shell says it stands.
fn extension_capability(state: &Result<ExtensionState, SensorError>) -> Capability {
    use CapabilityReason as Why;
    use CapabilityStatus as Status;
    match state {
        Ok(ExtensionState::NotLoaded) => {
            Capability::limited(Status::Pending, Why::GnomeExtensionNextLogin, &[])
        }
        Ok(ExtensionState::Disabled) => {
            Capability::limited(Status::Denied, Why::GnomeExtensionDisabled, &[])
        }
        // Enabled yet silent counts as broken.
        Ok(ExtensionState::Failed | ExtensionState::Enabled) | Err(SensorError::Failed(_)) => {
            Capability::limited(Status::Failed, Why::GnomeExtensionFailed, &[])
        }
        // No GNOME Shell: GNOME without it (Flashback), or it left.
        Err(SensorError::Unsupported) => {
            Capability::limited(Status::Unsupported, Why::DesktopUnsupported, &[])
        }
        Err(SensorError::PermissionDenied) => {
            Capability::limited(Status::Denied, Why::PermissionDenied, &[])
        }
    }
}

impl fmt::Debug for GnomeWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GnomeWindow").finish_non_exhaustive()
    }
}

/// The extension not running reads as unsupported, refusing as a denied permission.
fn sensor_error(error: &dbus::Error) -> SensorError {
    match error.name() {
        Some(name) if ABSENT.contains(&name) => SensorError::Unsupported,
        Some("org.freedesktop.DBus.Error.AccessDenied") => SensorError::PermissionDenied,
        _ => SensorError::Failed(format!(
            "asking the Worklume GNOME extension: {}",
            error.message().unwrap_or("unknown error")
        )),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{ExtensionState, extension_capability, reading_capability};
    use crate::{CapabilityReason as Why, CapabilityStatus as Status, SensorError};

    #[test]
    fn the_extension_state_explains_a_missing_reading() {
        for (state, status, reason) in [
            (
                Ok(ExtensionState::NotLoaded),
                Status::Pending,
                Why::GnomeExtensionNextLogin,
            ),
            (
                Ok(ExtensionState::Disabled),
                Status::Denied,
                Why::GnomeExtensionDisabled,
            ),
            (
                Ok(ExtensionState::Failed),
                Status::Failed,
                Why::GnomeExtensionFailed,
            ),
            (
                Ok(ExtensionState::Enabled),
                Status::Failed,
                Why::GnomeExtensionFailed,
            ),
            (
                Err(SensorError::Unsupported),
                Status::Unsupported,
                Why::DesktopUnsupported,
            ),
            (
                Err(SensorError::Failed("timeout".to_owned())),
                Status::Failed,
                Why::GnomeExtensionFailed,
            ),
            (
                Err(SensorError::PermissionDenied),
                Status::Denied,
                Why::PermissionDenied,
            ),
        ] {
            let capability = extension_capability(&state);
            assert_eq!(
                (capability.status(), capability.reasons()),
                (status, &[reason][..]),
                "{state:?}"
            );
        }
    }

    #[test]
    fn a_reading_is_granted_even_with_nothing_in_front() {
        assert_eq!(reading_capability(&Ok(None)).status(), Status::Granted);
        assert_eq!(
            reading_capability(&Err(SensorError::PermissionDenied)).reasons(),
            [Why::PermissionDenied]
        );
        assert_eq!(
            reading_capability(&Err(SensorError::Failed("bus".to_owned()))).reasons(),
            [Why::AdapterFailed]
        );
    }
}
