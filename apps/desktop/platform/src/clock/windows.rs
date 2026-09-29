//! Windows: interrupt time counts sleep and hibernation, unbiased interrupt time does not; both
//! count from boot in 100-nanosecond units.
//!
//! Windows documents no boot ID, so the agent keeps one in a volatile registry key
//! (`HKEY_CURRENT_USER\Software\Worklume\BootSession`). Windows discards volatile keys when the
//! user's hive unloads, at sign-out or restart, so a restart always brings a new ID. A sign-out
//! also does, which only closes the open slots early. The ID is named after the boot's moment on
//! the wall clock, which differs between boots.

#![allow(
    unsafe_code,
    reason = "the clock and registry functions are C functions; each call documents its safety"
)]

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPEN_CREATE_OPTIONS,
    REG_OPTION_NON_VOLATILE, REG_OPTION_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegGetValueW, RegSetValueExW,
};
use windows_sys::Win32::System::WindowsProgramming::{
    QueryInterruptTime, QueryUnbiasedInterruptTime,
};

use super::{ClockError, Counters, nanos_to_millis, wall_ms};

/// Interrupt time counts in 100-nanosecond units.
const TICK_NS: i128 = 100;

const PARENT_KEY: &str = r"Software\Worklume";
const BOOT_KEY: &str = "BootSession";
const BOOT_VALUE: &str = "BootId";

pub(super) fn counters() -> Result<Counters, ClockError> {
    let mut awake: u64 = 0;
    // SAFETY: the function writes one u64 into `awake`, which is valid and writable.
    let ok = unsafe { QueryUnbiasedInterruptTime(&raw mut awake) };
    if ok == 0 {
        return Err(ClockError::new(
            "reading the unbiased interrupt time",
            std::io::Error::last_os_error(),
        ));
    }
    let mut with_sleep: u64 = 0;
    // SAFETY: the function writes one u64 into `with_sleep`, which is valid and writable. It
    // cannot fail.
    unsafe { QueryInterruptTime(&raw mut with_sleep) };
    Ok(Counters {
        awake_ns: i128::from(awake) * TICK_NS,
        with_sleep_ns: i128::from(with_sleep) * TICK_NS,
    })
}

pub(super) fn boot_id() -> Result<String, ClockError> {
    // The parent is an ordinary key, so later settings can live beside the volatile one.
    let parent = Key::create(HKEY_CURRENT_USER, PARENT_KEY, REG_OPTION_NON_VOLATILE)?;
    let key = Key::create(parent.0, BOOT_KEY, REG_OPTION_VOLATILE)?;
    if let Some(id) = key.get_string(BOOT_VALUE)?
        && !id.is_empty()
    {
        return Ok(id);
    }
    let boot_ms = wall_ms()? - nanos_to_millis(counters()?.with_sleep_ns);
    let id = format!("windows-{boot_ms}");
    key.set_string(BOOT_VALUE, &id)?;
    Ok(id)
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Key {
    fn create(
        parent: HKEY,
        path: &str,
        options: REG_OPEN_CREATE_OPTIONS,
    ) -> Result<Self, ClockError> {
        let path = wide(path);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `parent` is a predefined root or a key this module opened; `path` is
        // NUL-terminated UTF-16 that outlives the call; `key` is writable. The class, security
        // attributes and disposition are optional and passed as null.
        let status = unsafe {
            RegCreateKeyExW(
                parent,
                path.as_ptr(),
                0,
                std::ptr::null(),
                options,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                std::ptr::null(),
                &raw mut key,
                std::ptr::null_mut(),
            )
        };
        check(status, "opening the boot session's registry key")?;
        Ok(Self(key))
    }

    fn get_string(&self, name: &str) -> Result<Option<String>, ClockError> {
        const CONTEXT: &str = "reading the boot ID from the registry";
        let name = wide(name);
        let mut buffer = [0_u16; 128];
        let mut size = u32::try_from(size_of_val(&buffer)).unwrap_or(0);
        // SAFETY: the key is open with query access; `name` is NUL-terminated UTF-16 that
        // outlives the call; `buffer` is writable for `size` bytes, and the function writes at
        // most that many and stores the count in `size`. The type is not requested (null).
        let status = unsafe {
            RegGetValueW(
                self.0,
                std::ptr::null(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        check(status, CONTEXT)?;
        let written = buffer
            .get(..usize::try_from(size).unwrap_or(0) / 2)
            .unwrap_or_default();
        let text = written.split(|&unit| unit == 0).next().unwrap_or_default();
        String::from_utf16(text)
            .map(Some)
            .map_err(|_| ClockError::new(CONTEXT, "the value is not valid UTF-16"))
    }

    fn set_string(&self, name: &str, value: &str) -> Result<(), ClockError> {
        const CONTEXT: &str = "saving the boot ID in the registry";
        let name = wide(name);
        let value = wide(value);
        let bytes = u32::try_from(size_of_val(value.as_slice()))
            .map_err(|_| ClockError::new(CONTEXT, "the value is too long"))?;
        // SAFETY: the key is open with set access; `name` is NUL-terminated UTF-16; `value` is
        // `bytes` bytes of NUL-terminated UTF-16. Both outlive the call.
        let status = unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                bytes,
            )
        };
        check(status, CONTEXT)
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `RegCreateKeyExW` and is closed exactly once, here.
        unsafe { RegCloseKey(self.0) };
    }
}

/// NUL-terminated UTF-16.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn check(status: WIN32_ERROR, context: &'static str) -> Result<(), ClockError> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(ClockError::new(
            context,
            std::io::Error::from_raw_os_error(status.cast_signed()),
        ))
    }
}
