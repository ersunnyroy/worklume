//! Input timing features (E09-S13): statistics about the rhythm of input in each slot, so the
//! server's automated-input detector (E33-S03) can tell a person from a mouse jiggler or an
//! auto-clicker without anything like a keystroke log.
//!
//! **One input** is a key press, a click, a pointer movement or a scroll gesture. Pointer and
//! scroll reports closer than [`MOVEMENT_GAP_MS`] to the previous one of the same kind continue
//! the same movement: a moving mouse reports its position many times a second at a steady rate,
//! and counting each report would make every hand look like a machine.
//!
//! **Intervals** are the times between consecutive inputs, of the same kind and of any kind.
//! They count only while the timer runs and the member isn't idle. One longer than
//! [`MAX_INTERVAL_MS`] is a pause rather than a rhythm and is left out. Whatever stops the count
//! (idle time, a pause, a break, a gap, a reboot, an agent restart) forgets the previous input,
//! so no interval spans it. An interval belongs to the slot of the input that ends it.
//!
//! **What is kept.** A slot keeps totals only: inputs per kind, and per interval kind a
//! histogram of 16 log2 buckets with the count, sum and sum of squares. An input's time is
//! forgotten once the next one is measured, and is never saved. **What is sent** is smaller
//! still (the `input_features` object of `slot.closed`, version [`FEATURES_VERSION`]): inputs
//! per kind, the share of key presses, and per interval kind the count, the entropy of the
//! histogram in bits and the coefficient of variation. Not the histogram: typing rhythm is a
//! biometric, so the device sends only what the detector needs. A jiggler or an auto-clicker
//! repeats one interval, so its entropy and variation are near zero; a person's are not.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use worklume_platform::InputKind;

/// Version of the `input_features` object.
pub const FEATURES_VERSION: u32 = 1;
/// Pointer or scroll reports closer than this to the previous one continue the same movement.
pub const MOVEMENT_GAP_MS: i64 = 250;
/// A longer interval is a pause, not a rhythm: it is not counted.
pub const MAX_INTERVAL_MS: i64 = 60_000;
/// Below this many intervals, entropy and variation say nothing and are sent as null.
pub const MIN_INTERVALS: u32 = 5;
/// Log2 buckets of interval milliseconds: bucket `b` holds `2^b ..= 2^(b+1) − 1` ms (bucket 0
/// also holds 0 ms). The last one reaches 65 535 ms, beyond [`MAX_INTERVAL_MS`].
const BUCKETS: usize = 16;

/// The kinds in the order the totals keep them, with their names in the payload.
const KINDS: [(InputKind, &str); 4] = [
    (InputKind::Key, "key"),
    (InputKind::Pointer, "pointer"),
    (InputKind::Click, "click"),
    (InputKind::Scroll, "scroll"),
];

const fn index(kind: InputKind) -> usize {
    match kind {
        InputKind::Key => 0,
        InputKind::Pointer => 1,
        InputKind::Click => 2,
        InputKind::Scroll => 3,
    }
}

/// Whether reports of this kind join into movements.
const fn moves(kind: InputKind) -> bool {
    matches!(kind, InputKind::Pointer | InputKind::Scroll)
}

/// Where the previous inputs were, to measure the next intervals. Held in memory only: after a
/// restart the count starts afresh.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Rhythm {
    /// Monotonic time of the previous input of each kind.
    last: [Option<i64>; 4],
    /// Monotonic time of the previous input of any kind.
    last_any: Option<i64>,
    /// Monotonic time of the latest pointer and scroll report, to join a movement's reports.
    last_report: [Option<i64>; 4],
}

/// An input, with the intervals it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counted {
    kind: InputKind,
    since_same: Option<i64>,
    since_any: Option<i64>,
}

impl Rhythm {
    /// Forgets the previous inputs: the next one ends no interval.
    pub(crate) fn forget(&mut self) {
        *self = Self::default();
    }

