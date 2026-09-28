//! The store's public behaviour: keys, journal, outbox, transactions, encryption at rest,
//! locking and recovery.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::fs;

use common::{MemorySecrets, event, files_under, journal, payload};
use serde_json::json;
use tempfile::TempDir;
use worklume_storage::{
    EventId, JournalId, MASTER_KEY_ENTRY, OpenReport, RecoveryReason, SpoolKind, StorageError,
    Store,
};

fn open(dir: &TempDir, secrets: &MemorySecrets) -> (Store, OpenReport) {
    Store::open(dir.path(), secrets).unwrap()
}

#[test]
fn first_open_creates_the_key_and_reopening_reuses_it() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();

    let (store, report) = open(&dir, &secrets);
    assert!(report.key_created);
    assert_eq!(report.recovered, None);
    let key = secrets.raw(MASTER_KEY_ENTRY).unwrap();
    assert_eq!(key.len(), 64);
    assert!(key.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let store_id = store.store_id().unwrap();
    assert!(!store_id.is_nil());
    drop(store);

    let (store, report) = open(&dir, &secrets);
    assert_eq!(report, OpenReport::default());
    assert_eq!(secrets.raw(MASTER_KEY_ENTRY).unwrap(), key);
    assert_eq!(store.store_id().unwrap(), store_id);
}

#[test]
fn journal_replays_from_a_cursor_and_ids_are_never_reused() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);

    let ids: Vec<JournalId> = store
        .write(|tx| {
            (1..=3)
                .map(|at| tx.append_journal(&journal("tracker.started", at)))
                .collect()
        })
        .unwrap();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));

    let all = store.journal_after(None, 10).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].kind.as_str(), "tracker.started");
    assert_eq!(all[0].time.wall_ms(), 1);
    assert_eq!(all[0].time.boot_id(), "boot-1");
    assert_eq!(all[0].payload, payload(json!({ "at": 1 })));
    assert_eq!(store.journal_after(Some(ids[0]), 10).unwrap().len(), 2);
    assert_eq!(store.journal_after(None, 1).unwrap().len(), 1);

    let removed = store.write(|tx| tx.remove_journal_through(ids[1])).unwrap();
    assert_eq!(removed, 2);
    let remaining = store.journal_after(None, 10).unwrap();
    assert_eq!(
        remaining.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        [ids[2]]
    );

    store.write(|tx| tx.remove_journal_through(ids[2])).unwrap();
    let next = store
        .write(|tx| tx.append_journal(&journal("tracker.stopped", 4)))
        .unwrap();
    assert!(next > ids[2], "an emptied journal still never reuses an ID");
}

#[test]
fn outbox_keeps_order_and_never_reuses_a_seq() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);

    let first = event("timer.started", 10);
    let seqs = store
        .write(|tx| {
            Ok(vec![
                tx.enqueue(&first)?,
                tx.enqueue(&event("slot.closed", 20))?,
                tx.enqueue(&event("timer.stopped", 30))?,
            ])
        })
        .unwrap();
    assert!(
        seqs.windows(2)
            .all(|pair| pair[1].get() == pair[0].get() + 1)
    );

    let pending = store.outbox_pending(10).unwrap();
    assert_eq!(
        pending.iter().map(|event| event.seq).collect::<Vec<_>>(),
        seqs
    );
    assert_eq!(pending[0].event_id, first.event_id);
    assert_eq!(pending[0].event_type, first.event_type);
    assert_eq!(pending[0].time, first.time);
    assert_eq!(pending[0].clock_offset_ms, Some(-250));
    assert_eq!(pending[0].payload, first.payload);

    let removed = store.write(|tx| tx.remove_acknowledged(&seqs)).unwrap();
    assert_eq!(removed, 3);
    assert_eq!(store.outbox_len().unwrap(), 0);

    let next = store
        .write(|tx| tx.enqueue(&event("timer.started", 40)))
        .unwrap();
    assert!(next > seqs[2], "an emptied outbox still never reuses a seq");
    assert_eq!(store.outbox_pending(10).unwrap()[0].seq, next);
}

