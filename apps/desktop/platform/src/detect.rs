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

/// The Linux desktop, which decides the Wayland adapters (see `docs/spikes/E13-S01-wayland.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDesktop {
    Gnome,
    Kde,
    /// Any other desktop that names itself: sway, Hyprland, Xfce, Cinnamon…
    Other,
}

impl LinuxDesktop {
    /// The capability report's name for it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gnome => "gnome",
            Self::Kde => "kde",
            Self::Other => "other",
        }
    }
}

/// What the agent runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformInfo {
    pub os: OperatingSystem,
    /// Present on Linux only.
    pub linux_session: Option<LinuxSession>,
    /// Present on Linux when the session names its desktop.
    pub linux_desktop: Option<LinuxDesktop>,
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
                linux_desktop: None,
            }
        } else if cfg!(target_os = "windows") {
            Self {
                os: OperatingSystem::Windows,
                linux_session: None,
                linux_desktop: None,
            }
        } else {
            let var = |name: &str| std::env::var(name).ok();
            Self {
                os: OperatingSystem::Linux,
                linux_session: Some(linux_session_from_env(var)),
                linux_desktop: linux_desktop_from_env(var),
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

/// Reads the desktop from `XDG_CURRENT_DESKTOP`, a colon-separated list such as
/// `ubuntu:GNOME` or `KDE`, falling back to `XDG_SESSION_DESKTOP`. Any desktop listing `GNOME`
/// counts as GNOME (Ubuntu's and Pop's do); whether GNOME Shell actually runs is up to the
/// adapters, which ask it.
#[must_use]
pub fn linux_desktop_from_env(var: impl Fn(&str) -> Option<String>) -> Option<LinuxDesktop> {
    let names = var("XDG_CURRENT_DESKTOP")
        .filter(|names| !names.trim().is_empty())
        .or_else(|| var("XDG_SESSION_DESKTOP"))?;
    let names: Vec<&str> = names
        .split(':')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let has = |wanted: &str| names.iter().any(|name| name.eq_ignore_ascii_case(wanted));
    if names.is_empty() {
        None
    } else if has("GNOME") {
        Some(LinuxDesktop::Gnome)
    } else if has("KDE") || has("plasma") {
        Some(LinuxDesktop::Kde)
    } else {
        Some(LinuxDesktop::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::{LinuxDesktop, LinuxSession, linux_desktop_from_env, linux_session_from_env};

    fn lookup<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    fn session(vars: &[(&str, &str)]) -> LinuxSession {
        linux_session_from_env(lookup(vars))
    }

    fn desktop(vars: &[(&str, &str)]) -> Option<LinuxDesktop> {
        linux_desktop_from_env(lookup(vars))
    }

    #[test]
    fn desktops_are_read_from_the_session() {
        let gnome = Some(LinuxDesktop::Gnome);
        assert_eq!(desktop(&[("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")]), gnome);
        assert_eq!(desktop(&[("XDG_CURRENT_DESKTOP", "GNOME")]), gnome);
        assert_eq!(desktop(&[("XDG_CURRENT_DESKTOP", "pop:gnome")]), gnome);
        assert_eq!(
            desktop(&[("XDG_CURRENT_DESKTOP", "KDE")]),
            Some(LinuxDesktop::Kde)
        );
        assert_eq!(
            desktop(&[("XDG_CURRENT_DESKTOP", "sway")]),
            Some(LinuxDesktop::Other)
        );
        assert_eq!(
            desktop(&[("XDG_CURRENT_DESKTOP", "X-Cinnamon")]),
            Some(LinuxDesktop::Other)
        );
        assert_eq!(
            desktop(&[("XDG_CURRENT_DESKTOP", "GNOME-Classic:GNOME")]),
            gnome
        );
        assert_eq!(
            desktop(&[("XDG_SESSION_DESKTOP", "plasma")]),
            Some(LinuxDesktop::Kde)
        );
        assert_eq!(
            desktop(&[
                ("XDG_CURRENT_DESKTOP", " "),
                ("XDG_SESSION_DESKTOP", "gnome")
            ]),
            gnome
        );
        assert_eq!(desktop(&[("XDG_CURRENT_DESKTOP", "::")]), None);
        assert_eq!(desktop(&[]), None);
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
