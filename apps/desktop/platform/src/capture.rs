//! Screenshots: what each monitor shows (per-OS capture: E10-S02, E11-S02, E12-S02, E13-S03).
//!
//! The screenshot pipeline (E09-S07) decides when to capture and what to keep: it skips idle,
//! paused and on-break time, blurs on the device, and encrypts before anything is stored. A
//! capture adapter only returns pixels, one frame per monitor: every monitor, or the active one
//! when the organization chooses (architecture §14). Pixels are screen content, among the most
//! sensitive things the agent handles: a frame's buffer is wiped when it is dropped, and a
//! frame's `Debug` never shows pixels.

use std::fmt;

use zeroize::Zeroizing;

use crate::SensorError;

/// A monitor, in the desktop's coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// The OS's name for it (`DP-1`, `eDP-1`), for telling monitors apart.
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl Monitor {
    /// Whether the point `(x, y)` is on this monitor.
    #[must_use]
    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        let right = i64::from(self.x) + i64::from(self.width);
        let bottom = i64::from(self.y) + i64::from(self.height);
        x >= self.x && y >= self.y && i64::from(x) < right && i64::from(y) < bottom
    }

    /// Whether `other` lies within this monitor.
    #[must_use]
    pub fn contains(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && i64::from(other.x) + i64::from(other.width)
                <= i64::from(self.x) + i64::from(self.width)
            && i64::from(other.y) + i64::from(other.height)
                <= i64::from(self.y) + i64::from(self.height)
    }
}

/// Which monitors to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Monitors {
    All,
    /// The one the member is using: under the pointer, else the primary one.
    Active,
}

/// What one monitor shows: `monitor.width × monitor.height` pixels, rows top to bottom, four
/// bytes each (red, green, blue, alpha), with no padding.
pub struct Frame {
    pub monitor: Monitor,
    pub rgba: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("monitor", &self.monitor)
            .field("bytes", &self.rgba.len())
            .finish_non_exhaustive()
    }
}

/// Captures what the monitors show.
pub trait ScreenCapture: Send + Sync + fmt::Debug {
    /// The monitors, each once: a monitor that merely spans others (a virtual one) is left out,
    /// and mirrored monitors count once.
    ///
    /// # Errors
    ///
    /// Fails when the OS can't say.
    fn monitors(&self) -> Result<Vec<Monitor>, SensorError>;

    /// One frame per monitor asked for.
    ///
    /// # Errors
    ///
    /// Fails when this session can't capture ([`SensorError::Unsupported`]), the member hasn't
    /// allowed it ([`SensorError::PermissionDenied`]), or the capture failed.
    fn capture(&self, which: Monitors) -> Result<Vec<Frame>, SensorError>;
}

/// The monitors to capture from `all`: none that spans others, identical ones once.
#[must_use]
pub fn distinct_monitors(all: &[Monitor]) -> Vec<Monitor> {
    let mut kept: Vec<Monitor> = Vec::with_capacity(all.len());
    for (index, monitor) in all.iter().enumerate() {
        let spans_others = all.iter().enumerate().any(|(other_index, other)| {
            other_index != index && monitor.contains(other) && !other.contains(monitor)
        });
        let seen = kept
            .iter()
            .any(|kept| kept.contains(monitor) && monitor.contains(kept));
        if !spans_others && !seen {
            kept.push(monitor.clone());
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::{Monitor, distinct_monitors};

    fn monitor(name: &str, x: i32, y: i32, width: u32, height: u32) -> Monitor {
        Monitor {
            name: name.to_owned(),
            x,
            y,
            width,
            height,
            primary: false,
        }
    }

    fn names(monitors: &[Monitor]) -> Vec<&str> {
        monitors
            .iter()
            .map(|monitor| monitor.name.as_str())
            .collect()
    }

    #[test]
    fn a_monitor_spanning_others_is_left_out() {
        let all = vec![
            monitor("screen", 0, 0, 2560, 1440),
            monitor("LEFT", 0, 0, 1920, 1080),
            monitor("RIGHT", 1920, 0, 640, 1440),
        ];
        assert_eq!(names(&distinct_monitors(&all)), ["LEFT", "RIGHT"]);
    }

    #[test]
    fn mirrored_monitors_count_once() {
        let all = vec![
            monitor("eDP-1", 0, 0, 1920, 1080),
            monitor("HDMI-1", 0, 0, 1920, 1080),
            monitor("DP-1", 1920, 0, 2560, 1440),
        ];
        assert_eq!(names(&distinct_monitors(&all)), ["eDP-1", "DP-1"]);
    }

    #[test]
    fn overlapping_monitors_are_both_kept() {
        let all = vec![
            monitor("A", 0, 0, 1920, 1080),
            monitor("B", 1000, 0, 1920, 1080),
        ];
        assert_eq!(names(&distinct_monitors(&all)), ["A", "B"]);
    }

    #[test]
    fn points_fall_on_the_monitor_that_holds_them() {
        let right = monitor("RIGHT", 1920, 0, 640, 1440);
        assert!(right.contains_point(1920, 0));
        assert!(right.contains_point(2559, 1439));
        assert!(!right.contains_point(2560, 0));
        assert!(!right.contains_point(1919, 10));
        assert!(!right.contains_point(2000, 1440));
    }
}
