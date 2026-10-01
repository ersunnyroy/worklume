//! What the server received, folded into time entries, idle periods, breaks and slots, with
//! every invariant the agent must keep.

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde_json::Value;

use crate::device::{Allowance, Span};
use crate::hands::InputLedger;
use worklume_core::input::{FEATURES_VERSION, MIN_INTERVALS};
use worklume_core::usage::UsageLevel;

/// A moment in an event: its wall time (RFC 3339 in the event) and its monotonic reading.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Point {
    wall_ms: i64,
    mono_ms: i64,
    boot_id: String,
}

#[derive(Debug)]
struct EntryRecord {
    start: Point,
    end: Option<Point>,
}

#[derive(Debug)]
struct IdleRecord {
    resolved: bool,
}

/// Totals of a checked timeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub events: usize,
    pub entries: usize,
    /// Tracked time by the entries' monotonic start and end, in milliseconds.
    pub entry_ms: i64,
    /// Tracked time by the slots, in seconds.
    pub slot_seconds: i64,
    pub slots: usize,
    pub idle_periods: usize,
    pub breaks: usize,
    /// `app_usage.batch` and `url_usage.batch` events.
    pub usage_batches: usize,
    /// Seconds of app usage across all slots.
    pub app_seconds: i64,
    /// Seconds of website usage across all slots.
    pub site_seconds: i64,
    /// Inputs the slots counted, per kind.
    pub inputs: InputLedger,
    /// Slots with input features.
    pub slots_with_input: usize,
}

/// The result of [`check`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimelineCheck {
    pub violations: Vec<String>,
    pub summary: Summary,
}

impl fmt::Display for TimelineCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{:?}", self.summary)?;
        for violation in &self.violations {
            writeln!(f, "- {violation}")?;
        }
        Ok(())
    }
}

/// Folds `events`, in the order the server stored them, and reports every broken invariant:
///
/// - Sequence numbers count up by one, and event IDs are unique.
/// - Within a boot, every event's wall time is its monotonic time plus one fixed offset: a
///   changed device clock never reaches event times.
/// - One time entry is open at a time; each stop closes the open entry, in the same boot and
///   not before it began.
/// - No entry overlaps a stretch in `untracked` (sleep, or no agent running).
/// - Idle periods begin inside a known entry, are resolved once, and one at most is pending at a
///   time. One that a pause ends holds at least a second. A `stop` resolution ends the open entry, which is a later one when the agent
///   restarted or a gap split the entry while the decision was pending.
/// - Breaks don't overlap tracking and end once.
/// - One slot per 10-minute window, with 1 to 600 tracked seconds, attributed to a known entry.
/// - Slots add up to the entries' tracked time within one second.
/// - At the end nothing is open: no entry, idle period or break.
/// - App and URL usage comes once per kind after its slot, adds up to no more than the slot's
///   tracked time, and holds no more detail than `allowance`: no titles or paths unless some
///   policy allowed them, never a query or fragment, never a path or title from a private
///   window.
/// - A slot's input features are consistent (counts, shares, intervals no more than inputs,
///   statistics only from enough intervals, within their ranges), and the slots count no more
///   inputs of any kind than were made.
#[must_use]
pub fn check(events: &[Value], untracked: &[Span], allowance: &Allowance) -> TimelineCheck {
    let mut fold = Fold {
        allowance: Some(allowance.clone()),
        ..Fold::default()
    };
    for event in events {
        fold.apply(event);
    }
    fold.finish(untracked)
}

#[derive(Default)]
struct Fold {
    violations: Vec<String>,
    summary: Summary,
    last_seq: Option<i64>,
    ids: HashSet<String>,
    offsets: HashMap<String, i64>,
    entries: HashMap<String, EntryRecord>,
    entry_order: Vec<String>,
    open_entry: Option<String>,
    idle: HashMap<String, IdleRecord>,
    pending_idle: Option<String>,
    /// Where the pending idle period began.
    pending_since: Option<Point>,
    open_break: Option<String>,
    slot_windows: HashSet<String>,
    /// Closed slots by ID: window, tracked seconds, and the usage batches seen for it.
    slots: HashMap<String, SlotRecord>,
    allowance: Option<Allowance>,
}

#[derive(Debug, Default)]
struct SlotRecord {
    window: String,
    tracked: i64,
    apps: bool,
    sites: bool,
}

