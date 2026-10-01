//! Test doubles for the platform traits, for tests in other crates and the simulation harness
//! (E09-S14). Built only with the `fake` feature, which production builds never enable.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zeroize::Zeroizing;

use crate::{
    ActiveWindow, BrowserTab, BrowserUrl, Clock, ClockError, ClockReading, ForegroundApp,
    IdleError, IdleSource, InputActivity, InputEvent, Secret, SecretStore, SecretStoreError,
    SecretStoreErrorKind, SensorError,
};

/// A credential store in memory. It can be switched to fail like a locked keychain.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<String, String>>,
    unavailable: Mutex<bool>,
}

impl MemorySecretStore {
    /// Makes every call fail as `Unavailable`, or work again.
    pub fn set_unavailable(&self, unavailable: bool) {
        *self
            .unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = unavailable;
    }

    fn check(&self) -> Result<(), SecretStoreError> {
        if *self
            .unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Err(SecretStoreError::new(
                SecretStoreErrorKind::Unavailable,
                "the fake keychain is locked",
            ))
        } else {
            Ok(())
        }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError> {
        self.check()?;
        Ok(self.entries().get(name).cloned().map(Zeroizing::new))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.entries().insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.entries().remove(name);
        Ok(())
    }
}

/// Clocks under the test's control. Awake time, sleep, clock changes and reboots are separate
/// steps, as on a real device: the monotonic clock keeps counting through a sleep, and the
/// suspended total grows by the sleep's length.
#[derive(Debug)]
pub struct FakeClock {
    reading: Mutex<ClockReading>,
    boots: Mutex<u32>,
    failing: Mutex<bool>,
}

impl FakeClock {
    /// Starts at `wall_ms`, an hour into the first boot (`boot-1`).
    #[must_use]
    pub fn new(wall_ms: i64) -> Self {
        Self {
            reading: Mutex::new(ClockReading {
                wall_ms,
                mono_ms: 3_600_000,
                suspended_ms: 0,
                boot_id: "boot-1".to_owned(),
            }),
            boots: Mutex::new(1),
            failing: Mutex::new(false),
        }
    }

    /// The current reading, whether or not reads are failing.
    #[must_use]
    pub fn reading(&self) -> ClockReading {
        lock(&self.reading).clone()
    }

    /// Awake time passes.
    pub fn advance(&self, duration: Duration) {
        let mut reading = lock(&self.reading);
        let ms = millis(duration);
        reading.wall_ms += ms;
        reading.mono_ms += ms;
    }

    /// The device sleeps for `duration` and wakes.
    pub fn sleep(&self, duration: Duration) {
        let mut reading = lock(&self.reading);
        let ms = millis(duration);
        reading.wall_ms += ms;
        reading.mono_ms += ms;
        reading.suspended_ms += ms;
    }

    /// Someone changes the wall clock; the monotonic clocks do not notice.
    pub fn shift_wall(&self, delta_ms: i64) {
        lock(&self.reading).wall_ms += delta_ms;
    }

    /// The device restarts after `downtime`: a new boot ID, and the monotonic clocks start again
    /// `uptime` after boot.
    pub fn reboot(&self, downtime: Duration, uptime: Duration) {
        let mut boots = lock(&self.boots);
        *boots += 1;
        let mut reading = lock(&self.reading);
        reading.wall_ms += millis(downtime);
        reading.mono_ms = millis(uptime);
        reading.suspended_ms = 0;
        reading.boot_id = format!("boot-{boots}");
    }

    /// Makes reads fail, or work again.
    pub fn set_failing(&self, failing: bool) {
        *lock(&self.failing) = failing;
    }
}

impl Clock for FakeClock {
    fn read(&self) -> Result<ClockReading, ClockError> {
        if *lock(&self.failing) {
            return Err(ClockError::new(
                "reading the fake clock",
                "it is set to fail",
            ));
        }
        Ok(self.reading())
    }
}

/// Idle time measured on a [`FakeClock`]: the time since the last [`FakeIdle::input`], sleep
/// included, as the OS idle counters report it.
#[derive(Debug)]
pub struct FakeIdle {
    clock: Arc<FakeClock>,
    last_input: Mutex<(String, i64)>,
    error: Mutex<Option<IdleError>>,
}

impl FakeIdle {
    /// The member has just touched the keyboard.
    #[must_use]
    pub fn new(clock: Arc<FakeClock>) -> Self {
        let now = clock.reading();
        Self {
            clock,
            last_input: Mutex::new((now.boot_id, now.mono_ms)),
            error: Mutex::new(None),
        }
    }

    /// The member touches the keyboard or mouse now.
    pub fn input(&self) {
        let now = self.clock.reading();
        *lock(&self.last_input) = (now.boot_id, now.mono_ms);
    }

