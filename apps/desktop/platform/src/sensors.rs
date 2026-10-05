//! The sensors this session offers: each an OS adapter where one exists and starts, otherwise
//! [`Unsupported`], so a missing capability is reported rather than guessed.
//!
//! - Linux X11: idle time and input events (E12-S03), the window in front (E12-S04) and screen
//!   capture (E12-S02); the session lock from logind or the screen saver (E12-S03, shared with
//!   Wayland).
//! - Linux Wayland: idle time and active spans (E13-S04), from Mutter's `IdleMonitor` on GNOME,
//!   else from `ext-idle-notify-v1`; on GNOME, the window in front from the Worklume extension
//!   (E13-S05), which the agent turns on; screen capture through the desktop portal (E13-S03),
//!   the restore token kept in the credential store. Never the X11 adapters, though `DISPLAY` is set for
//!   Xwayland: through it they would see only X11 apps.
//! - Everything else waits for its story: browser addresses (E12-S04 with E35, E10-S04,
//!   E11-S04), the macOS and Windows front app (E10-S04, E11-S04), and their input and locks
//!   (E10-S03, E11-S03).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::capabilities::{ActivitySource, Readings, assess};
use crate::{
    ActiveWindow, ActivitySpans, BrowserUrl, CapabilityProbe, CapabilityReason, CapabilityReport,
    Clock, Feature, ForegroundApp, IDLE_AFTER_MS, IdleSource, InputActivity, LinuxSession,
    OperatingSystem, PlatformInfo, ReportedPlatform, ScreenCapture, SecretStore, SessionLock,
    Unsupported,
};

/// The sensors of this session, and what couldn't start.
pub struct DeviceSensors {
    pub idle: Arc<dyn IdleSource>,
    pub window: Arc<dyn ActiveWindow>,
    pub browser: Arc<dyn BrowserUrl>,
    pub input: Arc<dyn InputActivity>,
    /// Active spans, where there are no input events (Wayland).
    pub spans: Arc<dyn ActivitySpans>,
    pub lock: Arc<dyn SessionLock>,
    /// Screenshots, for the screenshot pipeline (E09-S07).
    pub capture: Arc<dyn ScreenCapture>,
    /// Input is read event by event (hooks) rather than from idle time alone.
    pub input_hooks: bool,
    /// Activity comes from active spans (a compositor's idle notifications).
    pub activity_spans: bool,
    /// An adapter that exists here but didn't start, and why: for the log.
    pub problems: Vec<String>,
    /// What an adapter that started can't measure well: for the log. The capability report
    /// gives the same as reason codes.
    pub limits: Vec<String>,
    /// Where the agent runs.
    pub platform: PlatformInfo,
    /// Features whose adapter exists here but didn't start.
    failed: Vec<Feature>,
    /// Activity spans count a window that keeps the screen awake as activity.
    inhibited: bool,
    /// Adapters that report their own capability.
    probes: Vec<(Feature, Arc<dyn CapabilityProbe>)>,
    /// Screenshots through the desktop portal, which the member may have declined.
    #[cfg(target_os = "linux")]
    portal: Option<Arc<crate::linux::PortalCapture>>,
    started: Instant,
}

/// How long after starting the readers can answer: a compositor's idle reader knows the idle
/// time only once its first timeout has passed or the member was seen idle.
const SETTLING: Duration = Duration::from_millis(IDLE_AFTER_MS.unsigned_abs() + 500);

impl DeviceSensors {
    /// Starts this session's adapters. Input events are stamped with `clock`; secrets an
    /// adapter keeps (the Wayland screen-capture consent) go to `secrets`.
    #[must_use]
    pub fn start(clock: &Arc<dyn Clock>, secrets: &Arc<dyn SecretStore>) -> Self {
        let mut sensors = Self {
            idle: Arc::new(Unsupported),
            window: Arc::new(Unsupported),
            browser: Arc::new(Unsupported),
            input: Arc::new(Unsupported),
            spans: Arc::new(Unsupported),
            lock: Arc::new(Unsupported),
            capture: Arc::new(Unsupported),
            input_hooks: false,
            activity_spans: false,
            problems: Vec::new(),
            limits: Vec::new(),
            platform: PlatformInfo::current(),
            failed: Vec::new(),
            inhibited: false,
            probes: Vec::new(),
            #[cfg(target_os = "linux")]
            portal: None,
            started: Instant::now(),
        };
        sensors.start_os(clock, secrets);
        sensors
    }