impl Fold {
    fn apply(&mut self, event: &Value) {
        self.summary.events += 1;
        let kind = event["type"].as_str().unwrap_or_default().to_owned();
        let seq = event["seq"].as_i64().unwrap_or_default();
        let label = format!("seq {seq} {kind}");
        let id = event["event_id"].as_str().unwrap_or_default().to_owned();

        if let Some(last) = self.last_seq
            && seq != last + 1
        {
            self.fail(format!("{label}: follows seq {last}"));
        }
        self.last_seq = Some(seq);
        if !self.ids.insert(id.clone()) {
            self.fail(format!("{label}: event ID {id} stored twice"));
        }
        let Some(at) = point(event) else {
            self.fail(format!("{label}: unreadable time"));
            return;
        };
        let offset = at.wall_ms - at.mono_ms;
        match self.offsets.get(&at.boot_id) {
            Some(&known) if known != offset => self.fail(format!(
                "{label}: wall time is {} ms off this boot's monotonic time",
                offset - known
            )),
            Some(_) => {}
            None => {
                self.offsets.insert(at.boot_id.clone(), offset);
            }
        }

        let payload = &event["payload"];
        let text = |field: &str| payload[field].as_str().unwrap_or_default().to_owned();
        match kind.as_str() {
            "timer.started" => self.timer_started(&label, id, at),
            "timer.stopped" => {
                self.timer_stopped(&label, &text("time_entry_id"), &text("reason"), at);
            }
            "idle.detected" => self.idle_detected(&label, id, &text("time_entry_id"), &at),
            "idle.resolved" => {
                self.idle_resolved(&label, &text("idle_period_id"), &text("resolution"), at);
            }
            "break.started" => self.break_started(&label, id),
            "break.ended" => self.break_ended(&label, &text("break_id")),
            "slot.closed" => self.slot_closed(&label, &id, payload),
            "app_usage.batch" => self.app_usage(&label, payload),
            "url_usage.batch" => self.url_usage(&label, payload),
            other => self.fail(format!("{label}: unknown event type {other}")),
        }
    }

    fn timer_started(&mut self, label: &str, id: String, at: Point) {
        if let Some(open) = &self.open_entry {
            let message = format!("{label}: entry {open} is still open");
            self.fail(message);
        }
        if self.open_break.is_some() {
            self.fail(format!("{label}: during a break"));
        }
        self.summary.entries += 1;
        self.entry_order.push(id.clone());
        self.entries.insert(
            id.clone(),
            EntryRecord {
                start: at,
                end: None,
            },
        );
        self.open_entry = Some(id);
    }

    fn timer_stopped(&mut self, label: &str, entry_id: &str, reason: &str, at: Point) {
        if self.open_entry.as_deref() != Some(entry_id) {
            self.fail(format!("{label}: entry {entry_id} is not the open one"));
            return;
        }
        // A pause ends a pending idle period, which must hold some idle time: less than a
        // second rounds to nothing and only asks the member a pointless question.
        if matches!(reason, "sleep" | "session_locked")
            && let Some(since) = &self.pending_since
            && since.boot_id == at.boot_id
            && at.mono_ms - since.mono_ms < 1_000
        {
            self.fail(format!(
                "{label}: the pending idle period holds {} ms before the pause",
                at.mono_ms - since.mono_ms
            ));
        }
        self.close_entry(label, entry_id, at);
    }

    fn close_entry(&mut self, label: &str, entry_id: &str, at: Point) {
        let problem = self.entries.get_mut(entry_id).and_then(|entry| {
            let problem = if entry.start.boot_id != at.boot_id {
                Some("ends in another boot")
            } else if at.mono_ms < entry.start.mono_ms {
                Some("ends before it began")
            } else {
                None
            };
            entry.end = Some(at);
            problem
        });
        if let Some(problem) = problem {
            self.fail(format!("{label}: entry {entry_id} {problem}"));
        }
        self.open_entry = None;
    }

    fn idle_detected(&mut self, label: &str, id: String, entry_id: &str, at: &Point) {
        if let Some(pending) = &self.pending_idle {
            let message = format!("{label}: idle period {pending} is still pending");
            self.fail(message);
        }
        match self.entries.get(entry_id) {
            None => self.fail(format!("{label}: unknown entry {entry_id}")),
            Some(entry) => {
                let before_start = at.mono_ms < entry.start.mono_ms;
                let after_end = entry
                    .end
                    .as_ref()
                    .is_some_and(|end| at.mono_ms > end.mono_ms);
                if before_start || after_end {
                    self.fail(format!("{label}: idle begins outside entry {entry_id}"));
                }
            }
        }
        self.summary.idle_periods += 1;
        self.idle.insert(id.clone(), IdleRecord { resolved: false });
        self.pending_idle = Some(id);
        self.pending_since = Some(at.clone());
    }

