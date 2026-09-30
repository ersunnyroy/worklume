//! The XDG autostart entry (E12-S05).
//!
//! At login, desktop sessions start every entry in `$XDG_CONFIG_DIRS/autostart` (by default
//! `/etc/xdg/autostart`) and `$XDG_CONFIG_HOME/autostart` (by default `~/.config/autostart`). An
//! entry in the member's folder replaces the system's entry of the same name, and `Hidden=true`,
//! or GNOME's `X-GNOME-Autostart-enabled=false`, turns it off.
//!
//! The `.deb` and `.rpm` packages install the system entry, so the agent starts at login for
//! every member of the machine. Turning it off writes the member's own entry with
//! `Hidden=true`. Turning it back on removes that entry, so the package's entry, which package
//! updates keep current, applies again. Without an enabled system entry (another way of
//! installing, or an administrator who turned it off), turning it on writes a member's entry that
//! runs this executable, or the `AppImage` it came from.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Autostart, AutostartError, LAUNCHED_AT_LOGIN};

/// The entry's file name, the same in every folder.
pub const ENTRY_NAME: &str = "com.worklume.agent.desktop";

/// The member's entry that turns the system's entry off.
const OFF: &str = "[Desktop Entry]\nType=Application\nName=Worklume\nHidden=true\n";

/// The agent's XDG autostart entry. See the module documentation.
#[derive(Debug, Clone)]
pub struct XdgAutostart {
    user_dir: PathBuf,
    system_dirs: Vec<PathBuf>,
    executable: PathBuf,
}

impl XdgAutostart {
    /// The member's and the system's autostart folders from the XDG variables, starting this
    /// executable, or the `AppImage` it runs from.
    ///
    /// # Errors
    ///
    /// Fails when neither `XDG_CONFIG_HOME` nor `HOME` is set, or this executable can't be found.
    pub fn new() -> Result<Self, AutostartError> {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let config_home = var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| Path::new(&home).join(".config")))
            .ok_or_else(|| AutostartError::Failed("there is no home folder".to_owned()))?;
        let config_dirs = var("XDG_CONFIG_DIRS").map_or_else(
            || "/etc/xdg".to_owned(),
            |dirs| dirs.to_string_lossy().into_owned(),
        );
        let executable = match var("APPIMAGE") {
            Some(appimage) => PathBuf::from(appimage),
            None => std::env::current_exe().map_err(failed)?,
        };
        Ok(Self::with_dirs(
            config_home.join("autostart"),
            config_dirs
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(|dir| Path::new(dir).join("autostart"))
                .collect(),
            executable,
        ))
    }

    /// An entry in `user_dir` over the system folders `system_dirs`, in precedence order,
    /// starting `executable`.
    #[must_use]
    pub const fn with_dirs(
        user_dir: PathBuf,
        system_dirs: Vec<PathBuf>,
        executable: PathBuf,
    ) -> Self {
        Self {
            user_dir,
            system_dirs,
            executable,
        }
    }

    fn user_entry(&self) -> PathBuf {
        self.user_dir.join(ENTRY_NAME)
    }

    /// The system's entry, from the first folder that has one.
    fn system_entry(&self) -> Option<String> {
        self.system_dirs
            .iter()
            .find_map(|dir| fs::read_to_string(dir.join(ENTRY_NAME)).ok())
    }

    /// The member's entry that starts this executable.
    fn entry(&self) -> Result<String, AutostartError> {
        let path = self.executable.to_str().ok_or_else(|| {
            AutostartError::Failed("the executable's path isn't valid UTF-8".to_owned())
        })?;
        Ok(format!(
            "[Desktop Entry]\nType=Application\nName=Worklume\n\
             Comment=Starts the Worklume time tracking agent at login\n\
             Exec={} {LAUNCHED_AT_LOGIN}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exec_argument(path)?
        ))
    }

    fn write_user_entry(&self, text: &str) -> Result<(), AutostartError> {
        fs::create_dir_all(&self.user_dir).map_err(failed)?;
        // Written aside and renamed, so a login never meets half an entry.
        let partial = self.user_dir.join(format!(".{ENTRY_NAME}.partial"));
        fs::write(&partial, text).map_err(failed)?;
        fs::rename(&partial, self.user_entry()).map_err(|error| {
            drop(fs::remove_file(&partial));
            failed(error)
        })
    }

    fn remove_user_entry(&self) -> Result<(), AutostartError> {
        match fs::remove_file(self.user_entry()) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failed(error)),
            _ => Ok(()),
        }
    }
}

