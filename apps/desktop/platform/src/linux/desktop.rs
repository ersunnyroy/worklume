//! Which installed application a window belongs to (E12-S04), from the XDG desktop entries. On
//! Linux an app's stable identifier is its desktop-file ID (`org.gnome.Nautilus`,
//! `firefox`, `debian-xterm`), and its `Name` is the name people know it by.
//!
//! Entries come from `$XDG_DATA_HOME/applications` (by default `~/.local/share/applications`),
//! then each `$XDG_DATA_DIRS/applications` (by default `/usr/local/share` and `/usr/share`), then
//! the Flatpak and Snap export folders. When two folders hold the same ID, the earlier one wins,
//! as the XDG specification says, even when it is `Hidden`: a hidden entry deletes the app.
//!
//! **Matching a window**, strongest first:
//!
//! 1. The app ID the toolkit announces (`_GTK_APPLICATION_ID`, `_KDE_NET_WM_DESKTOP_FILE`).
//! 2. An entry whose `StartupWMClass` is the window's class or instance, ignoring case.
//! 3. An entry whose ID is the class or instance in lower case (`firefox`).
//! 4. The one entry whose `Exec` runs the window's executable. Launchers shared by many entries
//!    (`python3`, `flatpak`) match nothing.
//!
//! The window's instance name is used only to match: it can hold a path (Chromium puts its
//! profile folder there), so it is never kept.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Desktop entries larger than this aren't real ones.
const MAX_ENTRY_BYTES: u64 = 64 * 1024;
/// How deep subfolders of an applications folder are searched.
const MAX_DEPTH: usize = 3;

/// An application, as the member knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub identifier: String,
    pub name: String,
}

/// What a window says about its application.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hints<'a> {
    /// The ID its toolkit announces.
    pub app_id: Option<&'a str>,
    /// `WM_CLASS`'s class part.
    pub class: Option<&'a str>,
    /// `WM_CLASS`'s instance part: for matching only.
    pub instance: Option<&'a str>,
    /// The file name of its process's executable.
    pub program: Option<&'a str>,
}

#[derive(Debug, Clone)]
struct Entry {
    app: App,
    hidden: bool,
    wm_class: Option<String>,
    program: Option<String>,
}

/// The installed applications. See the module documentation.
#[derive(Debug, Default)]
pub struct DesktopFiles {
    /// In precedence order, one per ID.
    entries: Vec<Entry>,
    by_id: HashMap<String, usize>,
}

impl DesktopFiles {
    /// The applications folders, in precedence order, from the XDG variables.
    #[must_use]
    pub fn default_dirs() -> Vec<PathBuf> {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let data_home = var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| Path::new(&home).join(".local/share")));
        let data_dirs =
            var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        data_home
            .into_iter()
            .chain(
                data_dirs
                    .split(':')
                    .filter(|dir| !dir.is_empty())
                    .map(PathBuf::from),
            )
            .chain([
                PathBuf::from("/var/lib/flatpak/exports/share"),
                PathBuf::from("/var/lib/snapd/desktop"),
            ])
            .map(|dir| dir.join("applications"))
            .collect()
    }

    /// Reads every entry in `dirs`, earlier folders first. Unreadable files and folders are
    /// skipped.
    #[must_use]
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut files = Self::default();
        for dir in dirs {
            let mut found = Vec::new();
            collect(dir, dir, 0, &mut found);
            found.sort();
            for (id, path) in found {
                if files.by_id.contains_key(&id) {
                    continue;
                }
                let Some(entry) = read_entry(&id, &path) else {
                    continue;
                };
                files.by_id.insert(id, files.entries.len());
                files.entries.push(entry);
            }
        }
        files
    }

    /// The application a window with `hints` belongs to, if an entry says so.
    #[must_use]
    pub fn resolve(&self, hints: &Hints<'_>) -> Option<App> {
        let visible = |index: &usize| {
            let entry = &self.entries[*index];
            (!entry.hidden).then(|| entry.app.clone())
        };
        let names = || [hints.class, hints.instance].into_iter().flatten();
        if let Some(app) = hints
            .app_id
            .and_then(|id| self.by_id.get(id.trim_end_matches(".desktop")))
            .and_then(visible)
        {
            return Some(app);
        }
        if let Some(app) = self.entries.iter().find(|entry| {
            !entry.hidden
                && entry
                    .wm_class
                    .as_deref()
                    .is_some_and(|wm_class| names().any(|name| wm_class.eq_ignore_ascii_case(name)))
        }) {
            return Some(app.app.clone());
        }
        if let Some(app) = names()
            .find_map(|name| self.by_id.get(&name.to_lowercase()))
            .and_then(visible)
        {
            return Some(app);
        }
        let program = hints.program?;
        let mut running = self
            .entries
            .iter()
            .filter(|entry| entry.program.as_deref() == Some(program));
        match (running.next(), running.next()) {
            (Some(entry), None) if !entry.hidden => Some(entry.app.clone()),
            _ => None,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Finds `*.desktop` files under `dir`, with their IDs: the path below `root`, `/` read as `-`.
fn collect(root: &Path, dir: &Path, depth: usize, found: &mut Vec<(String, PathBuf)>) {
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    for item in listing.flatten() {
        let path = item.path();
        let Ok(kind) = item.file_type() else { continue };
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                collect(root, &path, depth + 1, found);
            }
        } else if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let id = relative
                .with_extension("")
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("-");
            found.push((id, path));
        }
    }
}

