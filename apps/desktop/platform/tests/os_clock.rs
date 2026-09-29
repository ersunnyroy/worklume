//! The real clocks, on whichever OS runs the tests (CI runs macOS, Windows and Linux).

#![allow(
    clippy::unwrap_used,
    reason = "a test fails on a clock error, which is what it checks"
)]

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use worklume_platform::{Clock, OsClock};

#[test]
fn the_boot_id_is_stable_within_a_boot() {
    let first = OsClock::new().unwrap().read().unwrap();
    let second = OsClock::new().unwrap().read().unwrap();
    assert!(!first.boot_id.is_empty());
    assert_eq!(
        first.boot_id, second.boot_id,
        "a second reader sees the same boot"
    );
}

#[test]
fn readings_move_forward_together() {
    let clock = OsClock::new().unwrap();
    let before = clock.read().unwrap();
    thread::sleep(Duration::from_millis(50));
    let after = clock.read().unwrap();

    assert!(before.mono_ms > 0);
    assert!(before.suspended_ms >= 0);
    assert!(
        before.suspended_ms <= before.mono_ms,
        "time asleep is part of the time since boot"
    );
    let mono = after.mono_ms - before.mono_ms;
    assert!(
        (50..5_000).contains(&mono),
        "monotonic time advanced {mono} ms"
    );
    let wall = after.wall_ms - before.wall_ms;
    assert!((0..5_000).contains(&wall), "wall time advanced {wall} ms");
    let slept = after.suspended_ms - before.suspended_ms;
    assert!(
        (0..1_000).contains(&slept),
        "a sleeping thread is not a sleeping device ({slept} ms)"
    );
}

#[test]
fn wall_time_is_utc_milliseconds() {
    let reading = OsClock::new().unwrap().read().unwrap();
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    assert!((now - reading.wall_ms).abs() < 5_000);
}
