//! The window in front on X11 (E12-S04): `_NET_ACTIVE_WINDOW`, which EWMH window managers set
//! on the root window, and the application it belongs to (see `linux::desktop`).
//!
//! - A 0 in `_NET_ACTIVE_WINDOW` means nothing is in front (the desktop), and so does no value
//!   yet from a window manager whose `_NET_SUPPORTED` lists it (Openbox sets it only once a
//!   window is first activated). A window manager that doesn't list it doesn't follow EWMH: the
//!   reading is unsupported, never "nothing in front".
//! - The app comes from the toolkit's app ID, `WM_CLASS` and the process's executable. The
//!   process is found through X-Resource, the server's own record, rather than `_NET_WM_PID`,
//!   which the client sets itself. Nothing from `WM_CLASS`'s instance part, or any path, is
//!   kept.
//! - A window's app is looked up once and kept while its `WM_CLASS` stays the same: the class is
//!   read on every reading, since the server reuses the IDs of closed windows. When no entry
//!   matches, the entries are read again, at most every five minutes, for apps installed since.
//!   Without an entry the app is named by its class, and without a class by its executable.
//! - The title (`_NET_WM_NAME`, else `WM_NAME`, decoded by its type) is read only when asked
//!   for.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use x11rb::connection::RequestConnection;
use x11rb::protocol::res::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

use super::{connect, failed, lock};
use crate::linux::desktop::{App, DesktopFiles, Hints};
use crate::{ActiveWindow, ForegroundApp, SensorError};

/// Windows whose app is kept; beyond it the cache starts again.
const MAX_CACHED: usize = 512;
/// How often the entries may be read again for a window no entry matched.
const RELOAD_EVERY: Duration = Duration::from_mins(5);
/// The most of a property read, in 4-byte units: titles and IDs are far shorter.
const MAX_PROPERTY: u32 = 1_024;

/// The atoms the reader asks about, interned once.
struct Atoms {
    net_active_window: u32,
    net_supported: u32,
    net_wm_name: u32,
    gtk_application_id: u32,
    kde_desktop_file: u32,
}

struct Apps {
    files: DesktopFiles,
    dirs: Vec<PathBuf>,
    loaded: Instant,
    /// Each window's app, with the `WM_CLASS` it had.
    by_window: HashMap<Window, (Vec<u8>, App)>,
}

/// The window in front, on X11. See the module documentation.
pub struct X11Window {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    apps: Mutex<Apps>,
}

impl X11Window {
    /// Connects to the display named by `DISPLAY`, finding apps in the XDG applications
    /// folders.
    ///
    /// # Errors
    ///
    /// [`SensorError::Failed`] when there is no X server to connect to.
    pub fn connect() -> Result<Self, SensorError> {
        Self::connect_with(DesktopFiles::default_dirs())
    }

    /// As [`X11Window::connect`], finding apps in `application_dirs`, in precedence order.
    ///
    /// # Errors
    ///
    /// As [`X11Window::connect`].
    pub fn connect_with(application_dirs: Vec<PathBuf>) -> Result<Self, SensorError> {
        let (conn, root) = connect()?;
        let intern = |name: &str| -> Result<u32, SensorError> {
            Ok(conn
                .intern_atom(false, name.as_bytes())
                .map_err(failed)?
                .reply()
                .map_err(failed)?
                .atom)
        };
        let atoms = Atoms {
            net_active_window: intern("_NET_ACTIVE_WINDOW")?,
            net_supported: intern("_NET_SUPPORTED")?,
            net_wm_name: intern("_NET_WM_NAME")?,
            gtk_application_id: intern("_GTK_APPLICATION_ID")?,
            kde_desktop_file: intern("_KDE_NET_WM_DESKTOP_FILE")?,
        };
        let files = DesktopFiles::load(&application_dirs);
        Ok(Self {
            conn,
            root,
            atoms,
            apps: Mutex::new(Apps {
                files,
                dirs: application_dirs,
                loaded: Instant::now(),
                by_window: HashMap::new(),
            }),
        })
    }

    /// A property's type and bytes, or `None` when the window doesn't have it. `kind` may be
    /// `AnyPropertyType`.
    fn property(
        &self,
        window: Window,
        property: impl Into<u32>,
        kind: impl Into<u32>,
    ) -> Result<Option<(u32, Vec<u8>)>, SensorError> {
        let reply = self
            .conn
            .get_property(false, window, property, kind, 0, MAX_PROPERTY)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        Ok((reply.type_ != u32::from(AtomEnum::NONE)).then_some((reply.type_, reply.value)))
    }

    /// A text property of any type: `STRING` is Latin-1, each byte its code point; the others
    /// (`UTF8_STRING`, `COMPOUND_TEXT`) are read as UTF-8.
    fn text(
        &self,
        window: Window,
        property: impl Into<u32>,
    ) -> Result<Option<String>, SensorError> {
        let text = self
            .property(window, property, AtomEnum::ANY)?
            .map(|(kind, bytes)| {
                if kind == u32::from(AtomEnum::STRING) {
                    bytes.iter().map(|&byte| char::from(byte)).collect()
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                }
            });
        Ok(text
            .map(|text| text.trim_end_matches('\0').to_owned())
            .filter(|text| !text.is_empty()))
    }