    fn idle_resolved(&mut self, label: &str, idle_id: &str, resolution: &str, at: Point) {
        match self.idle.get_mut(idle_id) {
            None => {
                self.fail(format!("{label}: unknown idle period {idle_id}"));
                return;
            }
            Some(record) if record.resolved => {
                self.fail(format!("{label}: idle period {idle_id} resolved twice"));
                return;
            }
            Some(record) => record.resolved = true,
        }
        if self.pending_idle.as_deref() == Some(idle_id) {
            self.pending_idle = None;
            self.pending_since = None;
        }
        // `stop` stops the timer with no `timer.stopped`: the open entry ends here, whichever
        // it is. It is a later one than the idle period began in when the agent restarted or a
        // gap split the entry meanwhile.
        if resolution == "stop"
            && let Some(open) = self.open_entry.clone()
        {
            self.close_entry(label, &open, at);
        }
    }

    fn break_started(&mut self, label: &str, id: String) {
        if self.open_entry.is_some() {
            self.fail(format!("{label}: the timer still runs"));
        }
        if self.open_break.is_some() {
            self.fail(format!("{label}: a break is already open"));
        }
        self.summary.breaks += 1;
        self.open_break = Some(id);
    }

    fn break_ended(&mut self, label: &str, break_id: &str) {
        if self.open_break.as_deref() == Some(break_id) {
            self.open_break = None;
        } else {
            self.fail(format!("{label}: break {break_id} is not open"));
        }
    }