    /// Makes reads fail with `error`, or work again with `None`.
    pub fn set_error(&self, error: Option<IdleError>) {
        *lock(&self.error) = error;
    }
}

impl IdleSource for FakeIdle {
    fn idle_for(&self) -> Result<Duration, IdleError> {
        if let Some(error) = lock(&self.error).clone() {
            return Err(error);
        }
        let now = self.clock.reading();
        let (boot_id, input_mono) = lock(&self.last_input).clone();
        // A new boot starts with fresh input counters.
        let since = if boot_id == now.boot_id {
            now.mono_ms - input_mono
        } else {
            now.mono_ms
        };
        Ok(Duration::from_millis(u64::try_from(since).unwrap_or(0)))
    }
}

/// A foreground app the test sets, with the title kept back unless asked for, as the real
/// readers do. It records whether a title was ever asked for.
#[derive(Debug, Default)]
pub struct FakeWindow {
    app: Mutex<Option<ForegroundApp>>,
    error: Mutex<Option<SensorError>>,
    titles_asked: Mutex<bool>,
}

impl FakeWindow {
    /// Puts `app` in front, or nothing.
    pub fn set(&self, app: Option<ForegroundApp>) {
        *lock(&self.app) = app;
    }

    /// Makes reads fail with `error`, or work again with `None`.
    pub fn set_error(&self, error: Option<SensorError>) {
        *lock(&self.error) = error;
    }

    /// Whether any read asked for the window title.
    #[must_use]
    pub fn titles_asked(&self) -> bool {
        *lock(&self.titles_asked)
    }
}

impl ActiveWindow for FakeWindow {
    fn foreground(&self, with_title: bool) -> Result<Option<ForegroundApp>, SensorError> {
        if let Some(error) = lock(&self.error).clone() {
            return Err(error);
        }
        if with_title {
            *lock(&self.titles_asked) = true;
        }
        Ok(lock(&self.app).clone().map(|mut app| {
            if !with_title {
                app.title = None;
            }
            app
        }))
    }
}

/// Browser tabs by app identifier, set by the test. It records whether it was ever read.
#[derive(Debug, Default)]
pub struct FakeBrowser {
    tabs: Mutex<HashMap<String, BrowserTab>>,
    error: Mutex<Option<SensorError>>,
    read: Mutex<bool>,
}

impl FakeBrowser {
    /// The tab the browser with identifier `app` shows, or none.
    pub fn set(&self, app: &str, tab: Option<BrowserTab>) {
        let mut tabs = lock(&self.tabs);
        match tab {
            Some(tab) => tabs.insert(app.to_owned(), tab),
            None => tabs.remove(app),
        };
    }

    /// Makes reads fail with `error`, or work again with `None`.
    pub fn set_error(&self, error: Option<SensorError>) {
        *lock(&self.error) = error;
    }

    /// Whether any tab was read.
    #[must_use]
    pub fn was_read(&self) -> bool {
        *lock(&self.read)
    }
}

impl BrowserUrl for FakeBrowser {
    fn active_tab(&self, app: &ForegroundApp) -> Result<Option<BrowserTab>, SensorError> {
        *lock(&self.read) = true;
        if let Some(error) = lock(&self.error).clone() {
            return Err(error);
        }
        Ok(lock(&self.tabs).get(&app.identifier).cloned())
    }
}

/// Input events the test pushes, drained like a real reader's buffer. It can fail like a
/// reader without permission, or on Wayland; a failing reader loses what it held.
#[derive(Debug, Default)]
pub struct FakeInput {
    events: Mutex<Vec<InputEvent>>,
    error: Mutex<Option<SensorError>>,
    drains: Mutex<u64>,
}

impl FakeInput {
    /// Events happen, oldest first.
    pub fn push(&self, events: impl IntoIterator<Item = InputEvent>) {
        lock(&self.events).extend(events);
    }

    /// Makes drains fail with `error`, or work again with `None`.
    pub fn set_error(&self, error: Option<SensorError>) {
        *lock(&self.error) = error;
    }

    /// Events not drained yet.
    #[must_use]
    pub fn pending(&self) -> usize {
        lock(&self.events).len()
    }

    /// How many times the buffer was drained, successfully or not.
    #[must_use]
    pub fn drains(&self) -> u64 {
        *lock(&self.drains)
    }
}

impl InputActivity for FakeInput {
    fn drain(&self) -> Result<Vec<InputEvent>, SensorError> {
        *lock(&self.drains) += 1;
        let events = std::mem::take(&mut *lock(&self.events));
        match lock(&self.error).clone() {
            Some(error) => Err(error),
            None => Ok(events),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}
