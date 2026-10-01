//! App and URL usage (E09-S12): what is kept at each privacy level, private windows (the
//! acceptance criterion), when usage counts, and how it reaches the outbox with its slot.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use common::rig::{Rig, policy};
use common::{Harness, seconds, target, uuid};
use serde_json::{Value, json};
use worklume_core::timekeeper::KeepIdle;
use worklume_core::tracker::{Activity, Observation};
use worklume_core::usage::{AppUse, PrivateWindows, UsageLevel, UsagePolicy, UsageSample};
use worklume_platform::{BrowserTab, ForegroundApp, SensorError};

const CHROME: &str = "com.google.Chrome";

fn usage(level: UsageLevel, window_titles: bool, private_windows: PrivateWindows) -> UsagePolicy {
    UsagePolicy {
        level,
        window_titles,
        private_windows,
    }
}

fn app(identifier: &str, name: &str, title: &str) -> ForegroundApp {
    ForegroundApp {
        identifier: identifier.to_owned(),
        name: name.to_owned(),
        title: Some(title.to_owned()),
    }
}

fn tab(url: &str, private: bool) -> BrowserTab {
    BrowserTab {
        browser: "Chrome".to_owned(),
        url: url.to_owned(),
        private,
    }
}

/// A timer running from 14:03 with `policy`, the member in `in_front` for the rest of the
/// 14:00 window, and every slot closed. Returns the rig.
fn one_window(
    policy_: UsagePolicy,
    in_front: ForegroundApp,
    browser_tab: Option<BrowserTab>,
) -> Rig {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(policy_);
    rig.browser.set(CHROME, browser_tab);
    rig.window.set(Some(in_front));
    rig.start();
    // 14:03 to 14:10, then the window closes.
    rig.work(7 * 60 + 1);
    rig
}

fn batch(rig: &Rig, kind: &str) -> Option<Value> {
    rig.harness
        .events_of(kind)
        .first()
        .map(|event| Value::Object(event.payload.clone()))
}

#[test]
fn nothing_is_kept_or_read_until_a_policy_allows_it() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.browser
        .set(CHROME, Some(tab("https://example.com/a", false)));
    rig.window
        .set(Some(app(CHROME, "Google Chrome", "Salaries - Sheets")));
    rig.start();
    rig.work(7 * 60 + 1);
    assert!(rig.harness.events_of("slot.closed").len() == 1);
    assert!(batch(&rig, "app_usage.batch").is_none());
    assert!(batch(&rig, "url_usage.batch").is_none());
    assert!(!rig.window.titles_asked());
    assert!(!rig.browser.was_read());
}

#[test]
fn apps_level_keeps_the_app_and_reads_no_browser() {
    let rig = one_window(
        usage(UsageLevel::Apps, false, PrivateWindows::Nothing),
        app(CHROME, "Google Chrome", "Salaries - Sheets"),
        Some(tab("https://docs.google.com/spreadsheets/d/1", false)),
    );
    let slot = rig.harness.events_of("slot.closed").remove(0);
    let apps = batch(&rig, "app_usage.batch").unwrap();
    assert_eq!(uuid(&apps["slot_id"]), *slot.event_id.as_uuid());
    assert_eq!(apps["slot_start"], "2026-09-21T14:00:00Z");
    assert_eq!(
        apps["apps"],
        json!([{"identifier": CHROME, "name": "Google Chrome", "title": null, "seconds": 420}])
    );
    assert!(batch(&rig, "url_usage.batch").is_none());
    assert!(
        !rig.browser.was_read(),
        "no tab read below the domains level"
    );
    assert!(
        !rig.window.titles_asked(),
        "no title read while titles are off"
    );
}

#[test]
fn domains_level_keeps_the_registrable_domain_only() {
    let rig = one_window(
        usage(UsageLevel::Domains, false, PrivateWindows::Nothing),
        app(CHROME, "Google Chrome", ""),
        Some(tab("https://mail.example.co.uk/inbox?id=42#msg", false)),
    );
    assert_eq!(
        batch(&rig, "url_usage.batch").unwrap()["sites"],
        json!([{
            "domain": "example.co.uk",
            "path": null,
            "browser": "chrome",
            "private_window": false,
            "seconds": 420,
        }])
    );
}

#[test]
fn paths_level_keeps_the_path_but_never_the_query() {
    let rig = one_window(
        usage(UsageLevel::Paths, false, PrivateWindows::Nothing),
        app(CHROME, "Google Chrome", ""),
        Some(tab(
            "https://github.com/acme/api/pull/7?token=secret#files",
            false,
        )),
    );
    let sites = batch(&rig, "url_usage.batch").unwrap()["sites"].clone();
    assert_eq!(sites[0]["domain"], "github.com");
    assert_eq!(sites[0]["path"], "/acme/api/pull/7");
}

