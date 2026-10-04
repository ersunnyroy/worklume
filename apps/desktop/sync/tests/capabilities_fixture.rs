//! Keeps the agent's capability reports and the API contract in step. Sample reports, encoded
//! as the agent sends them, and every status and reason code the agent knows must equal the
//! committed fixture, which the contract package validates against the `DesktopCapabilities`
//! schema and compares with the schema's code lists.
//!
//! After changing a report, regenerate the fixture with
//! `UPDATE_FIXTURES=1 cargo test -p worklume-sync --test capabilities_fixture`, then run the
//! contract package's tests.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

use std::path::PathBuf;

use serde_json::{Value, json};
use worklume_platform::{
    Capability, CapabilityReason as Why, CapabilityReport, CapabilityStatus as Status, Feature,
    LinuxDesktop, ReportedPlatform,
};
use worklume_sync::capabilities::encode;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/api-contract/test/fixtures/desktop-capabilities.json")
}

fn limited(status: Status, reason: Why, more: &[Why]) -> Capability {
    Capability::limited(status, reason, more)
}

/// A report with `overrides`, every other feature granted except browser addresses.
fn report(
    platform: ReportedPlatform,
    desktop: Option<LinuxDesktop>,
    overrides: &[(Feature, Capability)],
) -> CapabilityReport {
    CapabilityReport::new(platform, desktop, |feature| {
        overrides
            .iter()
            .find(|(overridden, _)| *overridden == feature)
            .map_or_else(
                || match feature {
                    Feature::Urls => limited(Status::Unsupported, Why::NotImplemented, &[]),
                    _ => Capability::granted(),
                },
                |(_, capability)| capability.clone(),
            )
    })
}

fn samples() -> Vec<(&'static str, CapabilityReport)> {
    let mut samples = vec![(
        "ubuntu_x11",
        report(ReportedPlatform::LinuxX11, Some(LinuxDesktop::Gnome), &[]),
    )];
    samples.extend(wayland_samples());
    samples.extend(vec![
        (
            "x11_without_input_events",
            report(
                ReportedPlatform::LinuxX11,
                Some(LinuxDesktop::Other),
                &[
                    (
                        Feature::Activity,
                        limited(
                            Status::Reduced,
                            Why::IdleTimeOnly,
                            &[Why::NoKeyboardMouseSplit, Why::AdapterFailed],
                        ),
                    ),
                    (
                        Feature::SessionLock,
                        limited(Status::Failed, Why::AdapterFailed, &[]),
                    ),
                ],
            ),
        ),
        (
            "macos_today",
            CapabilityReport::new(ReportedPlatform::MacOs, None, |_| {
                limited(Status::Unsupported, Why::NotImplemented, &[])
            }),
        ),
        (
            "linux_without_a_desktop_session",
            CapabilityReport::new(ReportedPlatform::LinuxX11, None, |_| {
                limited(Status::Unsupported, Why::NoGraphicalSession, &[])
            }),
        ),
    ]);
    samples
}

fn wayland_samples() -> Vec<(&'static str, CapabilityReport)> {
    let estimated = limited(
        Status::Reduced,
        Why::EstimatedFromIdle,
        &[Why::NoKeyboardMouseSplit],
    );
    vec![
        (
            "gnome_wayland_first_login",
            report(
                ReportedPlatform::LinuxWayland,
                Some(LinuxDesktop::Gnome),
                &[
                    (
                        Feature::Screenshots,
                        limited(Status::Pending, Why::ConsentNotAsked, &[]),
                    ),
                    (Feature::Activity, estimated.clone()),
                    (
                        Feature::Apps,
                        limited(Status::Pending, Why::GnomeExtensionNextLogin, &[]),
                    ),
                ],
            ),
        ),
        (
            "gnome_wayland_declined",
            report(
                ReportedPlatform::LinuxWayland,
                Some(LinuxDesktop::Gnome),
                &[
                    (
                        Feature::Screenshots,
                        limited(Status::Denied, Why::ConsentDeclined, &[]),
                    ),
                    (Feature::Activity, estimated),
                    (
                        Feature::Apps,
                        limited(Status::Denied, Why::GnomeExtensionDisabled, &[]),
                    ),
                ],
            ),
        ),
        (
            "kde_wayland",
            report(
                ReportedPlatform::LinuxWayland,
                Some(LinuxDesktop::Kde),
                &[
                    (
                        Feature::Screenshots,
                        limited(Status::Pending, Why::ConsentDialogOpen, &[]),
                    ),
                    (
                        Feature::Activity,
                        limited(
                            Status::Reduced,
                            Why::EstimatedFromIdle,
                            &[Why::NoKeyboardMouseSplit, Why::IdleInhibitorsCountAsActive],
                        ),
                    ),
                    (
                        Feature::Apps,
                        limited(Status::Unsupported, Why::DesktopUnsupported, &[]),
                    ),
                ],
            ),
        ),
    ]
}

#[test]
fn the_reports_match_the_committed_fixture() {
    let fixture = json!({
        "statuses": Status::ALL.map(Status::as_str),
        "reasons": Why::ALL.map(Why::as_str),
        "reports": samples()
            .iter()
            .map(|(name, report)| json!({ "name": name, "body": encode(report) }))
            .collect::<Vec<Value>>(),
    });
    let encoded = serde_json::to_string_pretty(&fixture).unwrap() + "\n";
    let path = fixture_path();
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, encoded).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).expect("the fixture exists");
    assert!(
        committed == encoded,
        "the agent's capability reports changed; regenerate {} with UPDATE_FIXTURES=1 and \
         check the contract",
        path.display()
    );
}

#[test]
fn the_body_holds_codes_and_no_desktop_where_there_is_none() {
    let mac = encode(&samples()[5].1);
    assert_eq!(mac["platform"], "macos");
    assert!(mac.get("desktop").is_none());
    let gnome = encode(&samples()[1].1);
    assert_eq!(gnome["desktop"], "gnome");
    assert_eq!(
        gnome["features"]["apps"],
        json!({ "status": "pending", "reasons": ["gnome_extension_next_login"] })
    );
    assert_eq!(
        gnome["features"]["idle"],
        json!({ "status": "granted", "reasons": [] })
    );
    let keys: Vec<&String> = gnome["features"].as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), Feature::ALL.len());
}
