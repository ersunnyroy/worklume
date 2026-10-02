//! Shows what this session's sensors measure, once a second: which started, idle time, active
//! spans and the lock. For checking an adapter on a real desktop; it prints no window titles,
//! addresses or input.
//!
//! `cargo run -p worklume-platform --example sensors -- [seconds]` (10 by default).

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "a diagnostic that reports to the terminal"
)]

use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use worklume_platform::{Clock, DeviceSensors, OsClock, PlatformInfo};

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
    let sensors = DeviceSensors::start(&clock);
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
        let locked = sensors
            .lock
            .locked()
            .map_or_else(|error| error.to_string(), |locked| locked.to_string());
        println!("idle {idle}; spans {spans}; locked {locked}");
    }
}
