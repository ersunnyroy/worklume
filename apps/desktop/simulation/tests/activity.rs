//! E13-S04: activity from a compositor's active spans (Wayland) equals activity from the same
//! input events (X11), whatever the timing of the input and of the agent's ticks. Polling the
//! idle time once a tick, the fallback without either, can fall well short on bursty input; the
//! example shows by how much.
//!
//! `PROPTEST_CASES` sets the number of cases (64 by default).

#![allow(
    clippy::unwrap_used,
    reason = "a failure to set up fails the property, like a broken invariant"
)]

use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use worklume_core::activity::ActivityMeter;
use worklume_platform::fake::{FakeClock, FakeSpans};
use worklume_platform::{ActivitySpans, InputEvent, InputKind};

/// Active milliseconds from events, from spans, and from polling, for inputs `gaps` apart
/// (the first one `gaps[0]` after the start) and ticks of the lengths `ticks`, in turn.
fn measure(gaps: &[u64], ticks: &[u64]) -> (u64, u64, u64) {
    let clock = Arc::new(FakeClock::new(1_789_981_200_000));
    let spans = FakeSpans::new(Arc::clone(&clock));
    let start = clock.reading().mono_ms;
    let inputs: Vec<i64> = gaps
        .iter()
        .scan(start, |at, gap| {
            *at += i64::try_from(*gap).unwrap();
            Some(*at)
        })
        .collect();
    let end = inputs.last().copied().unwrap_or(start) + 2_000;
    let (mut by_events, mut by_spans, mut by_poll) = (
        ActivityMeter::default(),
        ActivityMeter::default(),
        ActivityMeter::default(),
    );
    let (mut events_ms, mut spans_ms, mut poll_ms) = (0, 0, 0);
    let mut from = start;
    let mut last_input = None;
    for tick in ticks.iter().cycle() {
        if from >= end {
            break;
        }
        let to = from + i64::try_from(*tick).unwrap();
        let events: Vec<InputEvent> = inputs
            .iter()
            .filter(|&&at| at > from && at <= to)
            .map(|&mono_ms| InputEvent {
                mono_ms,
                kind: InputKind::Key,
            })
            .collect();
        for event in &events {
            // The compositor sees each input as it happens.
            let now = clock.reading().mono_ms;
            clock.advance(Duration::from_millis(
                u64::try_from(event.mono_ms - now).unwrap(),
            ));
            spans.input(event.mono_ms);
            last_input = Some(event.mono_ms);
        }
        let now = clock.reading().mono_ms;
        clock.advance(Duration::from_millis(u64::try_from(to - now).unwrap()));
        events_ms += by_events.from_events(&events, from, to).active_ms;
        spans_ms += by_spans
            .from_spans(&spans.drain().unwrap(), from, to)
            .active_ms;
        poll_ms += by_poll.from_last_input(last_input, from, to).active_ms;
        from = to;
    }
    (events_ms, spans_ms, poll_ms)
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(64)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: cases(),
        ..ProptestConfig::default()
    })]

    #[test]
    fn spans_and_events_count_the_same_active_time(
        gaps in prop::collection::vec(1_u64..4_000, 1..80),
        ticks in prop::collection::vec(100_u64..6_000, 1..8),
    ) {
        let (events, spans, poll) = measure(&gaps, &ticks);
        prop_assert_eq!(events, spans);
        // Polling only ever misses input.
        prop_assert!(poll <= events, "polling {} ms, events {} ms", poll, events);
    }
}

#[test]
fn polling_misses_bursts_that_spans_catch() {
    // Two inputs 900 ms apart at the start of every 3 seconds, ticks of one second: events
    // and spans see 1.9 s active in each 3, polling sees only the later input of each pair.
    let gaps: Vec<u64> = (0..20).flat_map(|_| [2_100, 900]).collect();
    let (events, spans, poll) = measure(&gaps, &[1_000]);
    assert_eq!(events, spans);
    assert_eq!(events, 20 * 1_900);
    assert_eq!(poll, 20 * 1_000);
}