    /// Registers a report at `mono_ms`, which must not precede the previous one. Returns the
    /// input it makes, or `None` when it continues a movement.
    pub(crate) fn count(&mut self, kind: InputKind, mono_ms: i64) -> Option<Counted> {
        let at = index(kind);
        if moves(kind) {
            let previous = self.last_report[at].replace(mono_ms);
            if previous.is_some_and(|previous| mono_ms - previous < MOVEMENT_GAP_MS) {
                return None;
            }
        }
        Some(Counted {
            kind,
            since_same: self.last[at].replace(mono_ms).map(|p| mono_ms - p),
            since_any: self.last_any.replace(mono_ms).map(|p| mono_ms - p),
        })
    }
}

/// The input totals of one slot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InputStats {
    /// Inputs per kind, in the order of `KINDS`.
    events: [u32; 4],
    /// Intervals between inputs of any kind, then per kind in the order of `KINDS`.
    intervals: [Intervals; 5],
}

impl InputStats {
    pub(crate) fn is_empty(&self) -> bool {
        self.events.iter().all(|&count| count == 0)
    }

    pub(crate) fn add(&mut self, input: &Counted) {
        let at = index(input.kind);
        self.events[at] = self.events[at].saturating_add(1);
        if let Some(ms) = input.since_any {
            self.intervals[0].add(ms);
        }
        if let Some(ms) = input.since_same {
            self.intervals[at + 1].add(ms);
        }
    }

    /// The `input_features` object for `slot.closed`, or null when the slot had no input the
    /// agent could see (none, or a platform without input events: see `input_method`).
    pub(crate) fn features(&self) -> Value {
        let total = self
            .events
            .iter()
            .fold(0_u32, |sum, &count| sum.saturating_add(count));
        if total == 0 {
            return Value::Null;
        }
        let mut events = Map::new();
        let mut intervals = Map::new();
        intervals.insert("any".to_owned(), self.intervals[0].summary());
        for (at, (_, name)) in KINDS.iter().enumerate() {
            events.insert((*name).to_owned(), json!(self.events[at]));
            intervals.insert((*name).to_owned(), self.intervals[at + 1].summary());
        }
        json!({
            "version": FEATURES_VERSION,
            "events": events,
            "keyboard_share": rounded(f64::from(self.events[0]) / f64::from(total)),
            "intervals": intervals,
        })
    }
}

/// A histogram of intervals, with the sums for their mean and spread.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Intervals {
    buckets: [u32; BUCKETS],
    count: u32,
    sum_ms: u64,
    sum_squares: u64,
}

impl Intervals {
    fn add(&mut self, ms: i64) {
        let Ok(ms) = u64::try_from(ms) else {
            return;
        };
        if ms > MAX_INTERVAL_MS.unsigned_abs() {
            return;
        }
        let bucket = ms
            .checked_ilog2()
            .and_then(|log| usize::try_from(log).ok())
            .map_or(0, |log| log.min(BUCKETS - 1));
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
        self.count = self.count.saturating_add(1);
        self.sum_ms = self.sum_ms.saturating_add(ms);
        self.sum_squares = self.sum_squares.saturating_add(ms * ms);
    }

    /// `{count, entropy_bits, cv}`, or null without intervals.
    fn summary(&self) -> Value {
        if self.count == 0 {
            return Value::Null;
        }
        let enough = self.count >= MIN_INTERVALS;
        json!({
            "count": self.count,
            "entropy_bits": enough.then(|| rounded(self.entropy_bits())),
            "cv": self.variation().filter(|_| enough).map(rounded),
        })
    }

    /// Shannon entropy of the bucket shares: 0 when every interval falls in one bucket, at most
    /// 4 bits. Summed as `p · log2(1/p)`, whose terms are never negative, so a single bucket
    /// gives 0 rather than −0.
    fn entropy_bits(&self) -> f64 {
        let count = f64::from(self.count);
        self.buckets
            .iter()
            .filter(|&&bucket| bucket > 0)
            .map(|&bucket| {
                let share = f64::from(bucket) / count;
                share * (count / f64::from(bucket)).log2()
            })
            .sum()
    }

    /// Standard deviation over mean, or `None` when every interval was 0 ms.
    #[allow(
        clippy::cast_precision_loss,
        reason = "intervals ending in one slot span at most 11 minutes, so the sums stay far \
                  below 2^53"
    )]
    fn variation(&self) -> Option<f64> {
        let count = f64::from(self.count);
        let mean = self.sum_ms as f64 / count;
        if mean <= 0.0 {
            return None;
        }
        let variance = (self.sum_squares as f64 / count - mean * mean).max(0.0);
        Some(variance.sqrt() / mean)
    }
}