#[test]
fn a_duplicate_event_id_rolls_back_the_whole_transaction() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);
    let original = event("timer.started", 10);
    store.write(|tx| tx.enqueue(&original)).unwrap();

    let result = store.write(|tx| {
        tx.append_journal(&journal("tracker.started", 11))?;
        tx.enqueue(&original)
    });
    assert!(matches!(result, Err(StorageError::DuplicateEvent)));
    assert!(store.journal_after(None, 10).unwrap().is_empty());
    assert_eq!(store.outbox_len().unwrap(), 1);
}

#[test]
fn a_failed_write_leaves_nothing_behind() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);
    let spool_id = EventId::new_v7().into();

    let result: Result<(), StorageError> = store.write(|tx| {
        tx.append_journal(&journal("tracker.started", 1))?;
        tx.enqueue(&event("timer.started", 1))?;
        tx.spool_insert(spool_id, SpoolKind::Screenshot, 1, b"image")?;
        Err(StorageError::InvalidInput {
            field: "test",
            reason: "the caller changed its mind",
        })
    });

    assert!(result.is_err());
    assert!(store.journal_after(None, 10).unwrap().is_empty());
    assert_eq!(store.outbox_len().unwrap(), 0);
    assert_eq!(store.spool_entry(spool_id).unwrap(), None);
    let spool_files = files_under(&dir.path().join("spool"));
    assert!(
        spool_files.is_empty(),
        "the written file was deleted: {spool_files:?}"
    );
}

#[test]
fn committed_data_survives_reopening() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);
    let spool_id = EventId::new_v7().into();
    store
        .write(|tx| {
            tx.append_journal(&journal("tracker.started", 1))?;
            tx.enqueue(&event("timer.started", 1))?;
            tx.spool_insert(spool_id, SpoolKind::Screenshot, 1, b"image bytes")
        })
        .unwrap();
    drop(store);

    let (store, report) = open(&dir, &secrets);
    assert_eq!(report, OpenReport::default());
    assert_eq!(store.journal_after(None, 10).unwrap().len(), 1);
    assert_eq!(store.outbox_len().unwrap(), 1);
    assert_eq!(*store.spool_read(spool_id).unwrap(), b"image bytes");
}

#[test]
fn nothing_is_readable_on_disk() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);
    let marker = "PLAINTEXT-MARKER-7f3a";
    store
        .write(|tx| {
            let mut entry = journal("tracker.started", 1);
            entry.payload = payload(json!({ "window": marker }));
            tx.append_journal(&entry)?;
            let mut outgoing = event("app_usage.batch", 1);
            outgoing.payload = payload(json!({ "url": marker }));
            tx.enqueue(&outgoing)?;
            tx.spool_insert(
                EventId::new_v7().into(),
                SpoolKind::Screenshot,
                1,
                marker.repeat(50).as_bytes(),
            )
        })
        .unwrap();

    // Checked while the store is open: the data sits in the write-ahead log, not yet
    // checkpointed into the main file.
    let files = files_under(dir.path());
    assert!(
        files.iter().any(|file| file.ends_with("store.db-wal")),
        "{files:?}"
    );
    for file in &files {
        // The lock file stays empty. Windows locks all of it while the store is open, so it
        // cannot be read from another handle; check its size instead.
        if file.ends_with("store.lock") {
            assert_eq!(fs::metadata(file).unwrap().len(), 0);
            continue;
        }
        let bytes = fs::read(file).unwrap();
        assert!(
            !bytes
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()),
            "{} contains plain text",
            file.display()
        );
    }
    let header = fs::read(dir.path().join("store.db")).unwrap();
    assert!(
        !header.starts_with(b"SQLite format 3"),
        "the database header is encrypted"
    );
    drop(store);
}

#[test]
fn a_second_process_cannot_open_the_store() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);
    assert!(matches!(
        Store::open(dir.path(), &secrets),
        Err(StorageError::InUse)
    ));
    drop(store);
    assert!(Store::open(dir.path(), &secrets).is_ok());
}