    fn slot_closed(&mut self, label: &str, id: &str, payload: &Value) {
        let window = payload["slot_start"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let tracked = payload["tracked_seconds"].as_i64().unwrap_or_default();
        let entry_id = payload["time_entry_id"].as_str().unwrap_or_default();
        if !self.slot_windows.insert(window.clone()) {
            self.fail(format!("{label}: a second slot for {window}"));
        }
        if parse_rfc3339_ms(&window).is_none_or(|ms| ms % 600_000 != 0) {
            self.fail(format!("{label}: {window} is not a 10-minute boundary"));
        }
        if !(1..=600).contains(&tracked) {
            self.fail(format!("{label}: {tracked} tracked seconds"));
        }
        if !self.entries.contains_key(entry_id) {
            self.fail(format!("{label}: unknown entry {entry_id}"));
        }
        self.input_features(label, &payload["input_features"]);
        self.summary.slots += 1;
        self.summary.slot_seconds += tracked;
        self.slots.insert(
            id.to_owned(),
            SlotRecord {
                window,
                tracked,
                ..SlotRecord::default()
            },
        );
    }

    /// Checks a usage batch's slot and total, and returns its records.
    fn usage_batch<'a>(&mut self, label: &str, payload: &'a Value, list: &str) -> &'a [Value] {
        self.summary.usage_batches += 1;
        let records = payload[list].as_array().map_or(&[][..], Vec::as_slice);
        let total: i64 = records
            .iter()
            .map(|record| record["seconds"].as_i64().unwrap_or(0))
            .sum();
        let slot_id = payload["slot_id"].as_str().unwrap_or_default();
        let window = payload["slot_start"].as_str().unwrap_or_default();
        let problem = match self.slots.get_mut(slot_id) {
            None => Some(format!("{label}: slot {slot_id} was not sent before it")),
            Some(slot) => {
                let seen = if list == "apps" {
                    &mut slot.apps
                } else {
                    &mut slot.sites
                };
                if *seen {
                    Some(format!("{label}: a second batch for slot {slot_id}"))
                } else if slot.window != window {
                    *seen = true;
                    Some(format!(
                        "{label}: slot_start {window} is not the slot's {}",
                        slot.window
                    ))
                } else if total > slot.tracked {
                    *seen = true;
                    Some(format!(
                        "{label}: {total} s of usage in a slot of {} s",
                        slot.tracked
                    ))
                } else {
                    *seen = true;
                    None
                }
            }
        };
        if let Some(problem) = problem {
            self.fail(problem);
        }
        if records.is_empty() {
            self.fail(format!("{label}: an empty batch"));
        }
        records
    }

    /// Checks a slot's `input_features` and adds its inputs to the summary.
    fn input_features(&mut self, label: &str, features: &Value) {
        if features.is_null() {
            return;
        }
        let problems = input_problems(features);
        for problem in problems {
            self.fail(format!("{label}: input features {problem}"));
        }
        let count = |kind: &str| features["events"][kind].as_u64().unwrap_or(0);
        let inputs = &mut self.summary.inputs;
        inputs.key += count("key");
        inputs.pointer += count("pointer");
        inputs.click += count("click");
        inputs.scroll += count("scroll");
        self.summary.slots_with_input += 1;
    }

    fn app_usage(&mut self, label: &str, payload: &Value) {
        let records = self.usage_batch(label, payload, "apps");
        let allowance = self.allowance.clone().unwrap_or_default().usage;
        for record in records {
            self.summary.app_seconds += record["seconds"].as_i64().unwrap_or(0);
            let title = record["title"].as_str();
            if allowance.level < UsageLevel::Apps {
                self.fail(format!(
                    "{label}: app usage under level {:?}",
                    allowance.level
                ));
            }
            if let Some(title) = title {
                if !allowance.window_titles {
                    self.fail(format!("{label}: title {title:?} kept with titles off"));
                }
                if allowance.private_titles.contains(title) {
                    self.fail(format!("{label}: title {title:?} of a private window kept"));
                }
            }
        }
    }

    fn url_usage(&mut self, label: &str, payload: &Value) {
        let records = self.usage_batch(label, payload, "sites");
        let allowance = self.allowance.clone().unwrap_or_default().usage;
        for record in records {
            self.summary.site_seconds += record["seconds"].as_i64().unwrap_or(0);
            let private = record["private_window"].as_bool().unwrap_or(false);
            let path = record["path"].as_str();
            if allowance.level < UsageLevel::Domains {
                self.fail(format!(
                    "{label}: site usage under level {:?}",
                    allowance.level
                ));
            }
            if private && !allowance.private_sites {
                self.fail(format!("{label}: a private window's site kept"));
            }
            if let Some(path) = path {
                if allowance.level < UsageLevel::Paths {
                    self.fail(format!("{label}: path {path:?} kept below the paths level"));
                }
                if private {
                    self.fail(format!("{label}: path {path:?} of a private window kept"));
                }
                if path.contains(['?', '#']) || !path.starts_with('/') {
                    self.fail(format!("{label}: path {path:?} is not a bare path"));
                }
            }
        }
    }

    fn finish(mut self, untracked: &[Span]) -> TimelineCheck {
        if let Some(open) = self.open_entry.clone() {
            self.fail(format!("entry {open} is still open at the end"));
        }
        if let Some(pending) = self.pending_idle.clone() {
            self.fail(format!("idle period {pending} is still pending at the end"));
        }
        if let Some(open) = self.open_break.clone() {
            self.fail(format!("break {open} is still open at the end"));
        }
        let mut overlaps = Vec::new();
        for id in &self.entry_order {
            let Some(entry) = self.entries.get(id) else {
                continue;
            };
            let Some(end) = &entry.end else {
                continue;
            };
            self.summary.entry_ms += end.mono_ms - entry.start.mono_ms;
            for span in untracked {
                if span.boot_id == entry.start.boot_id
                    && entry.start.mono_ms < span.to_ms
                    && span.from_ms < end.mono_ms
                {
                    overlaps.push(format!(
                        "entry {id} tracks time while {} ({} ms of it)",
                        span.why,
                        end.mono_ms.min(span.to_ms) - entry.start.mono_ms.max(span.from_ms)
                    ));
                }
            }
        }
        self.violations.extend(overlaps);
        let difference = self.summary.slot_seconds * 1_000 - self.summary.entry_ms;
        if difference.abs() > 1_000 {
            self.violations.push(format!(
                "slots hold {} s but entries {} ms",
                self.summary.slot_seconds, self.summary.entry_ms
            ));
        }
        let made = self.allowance.as_ref().map(|allowance| allowance.inputs);
        let counted = self.summary.inputs;
        if let Some(made) = made
            && (counted.key > made.key
                || counted.pointer > made.pointer
                || counted.click > made.click
                || counted.scroll > made.scroll)
        {
            self.violations.push(format!(
                "slots count inputs {counted:?}, more than the {made:?} made"
            ));
        }
        TimelineCheck {
            violations: self.violations,
            summary: self.summary,
        }
    }

    fn fail(&mut self, violation: String) {
        self.violations.push(violation);
    }
}