    /// What this device can measure now, from each reader (see [`crate::CapabilityReport`]).
    /// It asks the OS, a D-Bus call or two on Linux, so it belongs off the ticking thread;
    /// GNOME Shell may take up to a second to answer. Asked within 1.5 s of the start,
    /// it waits until then, so a reader that doesn't know yet doesn't read as failing.
    #[must_use]
    pub fn report(&self) -> CapabilityReport {
        if let Some(wait) = SETTLING.checked_sub(self.started.elapsed()) {
            std::thread::sleep(wait);
        }
        // A reader that reads unsupported: no adapter for this OS yet, a desktop without the
        // interface, or no desktop session at all.
        let (platform, missing) = match (self.platform.os, self.platform.linux_session) {
            (OperatingSystem::MacOs, _) => {
                (ReportedPlatform::MacOs, CapabilityReason::NotImplemented)
            }
            (OperatingSystem::Windows, _) => {
                (ReportedPlatform::Windows, CapabilityReason::NotImplemented)
            }
            (OperatingSystem::Linux, Some(LinuxSession::Wayland)) => (
                ReportedPlatform::LinuxWayland,
                CapabilityReason::DesktopUnsupported,
            ),
            (OperatingSystem::Linux, Some(LinuxSession::X11)) => (
                ReportedPlatform::LinuxX11,
                CapabilityReason::DesktopUnsupported,
            ),
            (OperatingSystem::Linux, _) => (
                ReportedPlatform::LinuxX11,
                CapabilityReason::NoGraphicalSession,
            ),
        };
        let activity = if self.input_hooks {
            ActivitySource::Hooks
        } else if self.activity_spans {
            ActivitySource::Spans {
                inhibited: self.inhibited,
            }
        } else {
            ActivitySource::IdleTime
        };
        // Not a browser: a browser reader answers that it isn't one, if it reads at all.
        let not_a_browser = ForegroundApp {
            identifier: "worklume-capability-check".to_owned(),
            name: "Worklume capability check".to_owned(),
            title: None,
        };
        let probed: Vec<_> = self
            .probes
            .iter()
            .map(|(feature, probe)| (*feature, probe.capability()))
            .collect();
        let readings = Readings {
            platform,
            desktop: self.platform.linux_desktop,
            missing,
            failed: self.failed.clone(),
            idle: self.idle.idle_for().map(drop),
            activity,
            apps: if self
                .probes
                .iter()
                .any(|(feature, _)| *feature == Feature::Apps)
            {
                Ok(())
            } else {
                self.window.foreground(false).map(drop)
            },
            urls: self.browser.active_tab(&not_a_browser).map(drop),
            lock: self.lock.locked().map(drop),
            screenshots: if self
                .probes
                .iter()
                .any(|(feature, _)| *feature == Feature::Screenshots)
            {
                Ok(())
            } else {
                self.capture.monitors().and_then(|monitors| {
                    if monitors.is_empty() {
                        Err(crate::SensorError::Failed("no monitor".to_owned()))
                    } else {
                        Ok(())
                    }
                })
            },
            probed,
        };
        assess(&readings)
    }

    /// After the member declined to share their screen, lets the next screenshot ask again,
    /// as the member asked from the agent's window. Returns whether they had declined; on
    /// desktops without such a consent, nothing changes.
    #[must_use]
    pub fn ask_again_for_screen_sharing(&self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(portal) = &self.portal {
            return portal.ask_again();
        }
        false
    }

