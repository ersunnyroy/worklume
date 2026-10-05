//! The JSON sent as the capability report: the `DesktopCapabilities` schema of
//! `PUT /v1/desktop/capabilities` in `packages/api-contract/openapi.yaml`. The device is the
//! token's, so the body names no device, member or organization.

use serde_json::{Map, Value, json};
use worklume_platform::CapabilityReport;

/// Encodes a capability report for the request body.
#[must_use]
pub fn encode(report: &CapabilityReport) -> Value {
    let features: Map<String, Value> = report
        .iter()
        .map(|(feature, capability)| {
            let reasons: Vec<&str> = capability
                .reasons()
                .iter()
                .map(|reason| reason.as_str())
                .collect();
            (
                feature.as_str().to_owned(),
                json!({ "status": capability.status().as_str(), "reasons": reasons }),
            )
        })
        .collect();
    let mut body = Map::new();
    body.insert("platform".to_owned(), report.platform.as_str().into());
    if let Some(desktop) = report.desktop {
        body.insert("desktop".to_owned(), desktop.as_str().into());
    }
    body.insert("features".to_owned(), Value::Object(features));
    Value::Object(body)
}