#[test]
fn window_titles_are_kept_only_when_turned_on() {
    let rig = one_window(
        usage(UsageLevel::Apps, true, PrivateWindows::Nothing),
        app(
            "com.microsoft.VSCode",
            "Visual Studio Code",
            "  main.rs — worklume  ",
        ),
        None,
    );
    let apps = batch(&rig, "app_usage.batch").unwrap();
    assert_eq!(apps["apps"][0]["title"], "main.rs — worklume");
    assert!(rig.window.titles_asked());
}

#[test]
fn private_windows_follow_the_policy() {
    // Domain only: the domain, never the path or the window title, whatever else is on.
    let rig = one_window(
        usage(UsageLevel::Paths, true, PrivateWindows::DomainOnly),
        app(CHROME, "Google Chrome", "Private: Job offers - LinkedIn"),
        Some(tab("https://www.linkedin.com/jobs/view/1?ref=x", true)),
    );
    assert_eq!(
        batch(&rig, "url_usage.batch").unwrap()["sites"],
        json!([{
            "domain": "linkedin.com",
            "path": null,
            "browser": "chrome",
            "private_window": true,
            "seconds": 420,
        }])
    );
    assert_eq!(
        batch(&rig, "app_usage.batch").unwrap()["apps"][0]["title"],
        Value::Null
    );

    // Nothing: no website at all, and still no title; the browser counts as the app in front.
    let rig = one_window(
        usage(UsageLevel::Paths, true, PrivateWindows::Nothing),
        app(CHROME, "Google Chrome", "Private: Job offers - LinkedIn"),
        Some(tab("https://www.linkedin.com/jobs/view/1", true)),
    );
    assert!(batch(&rig, "url_usage.batch").is_none());
    assert_eq!(
        batch(&rig, "app_usage.batch").unwrap()["apps"],
        json!([{"identifier": CHROME, "name": "Google Chrome", "title": null, "seconds": 420}])
    );
}

#[test]
fn a_private_window_title_is_never_kept_even_at_the_apps_level() {
    // At `apps` no website is kept, but the tab is still read to learn the window is private.
    let rig = one_window(
        usage(UsageLevel::Apps, true, PrivateWindows::DomainOnly),
        app(CHROME, "Google Chrome", "Private: Job offers - LinkedIn"),
        Some(tab("https://www.linkedin.com/jobs", true)),
    );
    assert!(rig.browser.was_read());
    assert!(batch(&rig, "url_usage.batch").is_none());
    assert_eq!(
        batch(&rig, "app_usage.batch").unwrap()["apps"][0]["title"],
        Value::Null
    );
}

#[test]
fn a_title_whose_window_cannot_be_checked_is_dropped() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Paths, true, PrivateWindows::DomainOnly));
    rig.browser.set_error(Some(SensorError::PermissionDenied));
    rig.window.set(Some(app(
        CHROME,
        "Google Chrome",
        "Maybe private - LinkedIn",
    )));
    rig.start();
    rig.work(7 * 60 + 1);
    assert_eq!(
        batch(&rig, "app_usage.batch").unwrap()["apps"],
        json!([{"identifier": CHROME, "name": "Google Chrome", "title": null, "seconds": 420}])
    );
    assert!(batch(&rig, "url_usage.batch").is_none());
}

#[test]
fn addresses_that_are_not_web_pages_are_never_kept() {
    let rig = one_window(
        usage(UsageLevel::Paths, false, PrivateWindows::Nothing),
        app(CHROME, "Google Chrome", ""),
        Some(tab("file:///Users/someone/Documents/salaries.xlsx", false)),
    );
    assert!(batch(&rig, "url_usage.batch").is_none());
}

#[test]
fn usage_counts_while_tracking_and_not_while_idle() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Apps, false, PrivateWindows::Nothing));
    rig.window
        .set(Some(app("com.apple.Terminal", "Terminal", "")));
    // Before the timer starts, nothing counts.
    rig.work(30);
    rig.start();
    // 14:03:30 to 14:04:30 working, then away: idle is detected at 14:09:30.
    rig.work(60);
    rig.away(8 * 60);

    let slot = rig.harness.events_of("slot.closed").remove(0);
    assert_eq!(seconds(&slot, "tracked_seconds"), 390);
    let apps = batch(&rig, "app_usage.batch").unwrap();
    assert_eq!(
        apps["apps"][0]["seconds"], 360,
        "the minute of work and the five minutes before idle was detected, not the idle time"
    );
}

