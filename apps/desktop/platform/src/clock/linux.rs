//! Linux: `CLOCK_BOOTTIME` counts suspend and `CLOCK_MONOTONIC` does not; the kernel names each
//! boot in `/proc/sys/kernel/random/boot_id`.

use super::{ClockError, Counters, unix};

pub(super) fn counters() -> Result<Counters, ClockError> {
    let awake_ns = unix::read_ns(libc::CLOCK_MONOTONIC, "reading CLOCK_MONOTONIC")?;
    let with_sleep_ns = unix::read_ns(libc::CLOCK_BOOTTIME, "reading CLOCK_BOOTTIME")?;
    Ok(Counters {
        awake_ns,
        with_sleep_ns,
    })
}

pub(super) fn boot_id() -> Result<String, ClockError> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|id| id.trim().to_owned())
        .map_err(|error| ClockError::new("reading the boot ID", error))
}
