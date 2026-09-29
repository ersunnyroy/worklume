//! 10-minute activity slots (`activity_slots`): one per device and UTC window (:00, :10, …),
//! closed only once its window has passed. The server keeps one row per member, device and
//! window, so a timer stopped and restarted within a window adds to the same slot.
//!
//! Durations accumulate in milliseconds. Seconds are rounded on running totals rather than per
//! slot, so however many slots a day has, their seconds add up to the tracked time within one
//! second (the E09-S04 acceptance criterion).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use worklume_storage::NewEvent;

use super::TrackerError;
use super::events::{self, fields};
use super::state::{Entry, WorkTarget};
use crate::time::{Moment, rfc3339_utc};

/// Length of a slot.
pub const SLOT_MS: i64 = 600_000;
const SLOT_MS_U64: u64 = 600_000;

/// How the platform measures input, reported with each slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMethod {
    /// Per-event input hooks (macOS event tap, Windows raw input, X11 `XInput2`).
    Hooks,
    /// Polling the OS idle time.
    IdlePoll,
    /// A desktop portal or compositor interface (Wayland).
    Portal,
}

impl InputMethod {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Hooks => "hooks",
            Self::IdlePoll => "idle_poll",
            Self::Portal => "portal",
        }
    }
}

/// Input measured since the previous tick, from the platform's activity source. Keyboard and
/// mouse are `None` when the OS cannot tell them apart (Wayland).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activity {
    pub active_ms: u64,
    pub keyboard_ms: Option<u64>,
    pub mouse_ms: Option<u64>,
}

impl Activity {
    /// No input.
    pub const NONE: Self = Self {
        active_ms: 0,
        keyboard_ms: Some(0),
        mouse_ms: Some(0),
    };
}

/// A slot still accumulating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OpenSlot {
    slot_id: Uuid,
    window_start: i64,
    /// The boot and wall-to-monotonic offset of the anchor the slot's times were derived from,
    /// to date its `slot.closed` event.
    boot_id: String,
    wall_to_mono: i64,
    tracked_ms: u64,
    active_ms: u64,
    idle_ms: u64,
    keyboard_ms: u64,
    mouse_ms: u64,
    /// Some input in this slot came without a keyboard/mouse split.
    split_missing: bool,
    clock_adjusted: bool,
    /// Tracked time per entry, to attribute the slot to the entry with the most.
    shares: Vec<Share>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Share {
    time_entry_id: Uuid,
    target: WorkTarget,
    ms: u64,
}

/// Rounds a stream of millisecond amounts to whole seconds so that the seconds handed out
/// always equal the floor of the running total.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Rounder {
    ms: u64,
    seconds: u64,
}

impl Rounder {
    fn take(&mut self, ms: u64) -> u64 {
        self.ms = self.ms.saturating_add(ms);
        let total = self.ms / 1_000;
        let seconds = total - self.seconds;
        self.seconds = total;
        seconds
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Totals {
    tracked: Rounder,
    active: Rounder,
    idle: Rounder,
    keyboard: Rounder,
    mouse: Rounder,
}

/// One tick's worth of time to add to slots.
pub(crate) struct Interval<'a> {
    /// Derived wall time at the start of the interval.
    pub(crate) from_wall_ms: i64,
    pub(crate) elapsed_ms: u64,
    pub(crate) activity: Activity,
    pub(crate) entry: &'a Entry,
    pub(crate) idle: bool,
    pub(crate) clock_adjusted: bool,
    pub(crate) anchor: &'a Moment,
}

/// The open slots and the rounding totals.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Slots {
    open: Vec<OpenSlot>,
    totals: Totals,
}

impl Slots {
    /// Adds tracked time, split at window boundaries. Activity is clamped to the elapsed time
    /// and shared out in proportion.
    pub(crate) fn account(&mut self, interval: &Interval<'_>) {
        let elapsed = interval.elapsed_ms;
        if elapsed == 0 {
            return;
        }
        let active = interval.activity.active_ms.min(elapsed);
        let keyboard = interval.activity.keyboard_ms.map(|ms| ms.min(elapsed));
        let mouse = interval.activity.mouse_ms.map(|ms| ms.min(elapsed));
        let share = |amount: u64, piece: u64| -> u64 {
            let scaled = u128::from(amount) * u128::from(piece) / u128::from(elapsed);
            u64::try_from(scaled).unwrap_or(u64::MAX)
        };

        let end = interval
            .from_wall_ms
            .saturating_add(i64::try_from(elapsed).unwrap_or(i64::MAX));
        let mut cursor = interval.from_wall_ms;
        while cursor < end {
            let window_start = cursor.div_euclid(SLOT_MS) * SLOT_MS;
            let piece_end = end.min(window_start + SLOT_MS);
            let piece = u64::try_from(piece_end - cursor).unwrap_or(0);
            let slot = self.slot_for(window_start, interval.anchor);
            slot.tracked_ms += piece;
            slot.active_ms += share(active, piece);
            if interval.idle {
                slot.idle_ms += piece;
            }
            match (keyboard, mouse) {
                (Some(keyboard), Some(mouse)) => {
                    slot.keyboard_ms += share(keyboard, piece);
                    slot.mouse_ms += share(mouse, piece);
                }
                _ => slot.split_missing = true,
            }
            slot.clock_adjusted |= interval.clock_adjusted;
            match slot
                .shares
                .iter_mut()
                .find(|existing| existing.time_entry_id == interval.entry.time_entry_id)
            {
                Some(existing) => existing.ms += piece,
                None => slot.shares.push(Share {
                    time_entry_id: interval.entry.time_entry_id,
                    target: interval.entry.target,
                    ms: piece,
                }),
            }
            cursor = piece_end;
        }
    }