fn read_entry(id: &str, path: &Path) -> Option<Entry> {
    if fs::metadata(path).ok()?.len() > MAX_ENTRY_BYTES {
        return None;
    }
    parse(id, &fs::read_to_string(path).ok()?)
}

/// The `[Desktop Entry]` group of an application's entry; `None` for anything else.
fn parse(id: &str, text: &str) -> Option<Entry> {
    let mut in_entry = false;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            // Localized keys (`Name[fr]`) are left out: the name is kept once, untranslated.
            fields.entry(key.trim()).or_insert_with(|| value.trim());
        }
    }
    let hidden = fields.get("Hidden") == Some(&"true");
    if !hidden && fields.get("Type") != Some(&"Application") {
        return None;
    }
    Some(Entry {
        app: App {
            identifier: id.to_owned(),
            name: fields
                .get("Name")
                .filter(|name| !name.is_empty())
                .map_or_else(|| id.to_owned(), |name| (*name).to_owned()),
        },
        hidden,
        wm_class: fields
            .get("StartupWMClass")
            .filter(|class| !class.is_empty())
            .map(|class| (*class).to_owned()),
        program: fields.get("Exec").and_then(|exec| program(exec)),
    })
}

/// The file name of the program an `Exec` line runs, past `env` and its assignments. Shared
/// launchers name no program.
fn program(exec: &str) -> Option<String> {
    let words = words(exec);
    let mut words = words.iter().map(String::as_str);
    let mut first = words.next()?;
    if file_name(first) == "env" {
        first = words.find(|word| !word.contains('=') && !word.starts_with('-'))?;
    }
    let name = file_name(first);
    let shared = [
        "flatpak", "snap", "sh", "bash", "python", "python3", "java", "perl", "wine", "xdg-open",
    ];
    (!name.is_empty() && !shared.contains(&name) && !name.starts_with("python3."))
        .then(|| name.to_owned())
}

