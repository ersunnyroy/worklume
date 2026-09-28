//! Values the store reads and writes. Constructors validate the rules the schema also enforces
//! with CHECK constraints, so bad input fails early with a clear error.

use std::fmt;

use serde_json::{Map, Value};
use uuid::{Uuid, Version};

use crate::StorageError;

/// Largest JSON payload, in bytes, for an outbox event or a journal entry. The server accepts
/// batches of up to 5 MB (architecture §13), so a single event may not approach that.
pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

/// A JSON object, the payload of every event and journal entry.
pub type Payload = Map<String, Value>;

/// The ID of an outbox event: a `UUIDv7` made on the device, which becomes the row ID on the
/// server (a slot, a screenshot or a time entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EventId(Uuid);

impl EventId {
    /// A new, time-ordered ID.
    #[must_use]
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }

    /// # Errors
    ///
    /// Fails unless the UUID is version 7.
    pub fn from_uuid(uuid: Uuid) -> Result<Self, StorageError> {
        if uuid.get_version() == Some(Version::SortRand) {
            Ok(Self(uuid))
        } else {
            Err(StorageError::invalid("event ID", "must be a UUIDv7"))
        }
    }

    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The ID of a spool file. A screenshot's file uses the screenshot's ID, which is also the ID of
/// the `screenshot.captured` event that references it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpoolId(Uuid);

impl SpoolId {
    /// # Errors
    ///
    /// Fails for the nil UUID.
    pub fn from_uuid(uuid: Uuid) -> Result<Self, StorageError> {
        if uuid.is_nil() {
            Err(StorageError::invalid(
                "spool ID",
                "must not be the nil UUID",
            ))
        } else {
            Ok(Self(uuid))
        }
    }

    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl From<EventId> for SpoolId {
    fn from(id: EventId) -> Self {
        Self(id.0)
    }
}

impl fmt::Display for SpoolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Position of an event in this device's outbox sequence. Strictly increasing and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Seq(pub(crate) i64);

impl Seq {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Position of a journal entry. Strictly increasing and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JournalId(pub(crate) i64);

impl JournalId {
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A lowercase dotted name such as `timer.started`: segments start with a letter and hold
/// letters, digits and underscores; at least two segments; at most 64 characters.
fn validate_dotted_name(name: &str, field: &'static str) -> Result<(), StorageError> {
    let rule = "must be a lowercase dotted name such as timer.started";
    if name.is_empty() || name.len() > 64 {
        return Err(StorageError::invalid(field, "must be 1 to 64 characters"));
    }
    let mut segments = 0;
    for segment in name.split('.') {
        segments += 1;
        let mut bytes = segment.bytes();
        if !bytes.next().is_some_and(|first| first.is_ascii_lowercase()) {
            return Err(StorageError::invalid(field, rule));
        }
        if !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_') {
            return Err(StorageError::invalid(field, rule));
        }
    }
    if segments < 2 {
        return Err(StorageError::invalid(field, rule));
    }
    Ok(())
}

/// The type of an outbox event, such as `timer.started` or `slot.closed`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventType(String);

impl EventType {
    /// # Errors
    ///
    /// Fails unless the name is a lowercase dotted name such as `timer.started`.
    pub fn new(name: impl Into<String>) -> Result<Self, StorageError> {
        let name = name.into();
        validate_dotted_name(&name, "event type")?;
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The kind of a journal entry, such as `tracker.started`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JournalKind(String);

impl JournalKind {
    /// # Errors
    ///
    /// Fails unless the name is a lowercase dotted name such as `tracker.started`.
    pub fn new(name: impl Into<String>) -> Result<Self, StorageError> {
        let name = name.into();
        validate_dotted_name(&name, "journal kind")?;
        Ok(Self(name))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// When something happened, by both clocks (architecture §13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    wall_ms: i64,
    mono_ms: i64,
    boot_id: String,
}

impl Timestamp {
    /// `wall_ms` is the device clock in UTC milliseconds since the Unix epoch. `mono_ms` is
    /// the monotonic clock in milliseconds, meaningful only within the boot named by `boot_id`.
    ///
    /// # Errors
    ///
    /// Fails for negative clock readings or a boot ID that is empty or over 64 characters.
    pub fn new(
        wall_ms: i64,
        mono_ms: i64,
        boot_id: impl Into<String>,
    ) -> Result<Self, StorageError> {
        let boot_id = boot_id.into();
        if wall_ms < 0 {
            return Err(StorageError::invalid(
                "wall time",
                "must not be before 1970",
            ));
        }
        if mono_ms < 0 {
            return Err(StorageError::invalid(
                "monotonic time",
                "must not be negative",
            ));
        }
        if boot_id.is_empty() || boot_id.len() > 64 {
            return Err(StorageError::invalid(
                "boot ID",
                "must be 1 to 64 characters",
            ));
        }
        Ok(Self {
            wall_ms,
            mono_ms,
            boot_id,
        })
    }

    #[must_use]
    pub const fn wall_ms(&self) -> i64 {
        self.wall_ms
    }

    #[must_use]
    pub const fn mono_ms(&self) -> i64 {
        self.mono_ms
    }

    #[must_use]
    pub fn boot_id(&self) -> &str {
        &self.boot_id
    }
}

/// An event to add to the outbox.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub event_id: EventId,
    pub event_type: EventType,
    pub time: Timestamp,
    /// Last known device-minus-server clock offset, if a sync has measured one.
    pub clock_offset_ms: Option<i64>,
    pub payload: Payload,
}

/// An event in the outbox, waiting for the server to acknowledge it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEvent {
    pub seq: Seq,
    pub event_id: EventId,
    pub event_type: EventType,
    pub time: Timestamp,
    pub clock_offset_ms: Option<i64>,
    pub payload: Payload,
}

/// A state change to add to the journal.
#[derive(Debug, Clone, PartialEq)]
pub struct NewJournalEntry {
    pub kind: JournalKind,
    pub time: Timestamp,
    pub payload: Payload,
}

/// A journal entry, read back for replay after a restart.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    pub id: JournalId,
    pub kind: JournalKind,
    pub time: Timestamp,
    pub payload: Payload,
}

/// What a spool file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpoolKind {
    Screenshot,
}

impl SpoolKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "screenshot" => Some(Self::Screenshot),
            _ => None,
        }
    }
}