impl Autostart for XdgAutostart {
    fn enabled(&self) -> Result<bool, AutostartError> {
        let entry = match fs::read_to_string(self.user_entry()) {
            Ok(text) => Some(text),
            // No member's folder, or a file in its place, which the session can't read either.
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                self.system_entry()
            }
            Err(error) => return Err(failed(error)),
        };
        Ok(entry.is_some_and(|text| is_on(&text)))
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), AutostartError> {
        let system = self.system_entry();
        match (enabled, system) {
            (true, Some(system)) if is_on(&system) => self.remove_user_entry(),
            (true, _) => self.write_user_entry(&self.entry()?),
            (false, Some(_)) => self.write_user_entry(OFF),
            (false, None) => self.remove_user_entry(),
        }
    }
}

/// Whether an entry starts anything: not hidden, and not switched off in GNOME.
fn is_on(text: &str) -> bool {
    let mut in_entry = false;
    let mut on = true;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some((key, value)) = line.split_once('=') {
            match (key.trim(), value.trim()) {
                ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false") => on = false,
                _ => {}
            }
        }
    }
    on
}

/// `path` as the program of an `Exec` line: quoted, with `"`, `` ` ``, `$` and `\` escaped
/// inside the quotes, `%` doubled, and then the backslashes escaped again, as desktop-entry
/// strings require.
fn exec_argument(path: &str) -> Result<String, AutostartError> {
    if path.chars().any(char::is_control) {
        return Err(AutostartError::Failed(
            "the executable's path holds a control character".to_owned(),
        ));
    }
    let mut quoted = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(quoted.replace('\\', "\\\\"))
}

