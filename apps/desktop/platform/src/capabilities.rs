//! What the agent can measure on this device, feature by feature (E13-S02): the capability map
//! of `PUT /v1/desktop/capabilities` (`packages/api-contract/openapi.yaml`), which tells admins
//! why a member has no screenshots or apps instead of leaving silent gaps.
//!
//! - **From the readers, not from the platform's name.** [`crate::DeviceSensors::report`] asks
//!   each reader what it reads now (idle time, the app in front without its title, the lock,
//!   the monitors), and adapters that know more say so through [`CapabilityProbe`]: the portal's
//!   consent, the GNOME extension's state. Input events and active spans are never drained for
//!   it: they belong to the timekeeper.
//! - **Every status but granted says why**, with reason codes the dashboard and the agent's
//!   window explain in the reader's language. Codes only: a report holds no app names, titles,
//!   paths or user names, so it can be logged.

use std::fmt;

use crate::{IdleError, LinuxDesktop, SensorError};

/// A feature the agent measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    Screenshots,
    /// Per-second activity.
    Activity,
    Idle,
    /// The app in front.
    Apps,
    /// Browser addresses.
    Urls,
    SessionLock,
}

impl Feature {
    /// Every feature, in the order reports list them.
    pub const ALL: [Self; 6] = [
        Self::Screenshots,
        Self::Activity,
        Self::Idle,
        Self::Apps,
        Self::Urls,
        Self::SessionLock,
    ];

    /// The contract's name for it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Screenshots => "screenshots",
            Self::Activity => "activity",
            Self::Idle => "idle",
            Self::Apps => "apps",
            Self::Urls => "urls",
            Self::SessionLock => "session_lock",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// How well a feature works here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityStatus {
    /// Measured fully.
    Granted,
    /// Measured, less precisely than on other desktops.
    Reduced,
    /// Waits for the member: a consent not yet given, or the next login.
    Pending,
    /// The member or the OS refused it.
    Denied,
    /// This desktop, or this version of the agent, can't measure it.
    Unsupported,
    /// The reader exists but isn't working.
    Failed,
}

impl CapabilityStatus {
    pub const ALL: [Self; 6] = [
        Self::Granted,
        Self::Reduced,
        Self::Pending,
        Self::Denied,
        Self::Unsupported,
        Self::Failed,
    ];

    /// The contract's name for it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Reduced => "reduced",
            Self::Pending => "pending",
            Self::Denied => "denied",
            Self::Unsupported => "unsupported",
            Self::Failed => "failed",
        }
    }
}

/// Why a feature isn't granted. The contract's `DesktopCapabilityReason` describes each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityReason {
    NotImplemented,
    NoGraphicalSession,
    AdapterFailed,
    PermissionDenied,
    ConsentNotAsked,
    ConsentDialogOpen,
    ConsentDeclined,
    NoScreencastPortal,
    EstimatedFromIdle,
    IdleTimeOnly,
    NoKeyboardMouseSplit,
    IdleInhibitorsCountAsActive,
    GnomeExtensionNextLogin,
    GnomeExtensionDisabled,
    GnomeExtensionFailed,
    DesktopUnsupported,
}

impl CapabilityReason {
    pub const ALL: [Self; 16] = [
        Self::NotImplemented,
        Self::NoGraphicalSession,
        Self::AdapterFailed,
        Self::PermissionDenied,
        Self::ConsentNotAsked,
        Self::ConsentDialogOpen,
        Self::ConsentDeclined,
        Self::NoScreencastPortal,
        Self::EstimatedFromIdle,
        Self::IdleTimeOnly,
        Self::NoKeyboardMouseSplit,
        Self::IdleInhibitorsCountAsActive,
        Self::GnomeExtensionNextLogin,
        Self::GnomeExtensionDisabled,
        Self::GnomeExtensionFailed,
        Self::DesktopUnsupported,
    ];