/// A file in the spool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolEntry {
    pub id: SpoolId,
    pub kind: SpoolKind,
    /// Size of the contents.
    pub plain_bytes: u64,
    /// Size on disk, including the header and authentication tag.
    pub stored_bytes: u64,
    pub created_wall_ms: i64,
}

/// Serializes a payload, enforcing the size limit.
pub(crate) fn encode_payload(payload: &Payload) -> Result<String, StorageError> {
    let text = serde_json::to_string(payload)
        .map_err(|_| StorageError::invalid("payload", "cannot be encoded as JSON"))?;
    if text.len() > MAX_PAYLOAD_BYTES {
        return Err(StorageError::invalid("payload", "is larger than 256 KiB"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{EventId, EventType, JournalKind, SpoolId, Timestamp, encode_payload};
    use crate::{MAX_PAYLOAD_BYTES, Payload};

    #[test]
    fn event_ids_must_be_v7() {
        assert!(EventId::from_uuid(Uuid::now_v7()).is_ok());
        assert!(EventId::from_uuid(Uuid::nil()).is_err());
        assert!(
            EventId::from_uuid(Uuid::from_u128(0x6ba7_b810_9dad_41d1_80b4_00c0_4fd4_30c8)).is_err()
        );
        let first = EventId::new_v7();
        assert!(EventId::new_v7() > first, "v7 IDs sort by creation time");
    }

    #[test]
    fn spool_ids_reject_nil() {
        assert!(SpoolId::from_uuid(Uuid::nil()).is_err());
        assert_eq!(
            SpoolId::from(EventId::new_v7()).as_uuid().get_version_num(),
            7
        );
    }

    #[test]
    fn dotted_names() {
        for valid in [
            "timer.started",
            "app_usage.batch",
            "a.b.c",
            "slot2.closed_v2",
        ] {
            assert!(EventType::new(valid).is_ok(), "{valid}");
            assert!(JournalKind::new(valid).is_ok(), "{valid}");
        }
        let too_long = format!("a.{}", "b".repeat(63));
        for invalid in [
            "",
            "timer",
            "Timer.started",
            "timer..started",
            "timer.",
            ".timer",
            "timer.2x",
            "timer.star-ted",
            "timer started.x",
            "_timer.x",
            &too_long,
        ] {
            assert!(EventType::new(invalid).is_err(), "{invalid}");
            assert!(JournalKind::new(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn timestamps() {
        assert!(Timestamp::new(0, 0, "boot").is_ok());
        assert!(Timestamp::new(-1, 0, "boot").is_err());
        assert!(Timestamp::new(0, -1, "boot").is_err());
        assert!(Timestamp::new(0, 0, "").is_err());
        assert!(Timestamp::new(0, 0, "b".repeat(65)).is_err());
    }

    #[test]
    fn payload_size_limit() {
        let mut payload = Payload::new();
        payload.insert("data".into(), "x".repeat(MAX_PAYLOAD_BYTES - 11).into());
        assert_eq!(encode_payload(&payload).unwrap().len(), MAX_PAYLOAD_BYTES);
        payload.insert("data".into(), "x".repeat(MAX_PAYLOAD_BYTES - 10).into());
        assert!(encode_payload(&payload).is_err());
    }
}