/// An `Exec` line's words: split on spaces outside double quotes, with `\` escaping inside them.
fn words(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut escaped) = (false, false);
    for c in exec.chars() {
        if escaped {
            word.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{App, DesktopFiles, Hints, parse, program};

    fn app(identifier: &str, name: &str) -> App {
        App {
            identifier: identifier.to_owned(),
            name: name.to_owned(),
        }
    }

    /// Two applications folders in a fresh temporary folder: the user's, then the system's.
    fn folders(test: &str, user: &[(&str, &str)], system: &[(&str, &str)]) -> Vec<PathBuf> {
        let root =
            std::env::temp_dir().join(format!("worklume-desktop-{test}-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        let dirs = vec![root.join("user"), root.join("system")];
        for (dir, entries) in dirs.iter().zip([user, system]) {
            for (path, text) in entries {
                let path = dir.join(path);
                fs::create_dir_all(path.parent().unwrap_or(dir)).expect("folder");
                fs::write(path, text).expect("entry");
            }
        }
        dirs
    }

    const XTERM: &str =
        "[Desktop Entry]\nName=XTerm\nExec=xterm\nType=Application\nStartupWMClass=XTerm\n";
    const CALC: &str = "[Desktop Entry]\nType=Application\nExec=libreoffice --calc %U\nName=LibreOffice Calc\nName[fr]=LibreOffice Calc (fr)\nStartupWMClass=libreoffice-calc\n\n[Desktop Action NewDocument]\nName=New Spreadsheet\nExec=libreoffice --calc\n";

    #[test]
    fn reads_the_desktop_entry_group_only() {
        let entry = parse("libreoffice-calc", CALC).expect("an application");
        assert_eq!(entry.app, app("libreoffice-calc", "LibreOffice Calc"));
        assert_eq!(entry.wm_class.as_deref(), Some("libreoffice-calc"));
        assert_eq!(entry.program.as_deref(), Some("libreoffice"));
        assert!(parse("link", "[Desktop Entry]\nType=Link\nName=Docs\n").is_none());
        assert!(parse("empty", "").is_none());
        // Keys in other groups, before or after, say nothing about the app.
        let text = "[Other]\nName=Before\n[Desktop Entry]\nType=Application\nExec=tool\n\
                    [Desktop Action Open]\nName=Open\nStartupWMClass=Evil\n";
        let entry = parse("tool", text).expect("an application");
        assert_eq!(entry.app, app("tool", "tool"));
        assert_eq!(entry.wm_class, None);
    }

    #[test]
    fn finds_the_program_an_exec_line_runs() {
        assert_eq!(
            program("/usr/bin/code --new-window %F").as_deref(),
            Some("code")
        );
        assert_eq!(
            program("env BAMF_DESKTOP_FILE_HINT=x /snap/bin/slack %U").as_deref(),
            Some("slack")
        );
        assert_eq!(
            program("\"/opt/My App/my-app\" %u").as_deref(),
            Some("my-app")
        );
        assert_eq!(program("\"/opt/Q\\\"uote/q\" %u").as_deref(), Some("q"));
        for shared in [
            "flatpak run org.mozilla.firefox",
            "python3 tool.py",
            "python3.12",
            "sh -c x",
        ] {
            assert_eq!(program(shared), None, "{shared}");
        }
    }

    #[test]
    fn matches_a_window_strongest_hint_first() {
        let dirs = folders(
            "match",
            &[],
            &[
                ("debian-xterm.desktop", XTERM),
                ("libreoffice-calc.desktop", CALC),
                (
                    "firefox.desktop",
                    "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n",
                ),
                (
                    "org.gnome.Nautilus.desktop",
                    "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus\nStartupWMClass=XTerm\n",
                ),
                (
                    "kde/okular.desktop",
                    "[Desktop Entry]\nType=Application\nName=Okular\nExec=okular %U\n",
                ),
            ],
        );
        let files = DesktopFiles::load(&dirs);
        assert_eq!(files.len(), 5);
        // The toolkit's ID beats a class that another entry claims.
        let hints = Hints {
            app_id: Some("org.gnome.Nautilus"),
            class: Some("XTerm"),
            ..Hints::default()
        };
        assert_eq!(
            files.resolve(&hints),
            Some(app("org.gnome.Nautilus", "Files"))
        );
        // StartupWMClass, ignoring case, against the class or the instance; the first entry wins.
        let hints = Hints {
            class: Some("xterm"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), Some(app("debian-xterm", "XTerm")));
        let hints = Hints {
            class: Some("Soffice"),
            instance: Some("libreoffice-calc"),
            ..Hints::default()
        };
        assert_eq!(
            files.resolve(&hints),
            Some(app("libreoffice-calc", "LibreOffice Calc"))
        );
        // The ID from the class, and subfolders joined with dashes.
        let hints = Hints {
            class: Some("Firefox"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), Some(app("firefox", "Firefox")));
        let hints = Hints {
            instance: Some("kde-okular"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), Some(app("kde-okular", "Okular")));
        // The executable, when one entry runs it.
        let hints = Hints {
            class: Some("Unknown"),
            program: Some("okular"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), Some(app("kde-okular", "Okular")));
        // Nothing matches.
        let hints = Hints {
            class: Some("Unknown"),
            program: Some("unknown"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), None);
    }

    #[test]
    fn an_earlier_folder_wins_and_a_hidden_entry_deletes_the_app() {
        let dirs = folders(
            "precedence",
            &[
                (
                    "firefox.desktop",
                    "[Desktop Entry]\nType=Application\nName=My Firefox\nExec=firefox\n",
                ),
                ("debian-xterm.desktop", "[Desktop Entry]\nHidden=true\n"),
                ("okular.desktop", "[Desktop Entry]\nHidden=true\n"),
                (
                    "org.kde.Gwenview.desktop",
                    "[Desktop Entry]\nName=Gwenview\nHidden=true\n",
                ),
            ],
            &[
                (
                    "firefox.desktop",
                    "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox\n",
                ),
                ("debian-xterm.desktop", XTERM),
                (
                    "okular.desktop",
                    "[Desktop Entry]\nType=Application\nName=Okular\nExec=okular\n",
                ),
                (
                    "org.kde.Gwenview.desktop",
                    "[Desktop Entry]\nType=Application\nName=Gwenview\nExec=gwenview\n",
                ),
            ],
        );
        let files = DesktopFiles::load(&dirs);
        let hints = Hints {
            class: Some("firefox"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), Some(app("firefox", "My Firefox")));
        // Deleted, whichever way the window points at it: its class, its ID, the toolkit's ID.
        let hints = Hints {
            class: Some("XTerm"),
            program: Some("xterm"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), None);
        let hints = Hints {
            class: Some("okular"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), None);
        let hints = Hints {
            app_id: Some("org.kde.Gwenview"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), None);
    }

    #[test]
    fn a_program_shared_by_two_entries_matches_neither() {
        let dirs = folders(
            "shared",
            &[],
            &[
                (
                    "a.desktop",
                    "[Desktop Entry]\nType=Application\nName=A\nExec=launcher --a\n",
                ),
                (
                    "b.desktop",
                    "[Desktop Entry]\nType=Application\nName=B\nExec=launcher --b\n",
                ),
            ],
        );
        let files = DesktopFiles::load(&dirs);
        let hints = Hints {
            program: Some("launcher"),
            ..Hints::default()
        };
        assert_eq!(files.resolve(&hints), None);
    }

    #[test]
    fn missing_folders_give_no_apps() {
        let files = DesktopFiles::load(&[PathBuf::from("/nonexistent/applications")]);
        assert!(files.is_empty());
    }
}
