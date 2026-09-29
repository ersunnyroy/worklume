/// The operating system the agent was built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystem {
    MacOs,
    Windows,
    Linux,
}

/// The Linux display server, which decides which capture and activity APIs exist.
/// Wayland offers reduced, consent-based capabilities (architecture §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxSession {
    X11,
    Wayland,
    Unknown,
}

/// What the agent runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: OperatingSystem,
    /// Present on Linux only.
    pub linux_session: Option<LinuxSession>,
}

impl PlatformInfo {
    /// Detects the current platform from the build target and, on Linux, the session
    /// environment.
    #[must_use]
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self {
                os: OperatingSystem::MacOs,
                linux_session: None,
            }
        } else if cfg!(target_os = "windows") {
            Self {
                os: OperatingSystem::Windows,
                linux_session: None,
            }
        } else {
            Self {
                os: OperatingSystem::Linux,
                linux_session: Some(linux_session_from_env(|name| std::env::var(name).ok())),
            }
        }
    }
}

/// Reads the display server from session variables. `XDG_SESSION_TYPE` is authoritative;
/// `WAYLAND_DISPLAY` and `DISPLAY` are fallbacks for sessions started without a login manager.
#[must_use]
pub fn linux_session_from_env(var: impl Fn(&str) -> Option<String>) -> LinuxSession {
    match var("XDG_SESSION_TYPE").as_deref().map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("wayland") => LinuxSession::Wayland,
        Some(value) if value.eq_ignore_ascii_case("x11") => LinuxSession::X11,
        _ if var("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()) => LinuxSession::Wayland,
        _ if var("DISPLAY").is_some_and(|value| !value.is_empty()) => LinuxSession::X11,
        _ => LinuxSession::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::{LinuxSession, linux_session_from_env};

    fn session(vars: &[(&str, &str)]) -> LinuxSession {
        linux_session_from_env(|name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        })
    }

    #[test]
    fn xdg_session_type_decides() {
        assert_eq!(
            session(&[("XDG_SESSION_TYPE", "wayland")]),
            LinuxSession::Wayland
        );
        assert_eq!(session(&[("XDG_SESSION_TYPE", "x11")]), LinuxSession::X11);
        assert_eq!(
            session(&[("XDG_SESSION_TYPE", "Wayland ")]),
            LinuxSession::Wayland
        );
    }

    #[test]
    fn xdg_session_type_wins_over_display_variables() {
        let vars = [
            ("XDG_SESSION_TYPE", "x11"),
            ("WAYLAND_DISPLAY", "wayland-0"),
        ];
        assert_eq!(session(&vars), LinuxSession::X11);
    }

    #[test]
    fn falls_back_to_display_variables() {
        assert_eq!(
            session(&[("WAYLAND_DISPLAY", "wayland-0")]),
            LinuxSession::Wayland
        );
        assert_eq!(session(&[("DISPLAY", ":0")]), LinuxSession::X11);
        assert_eq!(
            session(&[("XDG_SESSION_TYPE", "tty"), ("DISPLAY", ":0")]),
            LinuxSession::X11
        );
    }

    #[test]
    fn unknown_without_any_hint() {
        assert_eq!(session(&[]), LinuxSession::Unknown);
        assert_eq!(session(&[("DISPLAY", "")]), LinuxSession::Unknown);
    }
}