/// Three decimals: enough for the detector, and the same on every platform.
fn rounded(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use worklume_platform::InputKind;

    use super::{InputStats, Intervals, MAX_INTERVAL_MS, Rhythm};

    fn stats(reports: &[(InputKind, i64)]) -> InputStats {
        let mut rhythm = Rhythm::default();
        let mut stats = InputStats::default();
        for &(kind, mono_ms) in reports {
            if let Some(input) = rhythm.count(kind, mono_ms) {
                stats.add(&input);
            }
        }
        stats
    }

    #[test]
    fn a_steady_rhythm_has_no_entropy_or_variation() {
        let jiggler: Vec<_> = (0..20).map(|n| (InputKind::Pointer, n * 30_000)).collect();
        let features = stats(&jiggler).features();
        assert_eq!(features["events"]["pointer"], 20);
        assert_eq!(features["keyboard_share"], 0.0);
        assert_eq!(
            features["intervals"]["pointer"],
            json!({"count": 19, "entropy_bits": 0.0, "cv": 0.0})
        );
        assert_eq!(
            features["intervals"]["any"],
            features["intervals"]["pointer"]
        );
        assert_eq!(features["intervals"]["key"], serde_json::Value::Null);
        // Zero, not −0, which would read oddly on the server.
        assert!(!features.to_string().contains("-0"), "{features}");
    }

    #[test]
    fn a_movement_counts_once() {
        // Reports every 8 ms for a second, a pause, then another movement.
        let mut reports: Vec<_> = (0..125).map(|n| (InputKind::Pointer, n * 8)).collect();
        reports.extend((0..10).map(|n| (InputKind::Pointer, 3_000 + n * 8)));
        let features = stats(&reports).features();
        assert_eq!(features["events"]["pointer"], 2);
        assert_eq!(features["intervals"]["pointer"]["count"], 1);
        // A few intervals are too few to judge.
        assert_eq!(
            features["intervals"]["pointer"]["entropy_bits"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn keys_and_clicks_each_count() {
        let features = stats(&[
            (InputKind::Key, 0),
            (InputKind::Key, 40),
            (InputKind::Click, 50),
            (InputKind::Key, 60),
        ])
        .features();
        assert_eq!(
            features["events"],
            json!({"key": 3, "pointer": 0, "click": 1, "scroll": 0})
        );
        assert_eq!(features["keyboard_share"], 0.75);
        assert_eq!(features["intervals"]["any"]["count"], 3);
        assert_eq!(features["intervals"]["key"]["count"], 2);
        assert_eq!(features["intervals"]["click"], serde_json::Value::Null);
    }

    #[test]
    fn long_pauses_are_not_intervals() {
        let mut intervals = Intervals::default();
        intervals.add(MAX_INTERVAL_MS + 1);
        intervals.add(-5);
        assert_eq!(intervals.count, 0);
        intervals.add(MAX_INTERVAL_MS);
        intervals.add(0);
        intervals.add(1);
        assert_eq!(intervals.buckets[15], 1);
        assert_eq!(intervals.buckets[0], 2);
    }

    #[test]
    fn entropy_and_variation_are_measured() {
        // Two buckets, equally full: one bit.
        let mut intervals = Intervals::default();
        for ms in [100, 100, 100, 1_000, 1_000, 1_000] {
            intervals.add(ms);
        }
        let summary = intervals.summary();
        assert_eq!(summary["entropy_bits"], 1.0);
        // Mean 550, standard deviation 450.
        assert_eq!(summary["cv"], 0.818);
    }

    #[test]
    fn forgetting_starts_the_rhythm_afresh() {
        let mut rhythm = Rhythm::default();
        assert!(rhythm.count(InputKind::Key, 0).is_some());
        rhythm.forget();
        let input = rhythm.count(InputKind::Key, 100).unwrap();
        assert_eq!((input.since_same, input.since_any), (None, None));
    }

    #[test]
    fn no_input_sends_null() {
        assert_eq!(InputStats::default().features(), serde_json::Value::Null);
        assert!(InputStats::default().is_empty());
    }
}