#[test]
fn the_tracker_itself_counts_no_usage_while_idle() {
    // Called directly, with a sample on every tick: the tracker still leaves idle time out.
    let mut harness = Harness::new();
    let mut tracker = harness.tracker();
    let sample = UsageSample {
        app: Some(AppUse {
            identifier: "com.apple.Terminal".to_owned(),
            name: "Terminal".to_owned(),
            title: None,
        }),
        site: None,
    };
    let tick = |harness: &mut Harness, tracker: &mut worklume_core::tracker::Tracker, n| {
        for _ in 0..n {
            let now = harness.clock.advance_secs(1);
            let observed = Observation {
                usage: &sample,
                input: &[],
            };
            tracker.tick_with(&now, Activity::NONE, &observed).unwrap();
        }
        harness.clock.now()
    };
    tracker.start(target(1), &harness.clock.now()).unwrap();
    let since = tick(&mut harness, &mut tracker, 60);
    let now = tick(&mut harness, &mut tracker, 60);
    tracker.idle_detected(&since, &now).unwrap();
    tick(&mut harness, &mut tracker, 301);

    let apps = harness.events_of("app_usage.batch").remove(0);
    assert_eq!(
        apps.payload["apps"][0]["seconds"], 120,
        "not the 300 s pending as idle"
    );
}

#[test]
fn usage_follows_the_app_in_front() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Domains, false, PrivateWindows::Nothing));
    rig.browser
        .set(CHROME, Some(tab("https://github.com/acme", false)));
    rig.start();
    rig.window
        .set(Some(app("com.apple.Terminal", "Terminal", "")));
    rig.work(120);
    rig.window.set(Some(app(CHROME, "Google Chrome", "")));
    rig.work(240);
    // Nothing in front (the desktop), then an app the OS cannot read.
    rig.window.set(None);
    rig.work(30);
    rig.window.set_error(Some(SensorError::PermissionDenied));
    rig.work(31);

    let apps = batch(&rig, "app_usage.batch").unwrap();
    let seconds_of = |identifier: &str| {
        apps["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["identifier"] == identifier)
            .map(|entry| entry["seconds"].as_u64().unwrap())
    };
    assert_eq!(seconds_of("com.apple.Terminal"), Some(120));
    assert_eq!(seconds_of(CHROME), Some(240));
    assert_eq!(apps["apps"].as_array().unwrap().len(), 2);
    assert_eq!(
        batch(&rig, "url_usage.batch").unwrap()["sites"][0]["seconds"],
        240
    );
    // Tracking went on regardless.
    let slot = rig.harness.events_of("slot.closed").remove(0);
    assert_eq!(seconds(&slot, "tracked_seconds"), 420);
}

#[test]
fn usage_is_saved_with_the_slot_across_a_restart() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Apps, false, PrivateWindows::Nothing));
    rig.window
        .set(Some(app("com.apple.Terminal", "Terminal", "")));
    rig.start();
    // Snapshots every 30 s: the restart keeps the first 120 s.
    rig.work(125);
    rig.restart(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Apps, false, PrivateWindows::Nothing));
    rig.work(7 * 60);

    let apps = batch(&rig, "app_usage.batch").unwrap();
    let slot = rig.harness.events_of("slot.closed").remove(0);
    assert_eq!(
        apps["apps"][0]["seconds"],
        seconds(&slot, "tracked_seconds")
    );
    assert!(seconds(&slot, "tracked_seconds") >= 120 + 7 * 60 - 125 - 1);
}

#[test]
fn many_titles_lose_detail_but_keep_their_time() {
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Apps, true, PrivateWindows::Nothing));
    rig.start();
    for n in 0..70 {
        rig.window.set(Some(app(
            "com.apple.Safari",
            "Safari",
            &format!("Page {n}"),
        )));
        rig.work(6);
    }
    let apps = batch(&rig, "app_usage.batch").unwrap();
    let entries = apps["apps"].as_array().unwrap();
    let titled = entries
        .iter()
        .filter(|entry| entry["title"].is_string())
        .count();
    assert_eq!(titled, 50, "the first 50 titles are kept");
    let untitled = entries
        .iter()
        .find(|entry| entry["title"].is_null())
        .unwrap();
    assert_eq!(
        untitled["seconds"],
        (70 - 50) * 6,
        "the rest merge without a title"
    );
    let total: u64 = entries
        .iter()
        .map(|entry| entry["seconds"].as_u64().unwrap())
        .sum();
    assert_eq!(total, 420);
}

#[test]
fn the_slot_owner_is_the_usage_owner() {
    // Switching projects mid-window: usage goes with the slot, which the server joins by ID.
    let mut rig = Rig::new(policy(KeepIdle::Prompt));
    rig.set_usage_policy(usage(UsageLevel::Apps, false, PrivateWindows::Nothing));
    rig.window
        .set(Some(app("com.apple.Terminal", "Terminal", "")));
    rig.start();
    rig.work(60);
    rig.keeper().switch(target(2)).unwrap();
    rig.work(361);
    let slot = rig.harness.events_of("slot.closed").remove(0);
    let apps = batch(&rig, "app_usage.batch").unwrap();
    assert_eq!(uuid(&apps["slot_id"]), *slot.event_id.as_uuid());
    assert_eq!(apps["apps"][0]["seconds"], 420);
}
