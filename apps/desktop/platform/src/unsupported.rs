//! Sensors for a platform whose adapter hasn't arrived yet (E10–E13): every reading is
//! [`IdleError::Unsupported`] or [`SensorError::Unsupported`]. The agent then tracks time
//! without idle detection, app usage, input features or lock detection, and reports the
//! capability as missing, rather than guessing a value.

use std::time::Duration;

use crate::{
    ActiveWindow, Autostart, AutostartError, BrowserTab, BrowserUrl, ForegroundApp, Frame,
    IdleError, IdleSource, InputActivity, InputEvent, Monitor, Monitors, ScreenCapture,
    SensorError, SessionLock,
};

/// A sensor that cannot read anything in this build.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unsupported;

impl IdleSource for Unsupported {
    fn idle_for(&self) -> Result<Duration, IdleError> {
        Err(IdleError::Unsupported)
    }
}

impl ActiveWindow for Unsupported {
    fn foreground(&self, _with_title: bool) -> Result<Option<ForegroundApp>, SensorError> {
        Err(SensorError::Unsupported)
    }
}

impl BrowserUrl for Unsupported {
    fn active_tab(&self, _app: &ForegroundApp) -> Result<Option<BrowserTab>, SensorError> {
        Err(SensorError::Unsupported)
    }
}

impl InputActivity for Unsupported {
    fn drain(&self) -> Result<Vec<InputEvent>, SensorError> {
        Err(SensorError::Unsupported)
    }
}

impl SessionLock for Unsupported {
    fn locked(&self) -> Result<bool, SensorError> {
        Err(SensorError::Unsupported)
    }
}

impl ScreenCapture for Unsupported {
    fn monitors(&self) -> Result<Vec<Monitor>, SensorError> {
        Err(SensorError::Unsupported)
    }

    fn capture(&self, _which: Monitors) -> Result<Vec<Frame>, SensorError> {
        Err(SensorError::Unsupported)
    }
}

impl Autostart for Unsupported {
    fn enabled(&self) -> Result<bool, AutostartError> {
        Err(AutostartError::Unsupported)
    }

    fn set_enabled(&self, _enabled: bool) -> Result<(), AutostartError> {
        Err(AutostartError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::Unsupported;
    use crate::{
        ActiveWindow, Autostart, AutostartError, BrowserUrl, ForegroundApp, IdleError, IdleSource,
        InputActivity, Monitors, ScreenCapture, SensorError, SessionLock,
    };

    #[test]
    fn every_reading_is_unsupported() {
        let app = ForegroundApp {
            identifier: "com.example".to_owned(),
            name: "Example".to_owned(),
            title: None,
        };
        assert_eq!(Unsupported.idle_for(), Err(IdleError::Unsupported));
        assert_eq!(Unsupported.foreground(true), Err(SensorError::Unsupported));
        assert_eq!(Unsupported.active_tab(&app), Err(SensorError::Unsupported));
        assert_eq!(Unsupported.drain(), Err(SensorError::Unsupported));
        assert_eq!(Unsupported.locked(), Err(SensorError::Unsupported));
        assert_eq!(Unsupported.enabled(), Err(AutostartError::Unsupported));
        assert_eq!(Unsupported.monitors(), Err(SensorError::Unsupported));
        assert!(matches!(
            Unsupported.capture(Monitors::All),
            Err(SensorError::Unsupported)
        ));
        assert_eq!(
            Unsupported.set_enabled(true),
            Err(AutostartError::Unsupported)
        );
    }
}
