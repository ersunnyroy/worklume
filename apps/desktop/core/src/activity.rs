//! Activity % (E12-S03): a second is active when the member used the keyboard or mouse in it,
//! and a slot's activity is its active time over its tracked time (Phase 1 research, "Activity
//! %"). Keyboard and mouse are measured the same way on their own, so reports can split them.
//!
//! **From input events**, where the OS reports them (hooks: the macOS event tap, Windows raw
//! input, X11 `XInput2`), which also tells keyboard from mouse. An input makes the second after
//! it active. Counting from each input rather than on the clock's whole seconds means a tick
//! boundary never splits or double-counts a second: one key press is one active second however
//! the ticks fall, and continuous typing makes the whole time active. Key presses count for the
//! keyboard; pointer moves, clicks and scrolls for the mouse.
//!
//! **From the idle source**, where the OS gives only the time since the last input (Wayland, or
//! an event reader that isn't working): the combined figure the E12-S03 criterion asks for.
//! Each reading contributes the moment of the last input, so continuous input still covers the
//! whole time and a lone input one second, but inputs between two readings are unknown.
//! Keyboard and mouse are unknown too, which makes the slot's split null.
//!
//! The meter remembers the latest input of each kind, so a second that began in one tick is
//! counted in the next. Whatever stops the count (a sleep, a locked session, a new boot) makes it
//! forget.

use worklume_platform::{InputEvent, InputKind};

use crate::tracker::Activity;

/// How long an input keeps the member active.
pub const ACTIVE_AFTER_INPUT_MS: i64 = 1_000;

/// Measures activity tick by tick. See the module documentation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityMeter {
    any: Option<i64>,
    key: Option<i64>,
    mouse: Option<i64>,
}

impl ActivityMeter {
    /// Activity in the monotonic interval `(from_mono, to_mono]`, from the input events seen
    /// since the previous call.
    #[must_use]
    pub fn from_events(&mut self, events: &[InputEvent], from_mono: i64, to_mono: i64) -> Activity {
        let times = |keys: bool| {
            events
                .iter()
                .filter(move |event| (event.kind == InputKind::Key) == keys)
                .map(|event| event.mono_ms)
        };
        let active = covered(
            &mut self.any,
            events.iter().map(|event| event.mono_ms),
            from_mono,
            to_mono,
        );
        let keyboard = covered(&mut self.key, times(true), from_mono, to_mono);
        let mouse = covered(&mut self.mouse, times(false), from_mono, to_mono);
        Activity {
            active_ms: active,
            keyboard_ms: Some(keyboard),
            mouse_ms: Some(mouse),
        }
    }

    /// Activity in `(from_mono, to_mono]` when only the moment of the latest input is known.
    #[must_use]
    pub fn from_last_input(
        &mut self,
        last_input_mono: Option<i64>,
        from_mono: i64,
        to_mono: i64,
    ) -> Activity {
        self.key = None;
        self.mouse = None;
        Activity {
            active_ms: covered(
                &mut self.any,
                last_input_mono.into_iter(),
                from_mono,
                to_mono,
            ),
            keyboard_ms: None,
            mouse_ms: None,
        }
    }

    /// Forgets earlier input: nothing before this carries into the next tick.
    pub fn forget(&mut self) {
        *self = Self::default();
    }
}

