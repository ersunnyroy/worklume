//! Spool files through the store, reconciliation on open, and the size and age caps.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::fs;
use std::time::Duration;

use common::{MemorySecrets, event, files_under, journal};
use tempfile::TempDir;
use uuid::Uuid;
use worklume_storage::{EventId, SpoolId, SpoolKind, StorageCaps, StorageError, Store};

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn spool_path(dir: &TempDir, id: SpoolId) -> std::path::PathBuf {
    dir.path()
        .join("spool")
        .join(format!("{}.wls", id.as_uuid().as_simple()))
}

#[test]
fn insert_read_and_remove() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = Store::open(dir.path(), &secrets).unwrap();
    let id: SpoolId = EventId::new_v7().into();

    let entry = store
        .write(|tx| tx.spool_insert(id, SpoolKind::Screenshot, 1_000, b"webp bytes"))
        .unwrap();
    assert_eq!(entry.plain_bytes, 10);
    assert_eq!(entry.stored_bytes, 10 + 32);
    assert_eq!(store.spool_entry(id).unwrap(), Some(entry));
    assert_eq!(*store.spool_read(id).unwrap(), b"webp bytes");
    assert!(matches!(
        store.write(|tx| tx.spool_insert(id, SpoolKind::Screenshot, 1_000, b"again")),
        Err(StorageError::DuplicateSpoolFile(_))
    ));

    assert!(store.write(|tx| tx.spool_remove(id)).unwrap());
    assert!(!spool_path(&dir, id).exists());
    assert!(matches!(
        store.spool_read(id),
        Err(StorageError::SpoolFileMissing(_))
    ));
    assert!(!store.write(|tx| tx.spool_remove(id)).unwrap());
}

#[test]
fn a_removal_that_rolls_back_keeps_the_file() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = Store::open(dir.path(), &secrets).unwrap();
    let id: SpoolId = EventId::new_v7().into();
    store
        .write(|tx| tx.spool_insert(id, SpoolKind::Screenshot, 1, b"image"))
        .unwrap();

    let result: Result<(), StorageError> = store.write(|tx| {
        tx.spool_remove(id)?;
        Err(StorageError::DuplicateEvent)
    });
    assert!(result.is_err());
    assert_eq!(*store.spool_read(id).unwrap(), b"image");
}

#[test]
fn opening_removes_unfinished_files_and_reports_lost_ones() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = Store::open(dir.path(), &secrets).unwrap();
    let (kept, lost): (SpoolId, SpoolId) = (EventId::new_v7().into(), EventId::new_v7().into());
    store
        .write(|tx| {
            tx.spool_insert(kept, SpoolKind::Screenshot, 1, b"kept")?;
            tx.spool_insert(lost, SpoolKind::Screenshot, 2, b"lost")
        })
        .unwrap();
    drop(store);

    // A file without a row, as a crash between writing the file and committing leaves it.
    let unfinished = SpoolId::from_uuid(Uuid::now_v7()).unwrap();
    fs::copy(spool_path(&dir, kept), spool_path(&dir, unfinished)).unwrap();
    // A row whose file is gone.
    fs::remove_file(spool_path(&dir, lost)).unwrap();

    let (store, report) = Store::open(dir.path(), &secrets).unwrap();
    assert_eq!(report.spool_orphans_removed, 1);
    assert_eq!(report.spool_files_missing, [lost]);
    assert!(!spool_path(&dir, unfinished).exists());
    assert_eq!(store.spool_entry(lost).unwrap(), None);
    assert_eq!(*store.spool_read(kept).unwrap(), b"kept");
}

fn store_with_files(dir: &TempDir, secrets: &MemorySecrets, days: &[i64]) -> (Store, Vec<SpoolId>) {
    let (store, _) = Store::open(dir.path(), secrets).unwrap();
    let ids: Vec<SpoolId> = days.iter().map(|_| EventId::new_v7().into()).collect();
    store
        .write(|tx| {
            tx.append_journal(&journal("tracker.started", 1))?;
            tx.enqueue(&event("timer.started", 1))?;
            for (id, day) in ids.iter().zip(days) {
                tx.spool_insert(*id, SpoolKind::Screenshot, day * DAY_MS, &[0_u8; 1000])?;
            }
            Ok(())
        })
        .unwrap();
    (store, ids)
}

#[test]
fn caps_evict_old_files_then_the_oldest_until_the_store_fits() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, ids) = store_with_files(&dir, &secrets, &[1, 2, 40, 41, 42]);
    let now = 45 * DAY_MS;

    // Age only: files older than 30 days go.
    let generous = StorageCaps {
        max_total_bytes: u64::MAX,
        max_spool_age: Duration::from_hours(30 * 24),
    };
    let report = store.enforce_caps(&generous, now).unwrap();
    let evicted: Vec<SpoolId> = report.evicted.iter().map(|entry| entry.id).collect();
    assert_eq!(evicted, ids[..2]);
    assert_eq!(report.bytes_freed, 2 * 1032);
    assert!(!report.still_over_cap);
    assert!(!spool_path(&dir, ids[0]).exists());

    // Size: the oldest remaining file goes first, and only as many as needed.
    let usage = store.usage().unwrap();
    assert_eq!(usage.spool_files, 3);
    assert_eq!(usage.spool_bytes, 3 * 1032);
    assert!(usage.database_bytes > 0);
    let tight = StorageCaps {
        max_total_bytes: usage.total_bytes() - 1,
        ..generous
    };
    let report = store.enforce_caps(&tight, now).unwrap();
    assert_eq!(
        report
            .evicted
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [ids[2]]
    );
    assert!(!report.still_over_cap);
    assert_eq!(*store.spool_read(ids[3]).unwrap(), [0_u8; 1000]);

    // Time data is never evicted.
    assert_eq!(store.journal_after(None, 10).unwrap().len(), 1);
    assert_eq!(store.outbox_len().unwrap(), 1);
}

#[test]
fn caps_report_when_the_database_alone_is_over() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, ids) = store_with_files(&dir, &secrets, &[1, 2]);
    let caps = StorageCaps {
        max_total_bytes: 1,
        ..StorageCaps::default()
    };
    let report = store.enforce_caps(&caps, 3 * DAY_MS).unwrap();
    assert_eq!(report.evicted.len(), ids.len());
    assert!(report.still_over_cap);
    assert!(files_under(&dir.path().join("spool")).is_empty());
    assert_eq!(store.outbox_len().unwrap(), 1);
}

#[test]
fn caps_within_limits_evict_nothing() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = store_with_files(&dir, &secrets, &[1, 2]);
    let report = store
        .enforce_caps(&StorageCaps::default(), 3 * DAY_MS)
        .unwrap();
    assert!(report.evicted.is_empty());
    assert_eq!(store.usage().unwrap().spool_files, 2);
}

#[test]
fn default_caps_match_the_architecture() {
    let caps = StorageCaps::default();
    assert_eq!(caps.max_total_bytes, 2 * 1024 * 1024 * 1024);
    assert_eq!(caps.max_spool_age, Duration::from_hours(30 * 24));
}
