//! Test doubles and builders shared by the storage integration tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]
#![allow(
    clippy::expect_used,
    reason = "test helpers fail the test on bad setup"
)]

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::json;
use worklume_platform::{Secret, SecretStore, SecretStoreError, SecretStoreErrorKind};
use worklume_storage::{
    EventId, EventType, JournalKind, NewEvent, NewJournalEntry, Payload, Timestamp,
};
use zeroize::Zeroizing;

/// An in-memory credential store that can be switched to fail like a locked keychain.
#[derive(Debug, Default)]
pub struct MemorySecrets {
    entries: Mutex<HashMap<String, String>>,
    unavailable: Mutex<bool>,
}

impl MemorySecrets {
    pub fn set_unavailable(&self, unavailable: bool) {
        *self.unavailable.lock().expect("lock") = unavailable;
    }

    pub fn raw(&self, name: &str) -> Option<String> {
        self.entries.lock().expect("lock").get(name).cloned()
    }

    pub fn put_raw(&self, name: &str, value: &str) {
        self.entries
            .lock()
            .expect("lock")
            .insert(name.to_owned(), value.to_owned());
    }

    pub fn remove_raw(&self, name: &str) {
        self.entries.lock().expect("lock").remove(name);
    }

    fn check(&self) -> Result<(), SecretStoreError> {
        if *self.unavailable.lock().expect("lock") {
            Err(SecretStoreError::new(
                SecretStoreErrorKind::Unavailable,
                "the keychain is locked",
            ))
        } else {
            Ok(())
        }
    }
}

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError> {
        self.check()?;
        Ok(self.raw(name).map(Zeroizing::new))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.put_raw(name, value);
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.remove_raw(name);
        Ok(())
    }
}

/// A credential store kept in a plain file, so a child process can share the key with its
/// parent in the crash test. Test-only: the agent never keeps keys in files.
#[derive(Debug)]
pub struct FileSecrets {
    pub dir: PathBuf,
}

impl SecretStore for FileSecrets {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError> {
        match fs::read_to_string(self.dir.join(name)) {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(SecretStoreError::new(
                SecretStoreErrorKind::Failed,
                error.to_string(),
            )),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        fs::write(self.dir.join(name), value)
            .map_err(|error| SecretStoreError::new(SecretStoreErrorKind::Failed, error.to_string()))
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        let _ = fs::remove_file(self.dir.join(name));
        Ok(())
    }
}

pub fn time(wall_ms: i64) -> Timestamp {
    Timestamp::new(wall_ms, wall_ms / 2, "boot-1").expect("valid timestamp")
}

pub fn payload(value: serde_json::Value) -> Payload {
    match value {
        serde_json::Value::Object(map) => map,
        _ => Payload::new(),
    }
}

pub fn event(event_type: &str, wall_ms: i64) -> NewEvent {
    NewEvent {
        event_id: EventId::new_v7(),
        event_type: EventType::new(event_type).expect("valid event type"),
        time: time(wall_ms),
        clock_offset_ms: Some(-250),
        payload: payload(json!({ "at": wall_ms })),
    }
}

pub fn journal(kind: &str, wall_ms: i64) -> NewJournalEntry {
    NewJournalEntry {
        kind: JournalKind::new(kind).expect("valid journal kind"),
        time: time(wall_ms),
        payload: payload(json!({ "at": wall_ms })),
    }
}

/// Every file under `dir`, recursively.
pub fn files_under(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(next).expect("readable directory") {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
