//! Generated scripts: whatever the member, the device and the network do, the invariants hold.
//!
//! proptest shrinks a failing script to a short one before reporting it. Each case replays on
//! a fresh device, so a failure can be reproduced by pasting the printed script into a test.
//! `PROPTEST_CASES` sets the number of scripts (64 by default).

#![allow(
    clippy::unwrap_used,
    reason = "a harness error fails the property, like a broken invariant"
)]

use std::time::Duration;

use proptest::prelude::*;
use worklume_core::timekeeper::{IdlePolicy, KeepIdle};
use worklume_core::usage::{PrivateWindows, UsageLevel, UsagePolicy};
use worklume_simulation::{Answer, DeviceConfig, Focus, Step, run};

fn seconds(range: std::ops::RangeInclusive<u64>) -> impl Strategy<Value = Duration> {
    range.prop_map(Duration::from_secs)
}

fn policy() -> impl Strategy<Value = IdlePolicy> {
    (
        prop_oneof![
            Just(None),
            (60_u64..=900).prop_map(|s| Some(Duration::from_secs(s)))
        ],
        prop_oneof![
            Just(KeepIdle::Prompt),
            Just(KeepIdle::Always),
            Just(KeepIdle::Never)
        ],
    )
        .prop_map(|(timeout, keep)| IdlePolicy { timeout, keep })
}

fn usage_policy() -> impl Strategy<Value = UsagePolicy> {
    (
        prop_oneof![
            Just(UsageLevel::Off),
            Just(UsageLevel::Apps),
            Just(UsageLevel::Domains),
            Just(UsageLevel::Paths)
        ],
        any::<bool>(),
        prop_oneof![
            Just(PrivateWindows::DomainOnly),
            Just(PrivateWindows::Nothing)
        ],
    )
        .prop_map(|(level, window_titles, private_windows)| UsagePolicy {
            level,
            window_titles,
            private_windows,
        })
}

fn focus() -> impl Strategy<Value = Focus> {
    prop_oneof![
        3 => Just(Focus::Editor),
        4 => (1_u8..=5, any::<bool>()).prop_map(|(site, private)| Focus::Browser { site, private }),
        1 => Just(Focus::Desktop),
        1 => Just(Focus::Unreadable),
    ]
}

fn answer() -> impl Strategy<Value = Answer> {
    prop_oneof![
        Just(Answer::Keep),
        Just(Answer::Discard),
        (1_u8..=3).prop_map(Answer::Reassign),
        Just(Answer::Stop),
    ]
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (1_u8..=3).prop_map(Step::Start),
        2 => (1_u8..=3).prop_map(Step::Switch),
        2 => Just(Step::Stop),
        1 => Just(Step::StartBreak),
        1 => Just(Step::EndBreak),
        3 => answer().prop_map(Step::Answer),
        1 => Just(Step::Resume),
        6 => seconds(1..=1_200).prop_map(Step::Work),
        4 => seconds(1..=1_200).prop_map(Step::Away),
        1 => (seconds(1..=1_200), seconds(1..=90))
            .prop_map(|(duration, every)| Step::Jiggle { duration, every }),
        2 => seconds(1..=10_800).prop_map(Step::Sleep),
        1 => (-7_200_i64..=7_200).prop_map(Step::ShiftClock),
        1 => seconds(10..=600).prop_map(Step::Reboot),
        1 => seconds(1..=90).prop_map(Step::Crash),
        1 => Just(Step::CrashMidWrite),
        1 => Just(Step::Offline),
        1 => Just(Step::Online),
        1 => (1_u8..=3).prop_map(Step::LoseResponses),
        1 => policy().prop_map(Step::Policy),
        1 => usage_policy().prop_map(Step::UsagePolicy),
        4 => focus().prop_map(Step::Focus),
    ]
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
    fn every_script_keeps_the_invariants(
        steps in prop::collection::vec(step(), 1..40),
        policy in policy(),
        usage in usage_policy(),
    ) {
        let config = DeviceConfig {
            tick: Duration::from_secs(5),
            policy,
            usage,
        };
        let report = run(&steps, config).unwrap();
        prop_assert!(report.is_clean(), "{}", report);
    }
}