    /// The contract's code for it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::NoGraphicalSession => "no_graphical_session",
            Self::AdapterFailed => "adapter_failed",
            Self::PermissionDenied => "permission_denied",
            Self::ConsentNotAsked => "consent_not_asked",
            Self::ConsentDialogOpen => "consent_dialog_open",
            Self::ConsentDeclined => "consent_declined",
            Self::NoScreencastPortal => "no_screencast_portal",
            Self::EstimatedFromIdle => "estimated_from_idle",
            Self::IdleTimeOnly => "idle_time_only",
            Self::NoKeyboardMouseSplit => "no_keyboard_mouse_split",
            Self::IdleInhibitorsCountAsActive => "idle_inhibitors_count_as_active",
            Self::GnomeExtensionNextLogin => "gnome_extension_next_login",
            Self::GnomeExtensionDisabled => "gnome_extension_disabled",
            Self::GnomeExtensionFailed => "gnome_extension_failed",
            Self::DesktopUnsupported => "desktop_unsupported",
        }
    }
}

/// One feature's status and its reasons: none when granted, at least one otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    status: CapabilityStatus,
    reasons: Vec<CapabilityReason>,
}

impl Capability {
    /// Measured fully.
    #[must_use]
    pub const fn granted() -> Self {
        Self {
            status: CapabilityStatus::Granted,
            reasons: Vec::new(),
        }
    }

    /// Not fully measured, for `reason` and `more`, each kept once in the order given. A
    /// limited capability is never granted: [`CapabilityStatus::Granted`] reads as reduced.
    #[must_use]
    pub fn limited(
        status: CapabilityStatus,
        reason: CapabilityReason,
        more: &[CapabilityReason],
    ) -> Self {
        let mut reasons = vec![reason];
        for reason in more {
            if !reasons.contains(reason) {
                reasons.push(*reason);
            }
        }
        let status = match status {
            CapabilityStatus::Granted => CapabilityStatus::Reduced,
            status => status,
        };
        Self { status, reasons }
    }

    #[must_use]
    pub const fn status(&self) -> CapabilityStatus {
        self.status
    }

    #[must_use]
    pub fn reasons(&self) -> &[CapabilityReason] {
        &self.reasons
    }
}

/// A reader that knows better than its readings how well it works: the portal's consent, the
/// GNOME extension's state.
pub trait CapabilityProbe: Send + Sync {
    /// The capability now. It may ask the OS, and so take as long as a reading.
    fn capability(&self) -> Capability;
}

/// The platform as the server's `devices.platform` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportedPlatform {
    MacOs,
    Windows,
    LinuxX11,
    LinuxWayland,
}

impl ReportedPlatform {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Windows => "windows",
            Self::LinuxX11 => "linux_x11",
            Self::LinuxWayland => "linux_wayland",
        }
    }
}

/// What this device's agent can measure, feature by feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityReport {
    pub platform: ReportedPlatform,
    pub desktop: Option<LinuxDesktop>,
    features: [Capability; 6],
}

impl CapabilityReport {
    /// A report with each feature's capability from `capability`.
    #[must_use]
    pub fn new(
        platform: ReportedPlatform,
        desktop: Option<LinuxDesktop>,
        mut capability: impl FnMut(Feature) -> Capability,
    ) -> Self {
        Self {
            platform,
            desktop,
            features: Feature::ALL.map(&mut capability),
        }
    }

    #[must_use]
    pub fn get(&self, feature: Feature) -> &Capability {
        &self.features[feature.index()]
    }

    /// Each feature and its capability, in [`Feature::ALL`]'s order.
    pub fn iter(&self) -> impl Iterator<Item = (Feature, &Capability)> {
        Feature::ALL.into_iter().zip(&self.features)
    }

    /// Some feature isn't granted: admins see the device as reduced.
    #[must_use]
    pub fn is_reduced(&self) -> bool {
        self.features
            .iter()
            .any(|capability| capability.status != CapabilityStatus::Granted)
    }
}

/// `screenshots=granted activity=reduced(estimated_from_idle,no_keyboard_mouse_split) …`, for
/// the log.
impl fmt::Display for CapabilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.platform.as_str())?;
        for (feature, capability) in self.iter() {
            write!(f, " {}={}", feature.as_str(), capability.status.as_str())?;
            if !capability.reasons.is_empty() {
                let reasons: Vec<&str> = capability
                    .reasons
                    .iter()
                    .map(|reason| reason.as_str())
                    .collect();
                write!(f, "({})", reasons.join(","))?;
            }
        }
        Ok(())
    }
}

