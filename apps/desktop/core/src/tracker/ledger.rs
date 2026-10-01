//! The member's tracked time per 10-minute window over the last two days, for the agent's
//! "today" total (E09-S09).
//!
//! Slots leave the device when they close, so the agent keeps this small ledger instead. It
//! counts time the way the server will once idle decisions are in:
//!
//! - Time counts as it is tracked.
//! - When idle is detected, the idle time already counted (from where inactivity began) becomes
//!   *pending*, and so does the time tracked while the decision waits.
//! - Keeping or reassigning the idle time keeps it; discarding it, or stopping, drops it.
//!
//! Within one entry the timer runs without gaps, and idle time never begins before its entry,
//! so the idle time already counted in a window is exactly the part of the window from where
//! inactivity began.
//!
//! Two days cover "today" in any time zone. A window counts towards a day by its start, so a
//! day that begins in the middle of a window (a time zone 15 or 45 minutes off the hour) is out
//! by at most one window's edge.

use serde::{Deserialize, Serialize};

use super::slots::SLOT_MS;

/// How long windows are kept.
const KEEP_MS: i64 = 48 * 3_600_000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ledger {
    /// Windows with tracked time, oldest first.
    windows: Vec<Window>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Window {
    start: i64,
    kept_ms: u64,
    /// Idle time awaiting the member's decision.
    #[serde(default, skip_serializing_if = "is_zero")]
    pending_ms: u64,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
const fn is_zero(ms: &u64) -> bool {
    *ms == 0
}

impl Ledger {
    /// Adds `ms` tracked in the window starting at `start`, pending when idle awaits a decision.
    pub(crate) fn add(&mut self, start: i64, ms: u64, pending: bool) {
        let window = self.window(start);
        if pending {
            window.pending_ms += ms;
        } else {
            window.kept_ms += ms;
        }
    }

    /// Idle was detected: the time counted from `from_wall_ms` to `to_wall_ms` now waits for the
    /// member's decision.
    pub(crate) fn hold(&mut self, from_wall_ms: i64, to_wall_ms: i64) {
        for window in &mut self.windows {
            let overlap = (window.start + SLOT_MS).min(to_wall_ms) - window.start.max(from_wall_ms);
            let Ok(overlap) = u64::try_from(overlap) else {
                continue;
            };
            let moved = overlap.min(window.kept_ms);
            window.kept_ms -= moved;
            window.pending_ms += moved;
        }
    }

    /// The decision about the pending idle time: keep it, or drop it.
    pub(crate) fn settle(&mut self, keep: bool) {
        for window in &mut self.windows {
            if keep {
                window.kept_ms += window.pending_ms;
            }
            window.pending_ms = 0;
        }
        self.windows
            .retain(|window| window.kept_ms > 0 || window.pending_ms > 0);
    }

    /// Time in windows starting at or after `from_wall_ms`, pending idle time included: until
    /// the member decides, it counts.
    pub(crate) fn since(&self, from_wall_ms: i64) -> u64 {
        self.windows
            .iter()
            .filter(|window| window.start >= from_wall_ms)
            .map(|window| window.kept_ms + window.pending_ms)
            .sum()
    }

    /// Forgets windows that ended more than two days before `now_wall_ms`, unless they hold
    /// pending time.
    pub(crate) fn prune(&mut self, now_wall_ms: i64) {
        self.windows.retain(|window| {
            window.pending_ms > 0 || window.start + SLOT_MS > now_wall_ms - KEEP_MS
        });
    }

    fn window(&mut self, start: i64) -> &mut Window {
        let at = match self
            .windows
            .binary_search_by_key(&start, |window| window.start)
        {
            Ok(at) => at,
            Err(at) => {
                self.windows.insert(
                    at,
                    Window {
                        start,
                        kept_ms: 0,
                        pending_ms: 0,
                    },
                );
                at
            }
        };
        &mut self.windows[at]
    }
}

#[cfg(test)]
mod tests {
    use super::{KEEP_MS, Ledger};
    use crate::tracker::SLOT_MS;

    const MINUTE: u64 = 60_000;

    #[test]
    fn idle_time_is_pending_until_decided() {
        let mut ledger = Ledger::default();
        // Ten minutes in each of two windows; idle began four minutes into the second.
        ledger.add(0, 10 * MINUTE, false);
        ledger.add(SLOT_MS, 10 * MINUTE, false);
        ledger.hold(SLOT_MS + 4 * 60_000, 2 * SLOT_MS);
        ledger.add(2 * SLOT_MS, 3 * MINUTE, true);
        assert_eq!(
            ledger.since(0),
            23 * MINUTE,
            "pending time counts until decided"
        );

        let mut discarded = ledger.clone();
        discarded.settle(false);
        assert_eq!(discarded.since(0), 14 * MINUTE);
        assert_eq!(discarded.windows.len(), 2, "the emptied window is gone");

        ledger.settle(true);
        assert_eq!(ledger.since(0), 23 * MINUTE);
        ledger.add(2 * SLOT_MS, MINUTE, false);
        ledger.settle(false);
        assert_eq!(
            ledger.since(0),
            24 * MINUTE,
            "a later decision never touches kept time"
        );
    }

    #[test]
    fn holding_never_takes_more_than_was_counted() {
        let mut ledger = Ledger::default();
        // Only two minutes tracked in the window, from its start.
        ledger.add(0, 2 * MINUTE, false);
        ledger.hold(-SLOT_MS, SLOT_MS);
        assert_eq!(ledger.windows[0].pending_ms, 2 * MINUTE);
        assert_eq!(ledger.windows[0].kept_ms, 0);
    }

    #[test]
    fn a_day_counts_the_windows_that_start_in_it() {
        let mut ledger = Ledger::default();
        for window in 0..5 {
            ledger.add(window * SLOT_MS, MINUTE, false);
        }
        assert_eq!(ledger.since(2 * SLOT_MS), 3 * MINUTE);
        assert_eq!(ledger.since(2 * SLOT_MS + 1), 2 * MINUTE);
    }

    #[test]
    fn two_days_are_kept() {
        let mut ledger = Ledger::default();
        ledger.add(0, MINUTE, false);
        ledger.add(SLOT_MS, MINUTE, true);
        ledger.add(KEEP_MS, MINUTE, false);
        ledger.prune(KEEP_MS + SLOT_MS);
        assert_eq!(
            ledger.windows.len(),
            2,
            "the old window goes; one with pending time stays until decided"
        );
        ledger.settle(true);
        ledger.prune(KEEP_MS + 2 * SLOT_MS + 1);
        assert_eq!(ledger.since(0), MINUTE);
    }
}
