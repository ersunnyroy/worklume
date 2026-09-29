//! The JSON sent for each outbox event: the `DesktopEvent` schema of
//! `POST /v1/desktop/sync` in `packages/api-contract/openapi.yaml`.

use serde_json::{Value, json};
use worklume_core::time::rfc3339_utc;
use worklume_storage::OutboxEvent;

/// Encodes one outbox event for the sync request.
#[must_use]
pub fn encode(event: &OutboxEvent) -> Value {
    json!({
        "event_id": event.event_id.as_uuid(),
        "seq": event.seq.get(),
        "type": event.event_type.as_str(),
        "wall_time": rfc3339_utc(event.time.wall_ms()),
        "mono_time": {
            "ms": event.time.mono_ms(),
            "boot_id": event.time.boot_id(),
        },
        "clock_offset_ms": event.clock_offset_ms,
        "payload": event.payload,
    })
}
