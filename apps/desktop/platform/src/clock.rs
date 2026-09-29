//! The device's clocks (architecture §13).
//!
//! A reading holds the wall clock, a monotonic clock that **keeps counting while the device
//! sleeps**, the total time the device has spent asleep since boot, and the boot's ID.
//!
//! Rust's `Instant` is not enough for the tracker: on Linux and macOS it stops while the device
//! sleeps, so wall time derived from it (anchor plus monotonic delta) would fall behind by every
//! sleep. Each OS has a pair of counters that start together at boot, one counting sleep and one
//! not; their difference is exactly the time spent asleep. The tracker reads sleep from that
//! difference instead of from power notifications, which a busy or restarting agent can miss.
//!
//! | OS | Monotonic, counting sleep | Awake time only | Boot ID |
//! | --- | --- | --- | --- |
//! | Linux | `CLOCK_BOOTTIME` | `CLOCK_MONOTONIC` | `/proc/sys/kernel/random/boot_id` |
//! | macOS | `CLOCK_MONOTONIC_RAW` | `CLOCK_UPTIME_RAW` | `kern.bootsessionuuid` |
//! | Windows | `QueryInterruptTime` | `QueryUnbiasedInterruptTime` | a volatile registry key |

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
#[path = "clock/linux.rs"]
mod os;
#[cfg(target_os = "macos")]
#[path = "clock/macos.rs"]
mod os;
#[cfg(target_os = "windows")]
#[path = "clock/windows.rs"]
mod os;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
#[path = "clock/unsupported.rs"]
mod os;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;

/// One reading of the device's clocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockReading {
    /// UTC milliseconds since the Unix epoch, as the device's clock says. It can jump when the
    /// clock is changed.
    pub wall_ms: i64,
    /// Milliseconds since boot. It never goes backwards within a boot, and keeps counting while
    /// the device sleeps.
    pub mono_ms: i64,
    /// Milliseconds the device has spent asleep (suspended or hibernated) since boot. It grows
    /// only across a sleep, by the sleep's length.
    pub suspended_ms: i64,
    /// Identifies the boot. Monotonic readings compare only within one boot.
    pub boot_id: String,
}

/// The device's clocks, behind a trait so tests can control time.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Reads every clock at once.
    ///
    /// # Errors
    ///
    /// Fails when the OS refuses a reading.
    fn read(&self) -> Result<ClockReading, ClockError>;
}

/// A clock that could not be read.
#[derive(Debug, thiserror::Error)]
#[error("{context}: {detail}")]
pub struct ClockError {
    context: &'static str,
    detail: String,
}

impl ClockError {
    pub(crate) fn new(context: &'static str, detail: impl fmt::Display) -> Self {
        Self {
            context,
            detail: detail.to_string(),
        }
    }
}

/// The operating system's clocks. See the module documentation.
#[derive(Debug)]
pub struct OsClock {
    boot_id: String,
}

impl OsClock {
    /// Reads the boot ID, which stays the same for the process's lifetime.
    ///
    /// # Errors
    ///
    /// Fails when the boot ID cannot be read.
    pub fn new() -> Result<Self, ClockError> {
        let boot_id = os::boot_id()?;
        if boot_id.is_empty() {
            return Err(ClockError::new("reading the boot ID", "the ID is empty"));
        }
        Ok(Self { boot_id })
    }
}

impl Clock for OsClock {
    fn read(&self) -> Result<ClockReading, ClockError> {
        let counters = os::counters()?;
        Ok(ClockReading {
            wall_ms: wall_ms()?,
            mono_ms: nanos_to_millis(counters.with_sleep_ns),
            // The awake counter is read first, so the difference is never below the sleep time;
            // it may exceed it by the microseconds between the two reads.
            suspended_ms: nanos_to_millis((counters.with_sleep_ns - counters.awake_ns).max(0)),
            boot_id: self.boot_id.clone(),
        })
    }
}

/// Both monotonic counters of one reading, in nanoseconds since boot.
struct Counters {
    awake_ns: i128,
    with_sleep_ns: i128,
}

fn nanos_to_millis(nanos: i128) -> i64 {
    i64::try_from(nanos / 1_000_000).unwrap_or(i64::MAX)
}

fn wall_ms() -> Result<i64, ClockError> {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ClockError::new("reading the wall clock", "it is set before 1970"))?;
    i64::try_from(since_epoch.as_millis())
        .map_err(|_| ClockError::new("reading the wall clock", "it is out of range"))
}