/// How the timekeeper measures activity here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivitySource {
    /// Input events from hooks.
    Hooks,
    /// The compositor's idle notices, through the idle reader; `inhibited` when a window
    /// keeping the screen awake counts as activity.
    Spans { inhibited: bool },
    /// Polled idle time.
    IdleTime,
}

/// What the readers said, for [`assess`].
#[derive(Debug)]
pub(crate) struct Readings {
    pub platform: ReportedPlatform,
    pub desktop: Option<LinuxDesktop>,
    /// Why a reader that reads `Unsupported` can't: no adapter yet, no session, or the
    /// compositor.
    pub missing: CapabilityReason,
    /// Features whose adapter exists here but didn't start.
    pub failed: Vec<Feature>,
    pub idle: Result<(), IdleError>,
    pub activity: ActivitySource,
    pub apps: Result<(), SensorError>,
    pub urls: Result<(), SensorError>,
    pub lock: Result<(), SensorError>,
    pub screenshots: Result<(), SensorError>,
    /// What adapters said of themselves; it replaces their readings.
    pub probed: Vec<(Feature, Capability)>,
}

/// The report from what the readers said.
pub(crate) fn assess(readings: &Readings) -> CapabilityReport {
    use CapabilityReason as Why;
    use CapabilityStatus as Status;

    let from_reading = |reading: &Result<(), SensorError>| match reading {
        Ok(()) => Capability::granted(),
        Err(SensorError::Unsupported) => {
            Capability::limited(Status::Unsupported, readings.missing, &[])
        }
        Err(SensorError::PermissionDenied) => {
            Capability::limited(Status::Denied, Why::PermissionDenied, &[])
        }
        Err(SensorError::Failed(_)) => Capability::limited(Status::Failed, Why::AdapterFailed, &[]),
    };
    // Activity measured from idle time fails with the idle reader.
    let idle = match &readings.idle {
        _ if readings.failed.contains(&Feature::Idle) => {
            Err(SensorError::Failed("didn't start".to_owned()))
        }
        Ok(()) => Ok(()),
        Err(IdleError::Unsupported) => Err(SensorError::Unsupported),
        Err(IdleError::Failed(error)) => Err(SensorError::Failed(error.clone())),
    };
    let activity = match (readings.activity, &idle) {
        (ActivitySource::Hooks, _) => Capability::granted(),
        (_, Err(SensorError::Unsupported)) if readings.failed.contains(&Feature::Activity) => {
            Capability::limited(Status::Failed, Why::AdapterFailed, &[])
        }
        (ActivitySource::Spans { inhibited }, Ok(())) => {
            let inhibitors: &[Why] = if inhibited {
                &[Why::IdleInhibitorsCountAsActive]
            } else {
                &[]
            };
            Capability::limited(
                Status::Reduced,
                Why::EstimatedFromIdle,
                &[&[Why::NoKeyboardMouseSplit], inhibitors].concat(),
            )
        }
        // Input events that didn't start leave the idle time to measure activity.
        (ActivitySource::IdleTime, Ok(())) => {
            let failed: &[Why] = if readings.failed.contains(&Feature::Activity) {
                &[Why::AdapterFailed]
            } else {
                &[]
            };
            Capability::limited(
                Status::Reduced,
                Why::IdleTimeOnly,
                &[&[Why::NoKeyboardMouseSplit], failed].concat(),
            )
        }
        (_, reading) => from_reading(reading),
    };
    CapabilityReport::new(readings.platform, readings.desktop, |feature| {
        if readings.failed.contains(&feature) && feature != Feature::Activity {
            return Capability::limited(Status::Failed, Why::AdapterFailed, &[]);
        }
        if let Some((_, capability)) = readings
            .probed
            .iter()
            .find(|(probed, _)| *probed == feature)
        {
            return capability.clone();
        }
        match feature {
            Feature::Screenshots => from_reading(&readings.screenshots),
            Feature::Activity => activity.clone(),
            Feature::Idle => from_reading(&idle),
            Feature::Apps => from_reading(&readings.apps),
            // Browser addresses come from the browser (AT-SPI or the extension, E35), whatever
            // the compositor.
            Feature::Urls => match &readings.urls {
                Err(SensorError::Unsupported) => {
                    Capability::limited(Status::Unsupported, Why::NotImplemented, &[])
                }
                reading => from_reading(reading),
            },
            Feature::SessionLock => from_reading(&readings.lock),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ActivitySource, Capability, CapabilityReason as Why, CapabilityReport,
        CapabilityStatus as Status, Feature, Readings, ReportedPlatform, assess,
    };
    use crate::{IdleError, LinuxDesktop, SensorError};

    fn x11() -> Readings {
        Readings {
            platform: ReportedPlatform::LinuxX11,
            desktop: Some(LinuxDesktop::Gnome),
            missing: Why::NotImplemented,
            failed: Vec::new(),
            idle: Ok(()),
            activity: ActivitySource::Hooks,
            apps: Ok(()),
            urls: Err(SensorError::Unsupported),
            lock: Ok(()),
            screenshots: Ok(()),
            probed: Vec::new(),
        }
    }

    fn summary(report: &CapabilityReport) -> Vec<(Feature, Status, Vec<Why>)> {
        report
            .iter()
            .map(|(feature, capability)| {
                (feature, capability.status(), capability.reasons().to_vec())
            })
            .collect()
    }

    #[test]
    fn x11_measures_everything_but_urls() {
        let report = assess(&x11());
        assert_eq!(
            summary(&report),
            vec![
                (Feature::Screenshots, Status::Granted, vec![]),
                (Feature::Activity, Status::Granted, vec![]),
                (Feature::Idle, Status::Granted, vec![]),
                (Feature::Apps, Status::Granted, vec![]),
                (
                    Feature::Urls,
                    Status::Unsupported,
                    vec![Why::NotImplemented]
                ),
                (Feature::SessionLock, Status::Granted, vec![]),
            ]
        );
        assert!(report.is_reduced());
        assert_eq!(
            report.to_string(),
            "linux_x11 screenshots=granted activity=granted idle=granted apps=granted \
             urls=unsupported(not_implemented) session_lock=granted"
        );
    }

    #[test]
    fn wayland_activity_is_estimated_and_says_so() {
        let readings = Readings {
            platform: ReportedPlatform::LinuxWayland,
            desktop: Some(LinuxDesktop::Other),
            missing: Why::DesktopUnsupported,
            activity: ActivitySource::Spans { inhibited: true },
            apps: Err(SensorError::Unsupported),
            ..x11()
        };
        let report = assess(&readings);
        let activity = report.get(Feature::Activity);
        assert_eq!(activity.status(), Status::Reduced);
        assert_eq!(
            activity.reasons(),
            [
                Why::EstimatedFromIdle,
                Why::NoKeyboardMouseSplit,
                Why::IdleInhibitorsCountAsActive
            ]
        );
        assert_eq!(
            report.get(Feature::Apps).reasons(),
            [Why::DesktopUnsupported]
        );
        assert_eq!(report.get(Feature::Urls).reasons(), [Why::NotImplemented]);
        let uninhibited = assess(&Readings {
            activity: ActivitySource::Spans { inhibited: false },
            ..readings
        });
        assert_eq!(
            uninhibited.get(Feature::Activity).reasons(),
            [Why::EstimatedFromIdle, Why::NoKeyboardMouseSplit]
        );
    }

    #[test]
    fn activity_follows_the_reader_it_comes_from() {
        let spans_failing = assess(&Readings {
            activity: ActivitySource::Spans { inhibited: false },
            idle: Err(IdleError::Failed("Mutter left".to_owned())),
            ..x11()
        });
        assert_eq!(
            spans_failing.get(Feature::Activity).status(),
            Status::Failed
        );
        assert_eq!(spans_failing.get(Feature::Idle).status(), Status::Failed);

        let polled = assess(&Readings {
            activity: ActivitySource::IdleTime,
            failed: vec![Feature::Activity],
            ..x11()
        });
        assert_eq!(
            polled.get(Feature::Activity).reasons(),
            [
                Why::IdleTimeOnly,
                Why::NoKeyboardMouseSplit,
                Why::AdapterFailed
            ]
        );
        assert_eq!(polled.get(Feature::Activity).status(), Status::Reduced);

        let nothing = assess(&Readings {
            activity: ActivitySource::IdleTime,
            idle: Err(IdleError::Unsupported),
            ..x11()
        });
        assert_eq!(
            nothing.get(Feature::Activity).reasons(),
            [Why::NotImplemented]
        );
        assert_eq!(nothing.get(Feature::Activity).status(), Status::Unsupported);

        let both_failed = assess(&Readings {
            activity: ActivitySource::IdleTime,
            idle: Err(IdleError::Unsupported),
            failed: vec![Feature::Idle, Feature::Activity],
            ..x11()
        });
        for feature in [Feature::Idle, Feature::Activity] {
            assert_eq!(
                both_failed.get(feature).status(),
                Status::Failed,
                "{feature:?}"
            );
            assert_eq!(both_failed.get(feature).reasons(), [Why::AdapterFailed]);
        }
        let idle_failed = assess(&Readings {
            activity: ActivitySource::IdleTime,
            idle: Err(IdleError::Unsupported),
            failed: vec![Feature::Idle],
            ..x11()
        });
        assert_eq!(idle_failed.get(Feature::Activity).status(), Status::Failed);
    }

    #[test]
    fn readings_map_to_statuses() {
        let report = assess(&Readings {
            screenshots: Err(SensorError::Failed("no RandR".to_owned())),
            apps: Err(SensorError::PermissionDenied),
            lock: Err(SensorError::Unsupported),
            failed: vec![Feature::Idle],
            ..x11()
        });
        assert_eq!(
            summary(&report)[..4],
            [
                (
                    Feature::Screenshots,
                    Status::Failed,
                    vec![Why::AdapterFailed]
                ),
                (Feature::Activity, Status::Granted, vec![]),
                (Feature::Idle, Status::Failed, vec![Why::AdapterFailed]),
                (Feature::Apps, Status::Denied, vec![Why::PermissionDenied]),
            ]
        );
        assert_eq!(
            report.get(Feature::SessionLock).reasons(),
            [Why::NotImplemented]
        );
    }

    #[test]
    fn probes_replace_readings_but_not_a_failed_start() {
        let pending = Capability::limited(Status::Pending, Why::ConsentNotAsked, &[]);
        let report = assess(&Readings {
            screenshots: Err(SensorError::PermissionDenied),
            probed: vec![
                (Feature::Screenshots, pending.clone()),
                (Feature::Apps, pending.clone()),
            ],
            failed: vec![Feature::Apps],
            ..x11()
        });
        assert_eq!(report.get(Feature::Screenshots), &pending);
        assert_eq!(report.get(Feature::Apps).status(), Status::Failed);
    }

    #[test]
    fn limited_is_never_granted_and_keeps_each_reason_once() {
        let capability = Capability::limited(
            Status::Granted,
            Why::IdleTimeOnly,
            &[Why::NoKeyboardMouseSplit, Why::IdleTimeOnly],
        );
        assert_eq!(capability.status(), Status::Reduced);
        assert_eq!(
            capability.reasons(),
            [Why::IdleTimeOnly, Why::NoKeyboardMouseSplit]
        );
        assert!(Capability::granted().reasons().is_empty());
    }

    #[test]
    fn granted_everywhere_is_not_reduced() {
        let report = assess(&Readings {
            urls: Ok(()),
            ..x11()
        });
        assert!(!report.is_reduced());
    }

    #[test]
    fn codes_are_unique() {
        let mut reasons: Vec<&str> = Why::ALL.iter().map(|reason| reason.as_str()).collect();
        reasons.sort_unstable();
        reasons.dedup();
        assert_eq!(reasons.len(), Why::ALL.len());
        for (index, feature) in Feature::ALL.iter().enumerate() {
            assert_eq!(feature.index(), index);
        }
    }
}