#[test]
fn an_unavailable_keychain_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();

    secrets.set_unavailable(true);
    assert!(matches!(
        Store::open(dir.path(), &secrets),
        Err(StorageError::KeyUnavailable(_))
    ));
    secrets.set_unavailable(false);
    assert_eq!(secrets.raw(MASTER_KEY_ENTRY), None, "no key was created");

    let (store, _) = open(&dir, &secrets);
    store
        .write(|tx| tx.enqueue(&event("timer.started", 1)))
        .unwrap();
    drop(store);
    let database = fs::read(dir.path().join("store.db")).unwrap();

    secrets.set_unavailable(true);
    assert!(matches!(
        Store::open(dir.path(), &secrets),
        Err(StorageError::KeyUnavailable(_))
    ));
    secrets.set_unavailable(false);
    assert!(
        !dir.path().join("quarantine").exists(),
        "nothing was set aside"
    );
    assert_eq!(fs::read(dir.path().join("store.db")).unwrap(), database);
    let (store, report) = open(&dir, &secrets);
    assert_eq!(report, OpenReport::default());
    assert_eq!(store.outbox_len().unwrap(), 1);
}

fn recover_after(
    change_key: impl FnOnce(&MemorySecrets),
) -> (TempDir, MemorySecrets, Store, OpenReport) {
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    let (store, _) = open(&dir, &secrets);
    let old_id = store.store_id().unwrap();
    store
        .write(|tx| {
            tx.enqueue(&event("timer.started", 1))?;
            tx.spool_insert(EventId::new_v7().into(), SpoolKind::Screenshot, 1, b"image")
        })
        .unwrap();
    drop(store);

    change_key(&secrets);
    let (store, report) = open(&dir, &secrets);
    assert_eq!(store.outbox_len().unwrap(), 0, "the new store starts empty");
    assert_ne!(
        store.store_id().unwrap(),
        old_id,
        "the new store has a new sequence ID"
    );
    let (_, set_aside) = report.recovered.as_ref().unwrap();
    assert!(set_aside.join("store.db").exists());
    assert_eq!(files_under(&set_aside.join("spool")).len(), 1);
    (dir, secrets, store, report)
}

#[test]
fn a_missing_key_sets_the_old_store_aside() {
    let (_dir, secrets, _store, report) =
        recover_after(|secrets| secrets.remove_raw(MASTER_KEY_ENTRY));
    assert_eq!(report.recovered.unwrap().0, RecoveryReason::KeyMissing);
    assert!(report.key_created);
    assert_eq!(secrets.raw(MASTER_KEY_ENTRY).map(|key| key.len()), Some(64));
}

#[test]
fn a_malformed_key_sets_the_old_store_aside() {
    let (_dir, secrets, _store, report) =
        recover_after(|secrets| secrets.put_raw(MASTER_KEY_ENTRY, "not a key"));
    assert_eq!(report.recovered.unwrap().0, RecoveryReason::KeyMalformed);
    assert!(report.key_created);
    assert_eq!(secrets.raw(MASTER_KEY_ENTRY).map(|key| key.len()), Some(64));
}

#[test]
fn an_undecryptable_database_is_set_aside() {
    let other_key = "ab".repeat(32);
    let (_dir, secrets, _store, report) =
        recover_after(|secrets| secrets.put_raw(MASTER_KEY_ENTRY, &other_key));
    assert_eq!(
        report.recovered.unwrap().0,
        RecoveryReason::DatabaseUnreadable
    );
    assert!(!report.key_created, "the key in the keychain is kept");
    assert_eq!(secrets.raw(MASTER_KEY_ENTRY), Some(other_key));
}

#[test]
fn only_the_latest_set_aside_store_is_kept() {
    let (dir, secrets, store, first) =
        recover_after(|secrets| secrets.remove_raw(MASTER_KEY_ENTRY));
    drop(store);
    std::thread::sleep(std::time::Duration::from_millis(5));
    secrets.remove_raw(MASTER_KEY_ENTRY);
    let (_store, second) = open(&dir, &secrets);

    let quarantined: Vec<_> = fs::read_dir(dir.path().join("quarantine"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(quarantined, [second.recovered.unwrap().1]);
    assert!(!first.recovered.unwrap().1.exists());
}

#[cfg(unix)]
#[test]
fn the_store_directory_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = TempDir::new().unwrap();
    let store_dir = dir.path().join("agent");
    fs::create_dir(&store_dir).unwrap();
    fs::set_permissions(&store_dir, fs::Permissions::from_mode(0o755)).unwrap();
    let secrets = MemorySecrets::default();
    let _store = Store::open(&store_dir, &secrets).unwrap();
    for path in [store_dir.clone(), store_dir.join("spool")] {
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{}", path.display());
    }
}