    /// Marks the slot open at `wall_ms`, if any, as affected by a clock change.
    pub(crate) fn flag_clock_adjusted(&mut self, wall_ms: i64) {
        let window_start = wall_ms.div_euclid(SLOT_MS) * SLOT_MS;
        if let Some(slot) = self
            .open
            .iter_mut()
            .find(|slot| slot.window_start == window_start)
        {
            slot.clock_adjusted = true;
        }
    }

    fn slot_for(&mut self, window_start: i64, anchor: &Moment) -> &mut OpenSlot {
        if !self
            .open
            .iter()
            .any(|slot| slot.window_start == window_start)
        {
            self.open.push(OpenSlot {
                slot_id: events::new_id(),
                window_start,
                boot_id: anchor.boot_id.clone(),
                wall_to_mono: anchor.mono_ms - anchor.wall_ms,
                tracked_ms: 0,
                active_ms: 0,
                idle_ms: 0,
                keyboard_ms: 0,
                mouse_ms: 0,
                split_missing: false,
                clock_adjusted: false,
                shares: Vec::new(),
            });
            self.open.sort_by_key(|slot| slot.window_start);
        }
        let index = self
            .open
            .iter()
            .position(|slot| slot.window_start == window_start)
            .unwrap_or(0);
        let slot = &mut self.open[index];
        // A slot that outlived a reboot is dated in the boot that last added to it.
        slot.boot_id.clone_from(&anchor.boot_id);
        slot.wall_to_mono = anchor.mono_ms - anchor.wall_ms;
        slot
    }

    /// Closes, in order, every slot whose window ended by `wall_ms`.
    pub(crate) fn close_due(
        &mut self,
        wall_ms: i64,
        input_method: InputMethod,
        out: &mut Vec<NewEvent>,
    ) -> Result<(), TrackerError> {
        while self
            .open
            .first()
            .is_some_and(|slot| slot.window_start + SLOT_MS <= wall_ms)
        {
            let slot = self.open.remove(0);
            self.close(&slot, input_method, out)?;
        }
        Ok(())
    }

    fn close(
        &mut self,
        slot: &OpenSlot,
        input_method: InputMethod,
        out: &mut Vec<NewEvent>,
    ) -> Result<(), TrackerError> {
        let totals = &mut self.totals;
        let tracked = totals.tracked.take(slot.tracked_ms);
        let active = totals.active.take(slot.active_ms).min(tracked);
        let idle = totals.idle.take(slot.idle_ms).min(tracked);
        let (keyboard, mouse) = if slot.split_missing {
            (Value::Null, Value::Null)
        } else {
            (
                json!(totals.keyboard.take(slot.keyboard_ms).min(tracked)),
                json!(totals.mouse.take(slot.mouse_ms).min(tracked)),
            )
        };
        // The entry with the most time owns the slot; on a tie, the later one (max_by_key keeps
        // the last maximum). A slot that rounds to no tracked time carries nothing worth a row.
        let Some(owner) = slot
            .shares
            .iter()
            .max_by_key(|share| share.ms)
            .filter(|_| tracked > 0)
        else {
            return Ok(());
        };
        let mut flags = Vec::new();
        if slot.tracked_ms < SLOT_MS_U64 {
            flags.push("partial_slot");
        }
        if slot.clock_adjusted {
            flags.push("clock_adjusted");
        }
        let window_end = slot.window_start + SLOT_MS;
        let at = Moment {
            wall_ms: window_end,
            mono_ms: window_end + slot.wall_to_mono,
            boot_id: slot.boot_id.clone(),
        };
        let payload = fields([
            ("slot_start", json!(rfc3339_utc(slot.window_start))),
            ("time_entry_id", json!(owner.time_entry_id)),
            ("project_id", json!(owner.target.project_id)),
            (
                "task_id",
                owner.target.task_id.map_or(Value::Null, |task| json!(task)),
            ),
            ("tracked_seconds", json!(tracked)),
            ("active_seconds", json!(active)),
            ("keyboard_seconds", keyboard),
            ("mouse_seconds", mouse),
            ("idle_seconds", json!(idle)),
            ("input_method", json!(input_method.as_str())),
            ("flags", json!(flags)),
        ]);
        out.push(events::slot_closed(slot.slot_id, &at, payload)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Rounder;

    #[test]
    fn rounding_never_drifts_by_a_second() {
        let mut rounder = Rounder::default();
        let mut handed_out = 0;
        let mut total_ms = 0;
        for ms in [599_999, 600_001, 1, 999, 1_000, 333, 333, 334, 12_345] {
            handed_out += rounder.take(ms);
            total_ms += ms;
            assert_eq!(handed_out, total_ms / 1_000);
        }
    }
}