const INPUT_KINDS: [&str; 4] = ["key", "pointer", "click", "scroll"];

/// What is wrong with an `input_features` object.
fn input_problems(features: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    if features["version"].as_u64() != Some(u64::from(FEATURES_VERSION)) {
        problems.push(format!("have version {}", features["version"]));
    }
    let mut events = HashMap::new();
    for kind in INPUT_KINDS {
        match features["events"][kind].as_u64() {
            Some(count) => {
                events.insert(kind, count);
            }
            None => problems.push(format!("lack a count of {kind} inputs")),
        }
    }
    let total: u64 = events.values().sum();
    if total == 0 {
        problems.push("count no input: they should be null".to_owned());
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "input counts in one slot stay far below 2^53"
    )]
    let share = events.get("key").copied().unwrap_or(0) as f64 / total.max(1) as f64;
    // To three decimals, as the agent rounds: a share of exactly 0.4375 is 0.438.
    let share = (share * 1_000.0).round() / 1_000.0;
    let reported = features["keyboard_share"].as_f64().unwrap_or(-1.0);
    if (reported - share).abs() > 1e-9 {
        problems.push(format!("give a keyboard share of {reported}, not {share}"));
    }
    for kind in std::iter::once("any").chain(INPUT_KINDS) {
        let intervals = &features["intervals"][kind];
        let most = if kind == "any" {
            total
        } else {
            events.get(kind).copied().unwrap_or(0)
        };
        if intervals.is_null() {
            continue;
        }
        let count = intervals["count"].as_u64().unwrap_or(0);
        if count == 0 || count > most {
            problems.push(format!("hold {count} {kind} intervals for {most} inputs"));
        }
        let enough = count >= u64::from(MIN_INTERVALS);
        let entropy = intervals["entropy_bits"].as_f64();
        let cv = intervals["cv"].as_f64();
        if !enough && (entropy.is_some() || cv.is_some()) {
            problems.push(format!(
                "give statistics from only {count} {kind} intervals"
            ));
        }
        if enough && entropy.is_none() {
            problems.push(format!("give no entropy for {count} {kind} intervals"));
        }
        if entropy.is_some_and(|bits| !(0.0..=4.0).contains(&bits)) {
            problems.push(format!("give {kind} intervals {entropy:?} bits"));
        }
        if cv.is_some_and(|cv| cv < 0.0) {
            problems.push(format!("give {kind} intervals a variation of {cv:?}"));
        }
    }
    problems
}

fn point(event: &Value) -> Option<Point> {
    Some(Point {
        wall_ms: parse_rfc3339_ms(event["wall_time"].as_str()?)?,
        mono_ms: event["mono_time"]["ms"].as_i64()?,
        boot_id: event["mono_time"]["boot_id"].as_str()?.to_owned(),
    })
}

/// Parses the API's UTC timestamps, `2026-09-21T09:00:00Z` with optional milliseconds, into
/// milliseconds since the Unix epoch.
#[must_use]
pub fn parse_rfc3339_ms(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let (time, millis) = match time.split_once('.') {
        Some((time, fraction)) if fraction.len() == 3 => (time, fraction.parse::<i64>().ok()?),
        Some(_) => return None,
        None => (time, 0),
    };
    let mut time = time.splitn(3, ':').map(str::parse::<i64>);
    let (hour, minute, second) = (time.next()?.ok()?, time.next()?.ok()?, time.next()?.ok()?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some((((days * 24 + hour) * 60 + minute) * 60 + second) * 1_000 + millis)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's `days_from_civil`).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use worklume_core::time::rfc3339_utc;

    use super::parse_rfc3339_ms;

    #[test]
    fn parses_what_the_agent_writes() {
        for ms in [
            0,
            951_782_400_000,
            1_789_981_200_000,
            1_790_000_400_123,
            4_102_444_799_999,
        ] {
            assert_eq!(parse_rfc3339_ms(&rfc3339_utc(ms)), Some(ms));
        }
        let mut ms = 0;
        while ms < 4_102_444_800_000 {
            assert_eq!(parse_rfc3339_ms(&rfc3339_utc(ms)), Some(ms));
            ms += 86_399_999;
        }
        for bad in [
            "2026-09-21T09:00:00",
            "2026-13-01T00:00:00Z",
            "2026-09-21T09:00:00.5Z",
        ] {
            assert_eq!(parse_rfc3339_ms(bad), None, "{bad}");
        }
    }
}
