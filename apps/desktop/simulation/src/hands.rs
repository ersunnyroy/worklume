//! The member's hands, and a jiggler's: input events for the fake reader (E09-S13).
//!
//! A person types unevenly, clicks now and then, and moves the mouse in short bursts of
//! reports. A jiggler moves the pointer on a fixed schedule. Every event lands strictly inside
//! the tick it belongs to, and a movement or jiggle that would start within the movement gap
//! of the previous report of its kind is left out, so the [`InputLedger`] counts exactly the
//! inputs the agent counts when it sees every event, and never fewer than it counts when it
//! sees only some.

use worklume_core::input::MOVEMENT_GAP_MS;
use worklume_platform::{InputEvent, InputKind};

/// Inputs made, per kind, as the agent counts them: key presses, clicks, pointer movements and
/// scroll gestures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputLedger {
    pub key: u64,
    pub pointer: u64,
    pub click: u64,
    pub scroll: u64,
}

impl InputLedger {
    /// All inputs.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.key + self.pointer + self.click + self.scroll
    }

    pub(crate) const fn add(&mut self, kind: InputKind) {
        match kind {
            InputKind::Key => self.key += 1,
            InputKind::Pointer => self.pointer += 1,
            InputKind::Click => self.click += 1,
            InputKind::Scroll => self.scroll += 1,
        }
    }
}

/// Reports 8 ms apart: a mouse polled at 125 Hz.
const POINTER_REPORT_MS: i64 = 8;
const SCROLL_REPORT_MS: i64 = 16;

/// Makes input events, deterministically: the same script always makes the same events.
#[derive(Debug, Clone)]
pub(crate) struct Hands {
    noise: u64,
    /// The latest pointer and scroll report, to keep movements apart.
    last_report: [Option<i64>; 2],
    ledger: InputLedger,
}

impl Default for Hands {
    fn default() -> Self {
        Self {
            noise: 0x9E37_79B9_7F4A_7C15,
            last_report: [None; 2],
            ledger: InputLedger::default(),
        }
    }
}

impl Hands {
    pub(crate) const fn ledger(&self) -> InputLedger {
        self.ledger
    }

    /// A person working from `from_ms` to `to_ms`, in one boot's monotonic time: keys and
    /// clicks at uneven times, maybe a mouse movement and a scroll. Oldest first.
    pub(crate) fn person(&mut self, from_ms: i64, to_ms: i64) -> Vec<InputEvent> {
        self.forget_other_boot(from_ms);
        let span = to_ms - from_ms;
        if span < 2 {
            return Vec::new();
        }
        let seconds = u64::try_from(span / 1_000).unwrap_or(0);
        let mut events = Vec::new();
        for _ in 0..self.below(2 * seconds + 2) {
            events.push(self.single(from_ms, span, InputKind::Key));
        }
        for _ in 0..self.below(seconds / 2 + 2) {
            events.push(self.single(from_ms, span, InputKind::Click));
        }
        if self.below(2) == 0 {
            let reports = 3 + self.below(12);
            self.movement(&mut events, from_ms, span, InputKind::Pointer, reports);
        }
        if self.below(6) == 0 {
            let reports = 1 + self.below(4);
            self.movement(&mut events, from_ms, span, InputKind::Scroll, reports);
        }
        events.sort_by_key(|event| event.mono_ms);
        events
    }

    /// A jiggler moving the pointer every `every_ms`, next at `*next_ms`, between `from_ms`
    /// and `to_ms`. Oldest first.
    pub(crate) fn jiggler(
        &mut self,
        from_ms: i64,
        to_ms: i64,
        next_ms: &mut i64,
        every_ms: i64,
    ) -> Vec<InputEvent> {
        self.forget_other_boot(from_ms);
        let mut events = Vec::new();
        while *next_ms <= to_ms {
            if *next_ms > from_ms && self.apart(InputKind::Pointer, *next_ms) {
                self.report(&mut events, InputKind::Pointer, *next_ms, true);
            }
            *next_ms += every_ms.max(1);
        }
        events
    }

    fn single(&mut self, from_ms: i64, span: i64, kind: InputKind) -> InputEvent {
        self.ledger.add(kind);
        InputEvent {
            mono_ms: from_ms + 1 + self.offset(span - 1),
            kind,
        }
    }

    /// A burst of `reports`, inside the span, unless it would join the previous movement.
    fn movement(
        &mut self,
        events: &mut Vec<InputEvent>,
        from_ms: i64,
        span: i64,
        kind: InputKind,
        reports: u64,
    ) {
        let step = if kind == InputKind::Pointer {
            POINTER_REPORT_MS
        } else {
            SCROLL_REPORT_MS
        };
        let length = step * i64::try_from(reports).unwrap_or(1);
        if span <= length + 1 {
            return;
        }
        let start = from_ms + 1 + self.offset(span - length - 1);
        if !self.apart(kind, start) {
            return;
        }
        let mut at = start;
        for report in 0..reports {
            self.report(events, kind, at, report == 0);
            at += step;
        }
    }

    /// Whether a report at `mono_ms` starts a new movement.
    fn apart(&self, kind: InputKind, mono_ms: i64) -> bool {
        self.last_report[slot(kind)].is_none_or(|last| mono_ms - last >= MOVEMENT_GAP_MS)
    }

    fn report(&mut self, events: &mut Vec<InputEvent>, kind: InputKind, mono_ms: i64, first: bool) {
        if first {
            self.ledger.add(kind);
        }
        self.last_report[slot(kind)] = Some(mono_ms);
        events.push(InputEvent { mono_ms, kind });
    }

    /// A monotonic time before the last report means a new boot.
    fn forget_other_boot(&mut self, from_ms: i64) {
        for last in &mut self.last_report {
            if last.is_some_and(|last| last > from_ms) {
                *last = None;
            }
        }
    }

    /// An offset in `0..span`.
    fn offset(&mut self, span: i64) -> i64 {
        let span = u64::try_from(span.max(1)).unwrap_or(1);
        i64::try_from(self.below(span)).unwrap_or(0)
    }

    /// A number in `0..bound` (xorshift64*).
    const fn below(&mut self, bound: u64) -> u64 {
        let mut x = self.noise;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.noise = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D) % if bound == 0 { 1 } else { bound }
    }
}

const fn slot(kind: InputKind) -> usize {
    match kind {
        InputKind::Scroll => 1,
        _ => 0,
    }
}