/// Milliseconds of `(from, to]` that lie within [`ACTIVE_AFTER_INPUT_MS`] after `last` or any of
/// `times`. Moves `last` to the latest time seen.
fn covered(last: &mut Option<i64>, times: impl Iterator<Item = i64>, from: i64, to: i64) -> u64 {
    let mut all: Vec<i64> = last.iter().copied().chain(times).collect();
    all.sort_unstable();
    if let Some(&latest) = all.last() {
        *last = Some(latest);
    }
    let mut total: i64 = 0;
    // Everything up to here is counted already, or outside the interval.
    let mut reached = from;
    for time in all {
        let start = time.max(reached);
        let end = time.saturating_add(ACTIVE_AFTER_INPUT_MS).min(to);
        if end > start {
            total += end - start;
            reached = end;
        }
    }
    u64::try_from(total).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use worklume_platform::{InputEvent, InputKind};

    use super::ActivityMeter;
    use crate::tracker::Activity;

    fn key(mono_ms: i64) -> InputEvent {
        InputEvent {
            mono_ms,
            kind: InputKind::Key,
        }
    }

    fn pointer(mono_ms: i64) -> InputEvent {
        InputEvent {
            mono_ms,
            kind: InputKind::Pointer,
        }
    }

    const fn split(active_ms: u64, keyboard_ms: u64, mouse_ms: u64) -> Activity {
        Activity {
            active_ms,
            keyboard_ms: Some(keyboard_ms),
            mouse_ms: Some(mouse_ms),
        }
    }

    #[test]
    fn one_key_press_is_one_active_second_however_the_ticks_fall() {
        for offset in [0, 1, 300, 999] {
            let mut meter = ActivityMeter::default();
            let first = meter.from_events(&[key(offset)], offset - 1, 1_000);
            let second = meter.from_events(&[], 1_000, 2_000);
            let third = meter.from_events(&[], 2_000, 3_000);
            assert_eq!(
                first.active_ms + second.active_ms + third.active_ms,
                1_000,
                "offset {offset}"
            );
            assert_eq!(third, split(0, 0, 0));
        }
    }

    #[test]
    fn continuous_input_makes_the_whole_time_active() {
        let mut meter = ActivityMeter::default();
        let mut total = 0;
        for tick in 0..10 {
            let from = tick * 1_000;
            let events: Vec<_> = (0..4).map(|n| key(from + 100 + n * 250)).collect();
            total += meter.from_events(&events, from, from + 1_000).active_ms;
        }
        // The first 100 ms came before any input.
        assert_eq!(total, 9_900);
    }

    #[test]
    fn keyboard_and_mouse_are_measured_apart() {
        let mut meter = ActivityMeter::default();
        let activity = meter.from_events(&[key(0), pointer(3_000)], -1, 5_000);
        assert_eq!(activity, split(2_000, 1_000, 1_000));
        // Overlapping key and mouse seconds count once for the total.
        let mut meter = ActivityMeter::default();
        let activity = meter.from_events(&[key(0), pointer(500)], -1, 5_000);
        assert_eq!(activity, split(1_500, 1_000, 1_000));
    }

    #[test]
    fn activity_never_exceeds_the_interval() {
        let mut meter = ActivityMeter::default();
        let events: Vec<_> = (0..1_000).map(|n| pointer(n * 8)).collect();
        let activity = meter.from_events(&events, 0, 1_000);
        assert!(activity.active_ms <= 1_000);
        assert_eq!(activity.active_ms, 1_000);
        // An empty or backwards interval is never active.
        assert_eq!(meter.from_events(&[key(5_000)], 5_000, 5_000).active_ms, 0);
        assert_eq!(meter.from_events(&[key(5_000)], 6_000, 5_000).active_ms, 0);
    }

    #[test]
    fn input_before_the_interval_still_counts_inside_it() {
        // Drained late: the event happened before the tick began.
        let mut meter = ActivityMeter::default();
        assert_eq!(
            meter.from_events(&[key(800)], 1_000, 2_000),
            split(800, 800, 0)
        );
    }

    #[test]
    fn forgetting_drops_the_second_carried_over() {
        let mut meter = ActivityMeter::default();
        let _ = meter.from_events(&[key(900)], 0, 1_000);
        meter.forget();
        assert_eq!(meter.from_events(&[], 1_000, 2_000), split(0, 0, 0));
    }

    #[test]
    fn the_idle_source_gives_the_combined_figure() {
        let mut meter = ActivityMeter::default();
        // Continuous input: each reading's last input was just now.
        let mut total = 0;
        for tick in 1..=10 {
            let to = tick * 1_000;
            let activity = meter.from_last_input(Some(to - 20), to - 1_000, to);
            assert_eq!(activity.keyboard_ms, None);
            assert_eq!(activity.mouse_ms, None);
            total += activity.active_ms;
        }
        assert_eq!(total, 9_020);
        // A lone input, then nothing: one second in all.
        let mut meter = ActivityMeter::default();
        let a = meter
            .from_last_input(Some(10_500), 10_000, 11_000)
            .active_ms;
        let b = meter
            .from_last_input(Some(10_500), 11_000, 12_000)
            .active_ms;
        let c = meter
            .from_last_input(Some(10_500), 12_000, 13_000)
            .active_ms;
        assert_eq!((a, b, c), (500, 500, 0));
    }
}
