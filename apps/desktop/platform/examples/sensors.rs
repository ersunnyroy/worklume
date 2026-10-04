//! Shows what this session's sensors measure, once a second: which started, idle time, active
//! spans, the app in front and the lock. For checking an adapter on a real desktop; it prints no
//! window titles, addresses, input or pixels.
//!
//! `cargo run -p worklume-platform --example sensors -- [seconds] [capture]` (10 by default).
//! With `capture`, it also captures every monitor at the start and twice more, and prints the
//! frames' sizes: on Wayland the first asks to share the screen, the next ones shouldn't. Secrets
//! (the screen-sharing consent) are kept in memory only.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a diagnostic that reports to the terminal"
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use worklume_platform::{
    Clock, DeviceSensors, Monitors, OsClock, PlatformInfo, Secret, SecretStore, SecretStoreError,
};

/// Secrets in memory, for this run only.
#[derive(Debug, Default)]
struct Memory(Mutex<HashMap<String, String>>);

impl SecretStore for Memory {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError> {
        let entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(entries.get(name).map(|value| Secret::new(value.clone())))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        entries.insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(name);
        Ok(())
    }
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(10);
    let clock: Arc<dyn Clock> = match OsClock::new() {
        Ok(clock) => Arc::new(clock),
        Err(error) => {
            eprintln!("no clock: {error}");
            return;
        }
    };
    let secrets: Arc<dyn SecretStore> = Arc::new(Memory::default());
    let sensors = DeviceSensors::start(&clock, &secrets);
    println!("{:?}", PlatformInfo::current());
    println!(
        "input events: {}, active spans: {}",
        sensors.input_hooks, sensors.activity_spans
    );
    for problem in &sensors.problems {
        println!("not started: {problem}");
    }
    for limit in &sensors.limits {
        println!("limit: {limit}");
    }
    if std::env::args().any(|arg| arg == "capture") {
        for attempt in 1..=3 {
            let started = Instant::now();
            match sensors.capture.capture(Monitors::All) {
                Ok(frames) => {
                    let sizes: Vec<String> = frames
                        .iter()
                        .map(|frame| {
                            format!(
                                "{} {}x{} at ({}, {}), {} bytes",
                                frame.monitor.name,
                                frame.monitor.width,
                                frame.monitor.height,
                                frame.monitor.x,
                                frame.monitor.y,
                                frame.rgba.len()
                            )
                        })
                        .collect();
                    println!(
                        "capture {attempt}: {} in {:?}",
                        sizes.join("; "),
                        started.elapsed()
                    );
                }
                Err(error) => println!("capture {attempt}: {error} after {:?}", started.elapsed()),
            }
        }
    }
    for _ in 0..seconds {
        sleep(Duration::from_secs(1));
        let now = clock
            .read()
            .map(|reading| reading.mono_ms)
            .unwrap_or_default();
        let idle = sensors.idle.idle_for().map_or_else(
            |error| error.to_string(),
            |idle| format!("{} ms", idle.as_millis()),
        );
        let spans = sensors.spans.drain().map_or_else(
            |error| error.to_string(),
            |spans| {
                spans
                    .iter()
                    .map(|span| match span.end_mono {
                        Some(end) => format!("[{}, {}) ms ago", now - span.start_mono, now - end),
                        None => format!("[{} ms ago, now)", now - span.start_mono),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        );
        // The app only: never the title.
        let front = sensors.window.foreground(false).map_or_else(
            |error| error.to_string(),
            |app| app.map_or_else(|| "nothing".to_owned(), |app| app.identifier),
        );
        let locked = sensors
            .lock
            .locked()
            .map_or_else(|error| error.to_string(), |locked| locked.to_string());
        println!("idle {idle}; spans {spans}; in front {front}; locked {locked}");
    }
}
