//! `clock_gettime`, shared by the Linux and macOS clocks.

#![allow(
    unsafe_code,
    reason = "clock_gettime is a C function; the one call below documents its safety"
)]

use super::ClockError;

/// Reads `clock` in nanoseconds.
pub(super) fn read_ns(clock: libc::clockid_t, context: &'static str) -> Result<i128, ClockError> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a valid, writable `timespec` that outlives the call, and
    // `clock_gettime` writes nothing else.
    let result = unsafe { libc::clock_gettime(clock, &raw mut time) };
    if result != 0 {
        return Err(ClockError::new(context, std::io::Error::last_os_error()));
    }
    Ok(i128::from(time.tv_sec) * 1_000_000_000 + i128::from(time.tv_nsec))
}