fn failed(error: impl std::fmt::Display) -> AutostartError {
    AutostartError::Failed(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{ENTRY_NAME, XdgAutostart, exec_argument};
    use crate::{Autostart, AutostartError};

    /// Fresh member and system folders, the system's holding `system` if given.
    fn folders(test: &str, system: Option<&str>) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("worklume-autostart-{test}-{}", std::process::id()));
        drop(fs::remove_dir_all(&root));
        let (user, system_dir) = (root.join("user/autostart"), root.join("etc/xdg/autostart"));
        fs::create_dir_all(&system_dir).expect("a folder");
        if let Some(text) = system {
            fs::write(system_dir.join(ENTRY_NAME), text).expect("the system entry");
        }
        (user, system_dir)
    }

    fn autostart(user: &Path, system: &Path) -> XdgAutostart {
        XdgAutostart::with_dirs(
            user.to_path_buf(),
            vec![system.to_path_buf()],
            PathBuf::from("/opt/Worklume/worklume-desktop"),
        )
    }

    fn member_entry(user: &Path) -> Option<String> {
        fs::read_to_string(user.join(ENTRY_NAME)).ok()
    }

    const PACKAGED: &str = "[Desktop Entry]\nType=Application\nName=Worklume\nExec=/usr/bin/worklume-desktop --autostart\n";

    #[test]
    fn without_a_system_entry_the_member_entry_starts_this_executable() {
        let (user, system) = folders("plain", None);
        let autostart = autostart(&user, &system);
        assert_eq!(autostart.enabled(), Ok(false));
        autostart.set_enabled(true).expect("turned on");
        assert_eq!(autostart.enabled(), Ok(true));
        let entry = member_entry(&user).expect("an entry");
        assert!(
            entry.contains("\nExec=\"/opt/Worklume/worklume-desktop\" --autostart\n"),
            "{entry}"
        );
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
        // No partial file is left beside it.
        assert_eq!(fs::read_dir(&user).expect("the folder").count(), 1);
        autostart.set_enabled(false).expect("turned off");
        assert_eq!(autostart.enabled(), Ok(false));
        assert_eq!(member_entry(&user), None);
    }

    #[test]
    fn the_packaged_entry_is_switched_by_a_member_override() {
        let (user, system) = folders("packaged", Some(PACKAGED));
        let autostart = autostart(&user, &system);
        assert_eq!(autostart.enabled(), Ok(true));
        autostart.set_enabled(false).expect("turned off");
        assert_eq!(autostart.enabled(), Ok(false));
        assert!(
            member_entry(&user)
                .expect("an override")
                .contains("Hidden=true")
        );
        // Back on: the override goes, and the package's entry applies again.
        autostart.set_enabled(true).expect("turned on");
        assert_eq!(autostart.enabled(), Ok(true));
        assert_eq!(member_entry(&user), None);
        // Turning it on twice changes nothing.
        autostart.set_enabled(true).expect("still on");
        assert_eq!(member_entry(&user), None);
    }

    #[test]
    fn a_system_entry_turned_off_is_overridden_to_turn_it_on() {
        for off in ["Hidden=true", "X-GNOME-Autostart-enabled=false"] {
            let (user, system) = folders("admin-off", Some(&format!("{PACKAGED}{off}\n")));
            let autostart = autostart(&user, &system);
            assert_eq!(autostart.enabled(), Ok(false), "{off}");
            autostart.set_enabled(true).expect("turned on");
            assert_eq!(autostart.enabled(), Ok(true), "{off}");
            assert!(member_entry(&user).expect("an entry").contains("Exec="));
        }
    }

    #[test]
    fn gnome_startup_applications_switch_counts() {
        // GNOME's Startup Applications turns an entry off with its own key.
        let (user, system) = folders("gnome", Some(PACKAGED));
        fs::create_dir_all(&user).expect("a folder");
        fs::write(
            user.join(ENTRY_NAME),
            format!("{PACKAGED}X-GNOME-Autostart-enabled=false\n"),
        )
        .expect("an entry");
        assert_eq!(autostart(&user, &system).enabled(), Ok(false));
        // A key in another group doesn't.
        fs::write(
            user.join(ENTRY_NAME),
            format!("{PACKAGED}[Desktop Action Quit]\nHidden=true\n"),
        )
        .expect("an entry");
        assert_eq!(autostart(&user, &system).enabled(), Ok(true));
    }

    #[test]
    fn the_executable_is_quoted_and_escaped() {
        assert_eq!(
            exec_argument("/opt/My App/worklume").as_deref(),
            Ok("\"/opt/My App/worklume\"")
        );
        // `$`, `"` and `\` are escaped inside the quotes, `%` is doubled, then every backslash
        // is escaped once more.
        assert_eq!(
            exec_argument("/opt/a$b\"c\\d%e").as_deref(),
            Ok("\"/opt/a\\\\$b\\\\\"c\\\\\\\\d%%e\"")
        );
        assert!(matches!(
            exec_argument("/opt/a\nb"),
            Err(AutostartError::Failed(_))
        ));
    }

    #[test]
    fn a_folder_that_cannot_be_written_changes_nothing() {
        let (user, system) = folders("readonly", None);
        // A file where the member's folder should be.
        fs::create_dir_all(user.parent().expect("a parent")).expect("a folder");
        fs::write(&user, "not a folder").expect("a file");
        let autostart = autostart(&user, &system);
        assert!(matches!(
            autostart.set_enabled(true),
            Err(AutostartError::Failed(_))
        ));
        assert_eq!(autostart.enabled(), Ok(false));
    }
}
