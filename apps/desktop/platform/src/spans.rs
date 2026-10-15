//! Active time as spans (E13-S04), where the OS reports when the member became active and when
//! they went idle, rather than each input: Wayland's `ext-idle-notify-v1`, and GNOME's
//! `IdleMonitor` watches.
//!
//! The notifications use a one-second timeout ([`IDLE_AFTER_MS`]): the member goes idle one
//! second after their last input. A span then runs from the first input after idle to one second
//! after the last input of the burst, which is exactly the time the event meter counts as active
//! from the same inputs ("an input makes the second after it active"). The figure is combined:
//! notifications don't say whether the input was the keyboard or the mouse.
//!
//! [`Transitions`] turns the notifications into spans and idle time for both adapters.

// Only the Linux adapters and the fakes drive the transitions; on macOS and Windows parts of
// them go unused until their span sources exist.
#![cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "used by the Linux adapters and the fakes only")
)]

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use crate::{IdleError, SensorError};

/// How long without input the notifications wait before reporting the member idle. It equals
/// the event meter's "one active second after each input", so spans and events agree.
pub const IDLE_AFTER_MS: i64 = 1_000;

/// Two moments this close are the same one: the compositor's clock and the agent's differ by a
/// few milliseconds. An idle notice this soon after watching began means the member was idle
/// all along, and a last input this close to the one known isn't a new input.
const SLACK_MS: i64 = 100;

/// Spans kept until the agent drains them, beyond which the oldest are dropped: hours of
/// stop-and-go input if the agent stops draining.
const MAX_ENDED: usize = 10_000;

/// A stretch of activity, in the clock's monotonic milliseconds of the current boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveSpan {
    /// The first input of the burst.
    pub start_mono: i64,
    /// One timeout after the last input of the burst. `None` while the member is still active.
    pub end_mono: Option<i64>,
}

/// Active time from notifications. See the module documentation.
pub trait ActivitySpans: Send + Sync + fmt::Debug {
    /// The spans that ended since the previous call, oldest first, and then the open span, if
    /// the member is active now. An open span is returned again on each call until it ends, and
    /// then once more with its end. Spans are kept only until they are returned.
    ///
    /// # Errors
    ///
    /// Fails when this session has no such notifications, or the reader failed.
    fn drain(&self) -> Result<Vec<ActiveSpan>, SensorError>;
}

/// What the notifications have said so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    /// Nothing yet.
    #[default]
    Unknown,
    /// Watching since this moment, with no notification yet. `ext-idle-notify` says nothing
    /// until the member has been idle for a timeout, so this is active time unless the first
    /// notification says idle one timeout after watching began.
    Watching(i64),
    /// Active since this moment.
    Active(i64),
    /// Idle since the last input, at this moment.
    Idle(i64),
}

/// Turns active and idle notifications into [`ActiveSpan`]s and idle time.
#[derive(Debug, Default)]
pub(crate) struct Transitions {
    state: State,
    ended: VecDeque<ActiveSpan>,
}

impl Transitions {
    /// The adapter starts watching at `mono`, knowing nothing yet.
    pub(crate) fn watching(&mut self, mono: i64) {
        self.state = State::Watching(mono);
    }

    /// An input at `mono` after idle time: a span begins, unless one is open.
    pub(crate) fn active(&mut self, mono: i64) {
        if !matches!(self.state, State::Active(_)) {
            self.state = State::Active(mono);
        }
    }

    /// The member's last input was at `last_input`, and there has been none since: the open
    /// span ends one timeout after it. Idle again with a later last input means an input came
    /// and went unnoticed in between; it counts its one second.
    pub(crate) fn idle_since(&mut self, last_input: i64) {
        let end = last_input.saturating_add(IDLE_AFTER_MS);
        match self.state {
            State::Active(start) => self.end(start, end),
            State::Watching(since) if last_input > since + SLACK_MS => self.end(since, end),
            State::Idle(since) if last_input > since + SLACK_MS => self.end(last_input, end),
            // The same idle period, reported again.
            State::Idle(_) => return,
            State::Unknown | State::Watching(_) => {}
        }
        self.state = State::Idle(last_input);
    }

    fn end(&mut self, start: i64, end: i64) {
        if end > start {
            if self.ended.len() >= MAX_ENDED {
                self.ended.pop_front();
            }
            self.ended.push_back(ActiveSpan {
                start_mono: start,
                end_mono: Some(end),
            });
        }
    }

    /// The spans ended since the previous call, then the open one, at `now`.
    pub(crate) fn drain(&mut self, now: i64) -> Vec<ActiveSpan> {
        self.settle(now);
        let mut spans: Vec<ActiveSpan> = self.ended.drain(..).collect();
        if let State::Active(start) = self.state {
            spans.push(ActiveSpan {
                start_mono: start,
                end_mono: None,
            });
        }
        spans
    }

