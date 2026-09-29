//! macOS: `CLOCK_MONOTONIC_RAW` keeps counting while the Mac sleeps and `CLOCK_UPTIME_RAW` does
//! not (both unaffected by clock adjustments); `kern.bootsessionuuid` names each boot.

#![allow(
    unsafe_code,
    reason = "sysctlbyname is a C function; the one call below documents its safety"
)]

use std::ffi::CStr;

use super::{ClockError, Counters, unix};

pub(super) fn counters() -> Result<Counters, ClockError> {
    let awake_ns = unix::read_ns(libc::CLOCK_UPTIME_RAW, "reading CLOCK_UPTIME_RAW")?;
    let with_sleep_ns = unix::read_ns(libc::CLOCK_MONOTONIC_RAW, "reading CLOCK_MONOTONIC_RAW")?;
    Ok(Counters {
        awake_ns,
        with_sleep_ns,
    })
}

pub(super) fn boot_id() -> Result<String, ClockError> {
    const CONTEXT: &str = "reading kern.bootsessionuuid";
    let mut buffer = [0_u8; 64];
    let mut length = buffer.len();
    // SAFETY: the name is a NUL-terminated C string; `buffer` is writable for `length` bytes and
    // `sysctlbyname` writes at most that many, storing the count in `length`. No new value is
    // set (null pointer, zero length).
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            buffer.as_mut_ptr().cast(),
            &raw mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(ClockError::new(CONTEXT, std::io::Error::last_os_error()));
    }
    let written = buffer
        .get(..length)
        .ok_or_else(|| ClockError::new(CONTEXT, "the value is too long"))?;
    CStr::from_bytes_until_nul(written)
        .map_err(|_| ClockError::new(CONTEXT, "the value is not a C string"))?
        .to_str()
        .map(str::to_owned)
        .map_err(|_| ClockError::new(CONTEXT, "the value is not UTF-8"))
}