    #[cfg(target_os = "linux")]
    fn start_os(&mut self, clock: &Arc<dyn Clock>, secrets: &Arc<dyn SecretStore>) {
        use crate::linux::{LinuxSessionLock, X11Capture, X11Idle, X11Input, X11Window};

        self.lock = Arc::new(LinuxSessionLock::new());
        match self.platform.linux_session {
            Some(LinuxSession::X11) => {}
            Some(LinuxSession::Wayland) => {
                // Screenshots through the portal, which asks the member at the first capture.
                let portal = Arc::new(crate::linux::PortalCapture::new(Arc::clone(secrets)));
                self.capture = Arc::clone(&portal) as _;
                self.probes
                    .push((Feature::Screenshots, Arc::clone(&portal) as _));
                self.portal = Some(portal);
                return self.start_wayland(clock);
            }
            _ => return,
        }
        match X11Idle::connect() {
            Ok(idle) => self.idle = Arc::new(idle),
            Err(error) => self.fail(Feature::Idle, format!("idle time: {error}")),
        }
        match X11Window::connect() {
            Ok(window) => self.window = Arc::new(window),
            Err(error) => self.fail(Feature::Apps, format!("the window in front: {error}")),
        }
        match X11Capture::connect() {
            Ok(capture) => self.capture = Arc::new(capture),
            Err(error) => self.fail(Feature::Screenshots, format!("screen capture: {error}")),
        }
        match X11Input::start(Arc::clone(clock)) {
            Ok(input) => {
                self.input = Arc::new(input);
                self.input_hooks = true;
            }
            Err(error) => self.fail(Feature::Activity, format!("input events: {error}")),
        }
    }

    /// GNOME's Mutter first, which idle inhibitors don't affect; then the Wayland protocol.
    #[cfg(target_os = "linux")]
    fn start_wayland(&mut self, clock: &Arc<dyn Clock>) {
        use crate::linux::{MutterIdle, WaylandIdle};
        use crate::{LinuxDesktop, SensorError};

        let gnome = self.platform.linux_desktop == Some(LinuxDesktop::Gnome);
        let mutter = match MutterIdle::connect(Arc::clone(clock), gnome) {
            Ok(mutter) => Some(Arc::new(mutter)),
            Err(SensorError::Unsupported) => None,
            Err(error) => {
                self.problems
                    .push(format!("idle time from Mutter: {error}"));
                None
            }
        };
        if let Some(mutter) = mutter {
            self.idle = Arc::clone(&mutter) as _;
            self.spans = mutter;
            self.activity_spans = true;
            self.start_gnome_window();
            return;
        }
        match WaylandIdle::connect(Arc::clone(clock)) {
            Ok(wayland) => {
                if !wayland.ignores_inhibitors() {
                    self.inhibited = true;
                    self.limits.push(
                        "activity: the compositor's idle notices stop while a window inhibits \
                         idling, so a video playing counts as active"
                            .to_owned(),
                    );
                }
                let wayland = Arc::new(wayland);
                self.idle = Arc::clone(&wayland) as _;
                self.spans = wayland;
                self.activity_spans = true;
            }
            Err(error) => {
                // Activity on Wayland comes from the same reader.
                self.failed.push(Feature::Activity);
                self.fail(Feature::Idle, format!("idle time on Wayland: {error}"));
            }
        }
    }

    /// GNOME's window in front, from the Worklume extension, which the reader turns on once
    /// GNOME Shell knows it (see `linux::GnomeWindow`).
    #[cfg(target_os = "linux")]
    fn start_gnome_window(&mut self) {
        let window = Arc::new(crate::linux::GnomeWindow::new());
        self.window = Arc::clone(&window) as _;
        self.probes.push((Feature::Apps, window));
    }

    /// An adapter that exists here didn't start.
    #[cfg(target_os = "linux")]
    fn fail(&mut self, feature: Feature, problem: String) {
        self.failed.push(feature);
        self.problems.push(problem);
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(
        clippy::unused_self,
        clippy::missing_const_for_fn,
        reason = "the macOS and Windows adapters arrive with E10 and E11"
    )]
    fn start_os(&mut self, _clock: &Arc<dyn Clock>, _secrets: &Arc<dyn SecretStore>) {}
}

impl std::fmt::Debug for DeviceSensors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceSensors")
            .field("input_hooks", &self.input_hooks)
            .field("problems", &self.problems)
            .field("limits", &self.limits)
            .field("platform", &self.platform)
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}