    /// How long since the last input, at `now`: none while active.
    ///
    /// # Errors
    ///
    /// [`IdleError::Failed`] until the first notification, or the first timeout of watching.
    pub(crate) fn idle_for(&mut self, now: i64) -> Result<Duration, IdleError> {
        self.settle(now);
        match self.state {
            State::Active(_) => Ok(Duration::ZERO),
            State::Idle(since) => Ok(Duration::from_millis(
                u64::try_from(now.saturating_sub(since)).unwrap_or(0),
            )),
            State::Unknown | State::Watching(_) => {
                Err(IdleError::Failed("idle time isn't known yet".to_owned()))
            }
        }
    }

    /// Watching for longer than a timeout without hearing "idle" means the member is active,
    /// and has been since watching began.
    fn settle(&mut self, now: i64) {
        if let State::Watching(since) = self.state
            && now > since + IDLE_AFTER_MS + SLACK_MS
        {
            self.state = State::Active(since);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{ActiveSpan, IDLE_AFTER_MS, MAX_ENDED, Transitions};

    const fn ended(start_mono: i64, end_mono: i64) -> ActiveSpan {
        ActiveSpan {
            start_mono,
            end_mono: Some(end_mono),
        }
    }

    const fn open(start_mono: i64) -> ActiveSpan {
        ActiveSpan {
            start_mono,
            end_mono: None,
        }
    }

    /// The idle notification for a last input at `last`, as the compositor sends it.
    fn idled(transitions: &mut Transitions, last: i64) {
        transitions.idle_since(last);
    }

    #[test]
    fn a_burst_is_one_span_from_its_first_input_to_a_second_after_its_last() {
        let mut t = Transitions::default();
        t.idle_since(0);
        t.active(5_000);
        assert_eq!(t.drain(5_400), [open(5_000)]);
        assert_eq!(t.idle_for(5_400), Ok(Duration::ZERO));
        // Still open: returned again.
        assert_eq!(t.drain(6_000), [open(5_000)]);
        idled(&mut t, 7_200);
        assert_eq!(t.drain(8_500), [ended(5_000, 8_200)]);
        assert_eq!(t.drain(9_000), []);
        assert_eq!(t.idle_for(9_000), Ok(Duration::from_millis(1_800)));
    }

    #[test]
    fn spans_ended_between_drains_are_all_returned_oldest_first() {
        let mut t = Transitions::default();
        t.idle_since(0);
        t.active(1_000);
        idled(&mut t, 1_500);
        t.active(4_000);
        idled(&mut t, 4_000);
        t.active(9_000);
        assert_eq!(
            t.drain(9_100),
            [ended(1_000, 2_500), ended(4_000, 5_000), open(9_000)]
        );
    }

    #[test]
    fn watching_is_active_time_unless_the_first_notice_says_idle_at_once() {
        // Idle from the start: the notice comes one timeout after watching began.
        let mut t = Transitions::default();
        t.watching(10_000);
        assert!(t.idle_for(10_500).is_err());
        assert_eq!(t.drain(10_500), []);
        idled(&mut t, 10_000);
        assert_eq!(t.drain(11_100), []);
        assert_eq!(t.idle_for(13_000), Ok(Duration::from_secs(3)));
        // Active from the start, then idle: the watched time counts.
        let mut t = Transitions::default();
        t.watching(10_000);
        idled(&mut t, 12_000);
        assert_eq!(t.drain(13_500), [ended(10_000, 13_000)]);
        // Active, and no notice after a timeout: active since watching began.
        let mut t = Transitions::default();
        t.watching(10_000);
        assert_eq!(t.drain(11_000), []);
        assert_eq!(t.drain(11_200), [open(10_000)]);
        assert_eq!(t.idle_for(11_200), Ok(Duration::ZERO));
    }

    #[test]
    fn a_lone_input_between_two_idle_notices_counts_its_second() {
        // GNOME: the input that ended an idle period was missed, but the idle watch fired again.
        let mut t = Transitions::default();
        t.idle_since(1_000);
        idled(&mut t, 6_000);
        assert_eq!(t.drain(7_500), [ended(6_000, 6_000 + IDLE_AFTER_MS)]);
        // The same idle period reported again, by a clock a little off, is no input.
        idled(&mut t, 6_000);
        idled(&mut t, 6_050);
        idled(&mut t, 2_000);
        assert_eq!(t.drain(9_000), []);
        assert_eq!(t.idle_for(9_000), Ok(Duration::from_secs(3)));
    }

    #[test]
    fn idle_time_is_unknown_until_the_first_notice() {
        let mut t = Transitions::default();
        assert!(t.idle_for(0).is_err());
        assert_eq!(t.drain(0), []);
        t.active(100);
        assert_eq!(t.idle_for(200), Ok(Duration::ZERO));
    }

    #[test]
    fn spans_waiting_too_long_are_dropped_oldest_first() {
        let mut t = Transitions::default();
        let count = i64::try_from(MAX_ENDED).unwrap() + 5;
        for n in 0..count {
            t.active(n * 10_000);
            idled(&mut t, n * 10_000);
        }
        let spans = t.drain(count * 10_000);
        assert_eq!(spans.len(), MAX_ENDED);
        assert_eq!(spans[0], ended(50_000, 51_000));
    }
}
