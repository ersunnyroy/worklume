//! Which installed app a Wayland window belongs to, from what the compositor says of it: shared
//! by the GNOME extension's reader and KDE's Plasma window reader. Resolved from the desktop
//! entries as on X11 (`super::desktop`): the compositor's app ID, then the window's Wayland app
//! ID or X11 class, then the executable from `/proc`. Without an entry, the class names the
//! app, then the executable. Each app found is kept.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::desktop::{App, DesktopFiles, Hints};

/// How often the entries may be read again for an app no entry matched.
const RELOAD_EVERY: Duration = Duration::from_mins(5);
/// Apps kept; beyond it the cache starts again.
const MAX_CACHED: usize = 512;

/// Apps from the desktop entries in some folders. See the module documentation.
pub(crate) struct AppResolver {
    apps: Mutex<Apps>,
}

struct Apps {
    files: DesktopFiles,
    dirs: Vec<PathBuf>,
    loaded: Instant,
    /// Each app found, by what the compositor said and the executable.
    known: HashMap<(String, String, Option<String>), App>,
}

impl AppResolver {
    /// Apps from the entries in `application_dirs`.
    pub(crate) fn new(application_dirs: Vec<PathBuf>) -> Self {
        Self {
            apps: Mutex::new(Apps {
                files: DesktopFiles::load(&application_dirs),
                dirs: application_dirs,
                loaded: Instant::now(),
                known: HashMap::new(),
            }),
        }
    }

    /// The app of a window: `app_id` is the compositor's ID for it (a desktop-file ID when it
    /// knows one), `class` its Wayland app ID or X11 class, `pid` its process (0: unknown).
    pub(crate) fn identify(&self, app_id: &str, class: &str, pid: u32) -> Option<App> {
        let program = (pid > 0)
            .then(|| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
            .flatten()
            .and_then(|exe| Some(exe.file_name()?.to_string_lossy().into_owned()));
        let key = (app_id.to_owned(), class.to_owned(), program.clone());
        let mut apps = lock(&self.apps);
        if let Some(app) = apps.known.get(&key) {
            return Some(app.clone());
        }
        let nonempty = |text: &str| (!text.is_empty()).then(|| text.to_owned());
        let (app_id, class) = (nonempty(app_id), nonempty(class));
        // On Wayland the window's app ID is meant to be its desktop-file ID.
        let hints = Hints {
            app_id: app_id.as_deref().or(class.as_deref()),
            class: class.as_deref(),
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
                class.map(|class| App {
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