    /// The window's `WM_CLASS` bytes: instance and class, each ending in a NUL.
    fn class(&self, window: Window) -> Result<Vec<u8>, SensorError> {
        Ok(self
            .property(window, AtomEnum::WM_CLASS, AtomEnum::STRING)?
            .map(|(_, bytes)| bytes)
            .unwrap_or_default())
    }

    /// The application of a window whose `WM_CLASS` is `class`: from the entries, else named by
    /// its class, else by its executable.
    fn identify(&self, window: Window, class: &[u8]) -> Result<Option<App>, SensorError> {
        let app_id = match self.text(window, self.atoms.gtk_application_id)? {
            Some(id) => Some(id),
            None => self.text(window, self.atoms.kde_desktop_file)?,
        };
        let mut parts = class
            .split(|byte| *byte == 0)
            .map(|part| String::from_utf8_lossy(part).into_owned());
        let instance = parts.next().filter(|part| !part.is_empty());
        let class = parts.next().filter(|part| !part.is_empty());
        let program = self.program(window);
        let hints = Hints {
            app_id: app_id.as_deref(),
            class: class.as_deref(),
            instance: instance.as_deref(),
            program: program.as_deref(),
        };
        let mut apps = lock(&self.apps);
        let mut app = apps.files.resolve(&hints);
        if app.is_none() && apps.loaded.elapsed() >= RELOAD_EVERY {
            apps.files = DesktopFiles::load(&apps.dirs);
            apps.loaded = Instant::now();
            app = apps.files.resolve(&hints);
        }
        drop(apps);
        // Without an entry, the class is the best name there is, then the executable. The
        // instance is never used: it can hold a path.
        Ok(app
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
            }))
    }

    /// The file name of the window's process's executable, from the server's record of the
    /// client, when the process runs on this machine.
    fn program(&self, window: Window) -> Option<String> {
        self.conn
            .extension_information(res::X11_EXTENSION_NAME)
            .ok()??;
        let spec = res::ClientIdSpec {
            client: window,
            mask: res::ClientIdMask::LOCAL_CLIENT_PID,
        };
        let reply = self.conn.res_query_client_ids(&[spec]).ok()?.reply().ok()?;
        let pid = *reply.ids.first()?.value.first()?;
        let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        Some(exe.file_name()?.to_string_lossy().into_owned())
    }

    /// Whether the window manager says it maintains `_NET_ACTIVE_WINDOW`.
    fn supports_active_window(&self) -> Result<bool, SensorError> {
        let supported = self
            .property(self.root, self.atoms.net_supported, AtomEnum::ATOM)?
            .map(|(_, bytes)| bytes)
            .unwrap_or_default();
        Ok(supported
            .chunks_exact(4)
            .filter_map(|atom| atom.try_into().ok())
            .map(u32::from_ne_bytes)
            .any(|atom| atom == self.atoms.net_active_window))
    }

    fn title(&self, window: Window) -> Result<Option<String>, SensorError> {
        match self.text(window, self.atoms.net_wm_name)? {
            Some(title) => Ok(Some(title)),
            None => self.text(window, AtomEnum::WM_NAME),
        }
    }
}

impl ActiveWindow for X11Window {
    fn foreground(&self, with_title: bool) -> Result<Option<ForegroundApp>, SensorError> {
        let Some((_, active)) =
            self.property(self.root, self.atoms.net_active_window, AtomEnum::WINDOW)?
        else {
            return if self.supports_active_window()? {
                Ok(None)
            } else {
                // No EWMH window manager: which window is in front can't be known.
                Err(SensorError::Unsupported)
            };
        };
        // Format-32 data arrives in this client's byte order.
        let window = active
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .map_or(0, u32::from_ne_bytes);
        if window == 0 {
            return Ok(None);
        }
        let class = self.class(window)?;
        let cached = lock(&self.apps)
            .by_window
            .get(&window)
            .filter(|(known, _)| *known == class)
            .map(|(_, app)| app.clone());
        let app = if let Some(app) = cached {
            app
        } else {
            let Some(app) = self.identify(window, &class)? else {
                return Err(SensorError::Failed(
                    "the window in front names no application".to_owned(),
                ));
            };
            let mut apps = lock(&self.apps);
            if apps.by_window.len() >= MAX_CACHED {
                apps.by_window.clear();
            }
            apps.by_window.insert(window, (class, app.clone()));
            app
        };
        let title = if with_title {
            self.title(window)?
        } else {
            None
        };
        Ok(Some(ForegroundApp {
            identifier: app.identifier,
            name: app.name,
            title,
        }))
    }
}

impl fmt::Debug for X11Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("X11Window").finish_non_exhaustive()
    }
}
