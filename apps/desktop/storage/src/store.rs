//! Opening the store, and reading and writing it.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension as _, Row, TransactionBehavior, params};
use uuid::Uuid;
use worklume_platform::SecretStore;
use zeroize::Zeroizing;

use crate::db::{self, OpenFailure};
use crate::keys::{self, MasterKey, StoredKey};
use crate::model::encode_payload;
use crate::spool::{Spool, sync_dir};
use crate::{
    EventId, EventType, JournalEntry, JournalId, JournalKind, NewEvent, NewJournalEntry,
    OutboxEvent, Payload, Seq, SpoolEntry, SpoolId, SpoolKind, StorageError, Timestamp,
};

const LOCK_FILE: &str = "store.lock";
const DATABASE_FILE: &str = "store.db";
/// `SQLite`'s companion files for WAL mode. They move with the database, never on their own.
const DATABASE_COMPANIONS: [&str; 2] = ["store.db-wal", "store.db-shm"];
const SPOOL_DIR: &str = "spool";
const QUARANTINE_DIR: &str = "quarantine";

/// Why an existing store was set aside when opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryReason {
    /// The credential store answered that it has no key, but a database exists. Without the key
    /// the data cannot be decrypted by anyone.
    KeyMissing,
    /// The credential store holds something under the key's name that is not a key.
    KeyMalformed,
    /// The key did not decrypt the database, or the file is not a database.
    DatabaseUnreadable,
}

/// What happened while opening, for the agent to report to the member and, later, the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// A new master key was created (first run, or after recovery).
    pub key_created: bool,
    /// An unusable store was moved here, and a new one started. Only the latest set-aside store
    /// is kept.
    pub recovered: Option<(RecoveryReason, PathBuf)>,
    /// Spool files without an index row (unfinished writes) that were deleted.
    pub spool_orphans_removed: usize,
    /// Indexed spool files that were gone from disk; their rows were deleted.
    pub spool_files_missing: Vec<SpoolId>,
}

/// The agent's encrypted local store: the journal, the outbox and the spool.
///
/// One process at a time may open a store directory; a lock file enforces it. The store is
/// `Sync`: operations are serialized on one connection, which suits the agent's low write rate.
pub struct Store {
    connection: Mutex<Connection>,
    spool: Spool,
    dir: PathBuf,
    _lock: File,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Opens the store in `dir`, creating it and its key on first use.
    ///
    /// The key comes from `secrets`. If the credential store cannot be used, opening fails and
    /// nothing is changed, because a locked keychain may unlock later. If it definitely has no
    /// usable key, or the key does not decrypt the database, the old store is set aside (see
    /// [`OpenReport::recovered`]) and a new one begins.
    ///
    /// # Errors
    ///
    /// Fails when the credential store is unavailable, another process has the store open, the
    /// schema is newer than this agent, or the file system or database fails.
    pub fn open(dir: &Path, secrets: &dyn SecretStore) -> Result<(Self, OpenReport), StorageError> {
        create_private_dir(dir)?;
        let lock = acquire_lock(&dir.join(LOCK_FILE))?;
        let mut report = OpenReport::default();
        let database_path = dir.join(DATABASE_FILE);

        let master = match keys::load(secrets)? {
            StoredKey::Present(master) => master,
            stored => {
                if database_path.exists() {
                    let reason = if matches!(stored, StoredKey::Malformed) {
                        RecoveryReason::KeyMalformed
                    } else {
                        RecoveryReason::KeyMissing
                    };
                    report.recovered = Some((reason, quarantine(dir)?));
                }
                report.key_created = true;
                keys::create(secrets)?
            }
        };

        let connection = match db::open(&database_path, &master.database_key()) {
            Ok(connection) => connection,
            Err(OpenFailure::Unreadable) => {
                report.recovered = Some((RecoveryReason::DatabaseUnreadable, quarantine(dir)?));
                match db::open(&database_path, &master.database_key()) {
                    Ok(connection) => connection,
                    Err(OpenFailure::Unreadable) => return Err(StorageError::Unreadable),
                    Err(OpenFailure::Other(error)) => return Err(error),
                }
            }
            Err(OpenFailure::Other(error)) => return Err(error),
        };

        let store = Self::assemble(dir, connection, &master, lock)?;
        let (orphans, missing) = store.reconcile_spool()?;
        report.spool_orphans_removed = orphans;
        report.spool_files_missing = missing;
        Ok((store, report))
    }

    fn assemble(
        dir: &Path,
        mut connection: Connection,
        master: &MasterKey,
        lock: File,
    ) -> Result<Self, StorageError> {
        db::migrate(&mut connection)?;
        let spool_dir = dir.join(SPOOL_DIR);
        create_private_dir(&spool_dir)?;
        Ok(Self {
            connection: Mutex::new(connection),
            spool: Spool::new(spool_dir, &master.spool_key()),
            dir: dir.to_path_buf(),
            _lock: lock,
        })
    }

    fn reconcile_spool(&self) -> Result<(usize, Vec<SpoolId>), StorageError> {
        let connection = self.connection();
        let indexed = {
            let mut statement = connection.prepare("SELECT id FROM spool_files")?;
            let rows = statement.query_map([], |row| spool_id_column(row, 0))?;
            rows.collect::<Result<HashSet<_>, _>>()?
        };
        let (orphans, missing) = self.spool.reconcile(&indexed)?;
        if !missing.is_empty() {
            let mut statement = connection.prepare("DELETE FROM spool_files WHERE id = ?1")?;
            for id in &missing {
                statement.execute([id.as_uuid().as_bytes()])?;
            }
        }
        Ok((orphans, missing))
    }

    /// Identifies this store's outbox sequence. A store created from scratch gets a new ID, so
    /// the server can tell a restarted `seq` from a replay (E09-S05).
    ///
    /// # Errors
    ///
    /// Fails when the database fails.
    pub fn store_id(&self) -> Result<Uuid, StorageError> {
        let bytes: Vec<u8> = self.connection().query_row(
            "SELECT store_id FROM store_info WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        Uuid::from_slice(&bytes).map_err(|_| StorageError::invalid("store ID", "is not 16 bytes"))
    }

    /// Runs `write` in one transaction. Everything it does commits together, or not at all.
    ///
    /// The commit is flushed to storage before this returns, so a journal entry is durable
    /// before the state change it records takes effect.
    ///
    /// # Errors
    ///
    /// Returns the error from `write`, or a database error from the commit. Either way, nothing
    /// was written, and spool files it created are deleted.
    pub fn write<T>(
        &self,
        write: impl FnOnce(&mut WriteTx<'_>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let mut connection = self.connection();
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut tx = WriteTx {
            transaction,
            spool: &self.spool,
            created: Vec::new(),
            deleted: Vec::new(),
        };
        let result = write(&mut tx);
        let WriteTx {
            transaction,
            created,
            deleted,
            ..
        } = tx;
        let committed = result.and_then(|value| {
            transaction.commit()?;
            Ok(value)
        });
        match committed {
            Ok(value) => {
                self.spool.remove_all(deleted);
                Ok(value)
            }
            Err(error) => {
                // The transaction rolled back when dropped; the files it wrote have no rows.
                self.spool.remove_all(created);
                Err(error)
            }
        }
    }

    /// Journal entries after `after` (or from the start), oldest first.
    ///
    /// # Errors
    ///
    /// Fails when the database fails or a row cannot be read back.
    pub fn journal_after(
        &self,
        after: Option<JournalId>,
        limit: usize,
    ) -> Result<Vec<JournalEntry>, StorageError> {
        let connection = self.connection();
        let mut statement = connection.prepare_cached(
            "SELECT id, kind, wall_ms, mono_ms, boot_id, payload
             FROM journal WHERE id > ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![after.map_or(0, JournalId::get), limit_param(limit)],
            |row| {
                Ok((
                    JournalId(row.get(0)?),
                    row.get::<_, String>(1)?,
                    timestamp_columns(row, 2)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (id, kind, time, payload) = row?;
            Ok(JournalEntry {
                id,
                kind: JournalKind::new(kind)?,
                time: time?,
                payload: decode_payload(&payload)?,
            })
        })
        .collect()
    }

    /// Events waiting for the server, in `seq` order.
    ///
    /// # Errors
    ///
    /// Fails when the database fails or a row cannot be read back.
    pub fn outbox_pending(&self, limit: usize) -> Result<Vec<OutboxEvent>, StorageError> {
        let connection = self.connection();
        let mut statement = connection.prepare_cached(
            "SELECT seq, event_id, event_type, wall_ms, mono_ms, boot_id, clock_offset_ms, payload
             FROM outbox ORDER BY seq LIMIT ?1",
        )?;
        let rows = statement.query_map([limit_param(limit)], |row| {
            Ok((
                Seq(row.get(0)?),
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
                timestamp_columns(row, 3)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (seq, event_id, event_type, time, clock_offset_ms, payload) = row?;
            let uuid = Uuid::from_slice(&event_id)
                .map_err(|_| StorageError::invalid("event ID", "is not 16 bytes"))?;
            Ok(OutboxEvent {
                seq,
                event_id: EventId::from_uuid(uuid)?,
                event_type: EventType::new(event_type)?,
                time: time?,
                clock_offset_ms,
                payload: decode_payload(&payload)?,
            })
        })
        .collect()
    }

    /// Number of events waiting for the server.
    ///
    /// # Errors
    ///
    /// Fails when the database fails.
    pub fn outbox_len(&self) -> Result<u64, StorageError> {
        let count: i64 = self
            .connection()
            .query_row("SELECT count(*) FROM outbox", [], |row| row.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    /// A spool file's index entry, if it exists.
    ///
    /// # Errors
    ///
    /// Fails when the database fails.
    pub fn spool_entry(&self, id: SpoolId) -> Result<Option<SpoolEntry>, StorageError> {
        let connection = self.connection();
        let mut statement = connection.prepare_cached(
            "SELECT id, kind, plain_bytes, stored_bytes, created_wall_ms
             FROM spool_files WHERE id = ?1",
        )?;
        statement
            .query_row([id.as_uuid().as_bytes()], spool_entry_row)
            .optional()?
            .transpose()
    }

    /// Decrypts a spool file. The contents are wiped from memory when dropped.
    ///
    /// # Errors
    ///
    /// Fails when the file is not in the index or on disk, or fails its integrity check.
    pub fn spool_read(&self, id: SpoolId) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        if self.spool_entry(id)?.is_none() {
            return Err(StorageError::SpoolFileMissing(id));
        }
        self.spool.read(id)
    }

    /// Runs `SQLite`'s and `SQLCipher`'s integrity checks and returns the problems found; empty
    /// means healthy. It reads every page, so it is for diagnostics, not for every open.
    ///
    /// # Errors
    ///
    /// Fails when the checks cannot run.
    pub fn verify_integrity(&self) -> Result<Vec<String>, StorageError> {
        let connection = self.connection();
        let mut problems = Vec::new();
        for pragma in ["PRAGMA integrity_check", "PRAGMA cipher_integrity_check"] {
            let mut statement = connection.prepare(pragma)?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                let row = row?;
                if row != "ok" {
                    problems.push(row);
                }
            }
        }
        Ok(problems)
    }

    pub(crate) fn connection(&self) -> MutexGuard<'_, Connection> {
        // A panic mid-transaction drops the transaction, which rolls it back, so the connection
        // is consistent even when the mutex is poisoned.
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn database_files(&self) -> impl Iterator<Item = PathBuf> + '_ {
        std::iter::once(DATABASE_FILE)
            .chain(DATABASE_COMPANIONS)
            .map(|name| self.dir.join(name))
    }
}

/// Writes inside one transaction; see [`Store::write`].
pub struct WriteTx<'a> {
    transaction: rusqlite::Transaction<'a>,
    spool: &'a Spool,
    /// Spool files written in this transaction, deleted if it does not commit.
    created: Vec<SpoolId>,
    /// Spool files whose rows this transaction deletes, removed from disk after it commits.
    deleted: Vec<SpoolId>,
}

impl fmt::Debug for WriteTx<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WriteTx").finish_non_exhaustive()
    }
}

impl WriteTx<'_> {
    /// Appends a journal entry.
    ///
    /// # Errors
    ///
    /// Fails when the payload is too large or the database fails.
    pub fn append_journal(&mut self, entry: &NewJournalEntry) -> Result<JournalId, StorageError> {
        let payload = encode_payload(&entry.payload)?;
        self.transaction
            .prepare_cached(
                "INSERT INTO journal (kind, wall_ms, mono_ms, boot_id, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                entry.kind.as_str(),
                entry.time.wall_ms(),
                entry.time.mono_ms(),
                entry.time.boot_id(),
                payload,
            ])?;
        Ok(JournalId(self.transaction.last_insert_rowid()))
    }

    /// Deletes journal entries up to and including `through`, once the state they record is
    /// captured elsewhere (for example in a closed slot). IDs are never reused.
    ///
    /// # Errors
    ///
    /// Fails when the database fails.
    pub fn remove_journal_through(&mut self, through: JournalId) -> Result<u64, StorageError> {
        let removed = self
            .transaction
            .execute("DELETE FROM journal WHERE id <= ?1", [through.get()])?;
        Ok(u64::try_from(removed).unwrap_or(0))
    }

    /// Adds an event to the outbox and returns its `seq`.
    ///
    /// # Errors
    ///
    /// Fails with [`StorageError::DuplicateEvent`] when the outbox already holds the event ID,
    /// when the payload is too large, or when the database fails.
    pub fn enqueue(&mut self, event: &NewEvent) -> Result<Seq, StorageError> {
        let payload = encode_payload(&event.payload)?;
        let inserted = self
            .transaction
            .prepare_cached(
                "INSERT INTO outbox
                   (event_id, event_type, wall_ms, mono_ms, boot_id, clock_offset_ms, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?
            .execute(params![
                event.event_id.as_uuid().as_bytes(),
                event.event_type.as_str(),
                event.time.wall_ms(),
                event.time.mono_ms(),
                event.time.boot_id(),
                event.clock_offset_ms,
                payload,
            ]);
        match inserted {
            Ok(_) => Ok(Seq(self.transaction.last_insert_rowid())),
            Err(rusqlite::Error::SqliteFailure(failure, _))
                if failure.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                Err(StorageError::DuplicateEvent)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Deletes events the server has acknowledged (accepted, or reported as duplicates). Only
    /// the sync engine calls this, and only with a server verdict in hand.
    ///
    /// # Errors
    ///
    /// Fails when the database fails.
    pub fn remove_acknowledged(&mut self, seqs: &[Seq]) -> Result<u64, StorageError> {
        let mut statement = self
            .transaction
            .prepare_cached("DELETE FROM outbox WHERE seq = ?1")?;
        let mut removed = 0;
        for seq in seqs {
            removed += statement.execute([seq.get()])?;
        }
        Ok(u64::try_from(removed).unwrap_or(0))
    }

    /// Encrypts `contents` into a new spool file and indexes it. The file is on storage before
    /// the transaction can commit; if it does not commit, the file is deleted.
    ///
    /// # Errors
    ///
    /// Fails when the ID is already in the spool, the contents exceed
    /// [`MAX_SPOOL_FILE_BYTES`](crate::MAX_SPOOL_FILE_BYTES), or the file system or database
    /// fails.
    pub fn spool_insert(
        &mut self,
        id: SpoolId,
        kind: SpoolKind,
        created_wall_ms: i64,
        contents: &[u8],
    ) -> Result<SpoolEntry, StorageError> {
        if created_wall_ms < 0 {
            return Err(StorageError::invalid(
                "creation time",
                "must not be before 1970",
            ));
        }
        let exists: bool = self.transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM spool_files WHERE id = ?1)",
            [id.as_uuid().as_bytes()],
            |row| row.get(0),
        )?;
        if exists {
            return Err(StorageError::DuplicateSpoolFile(id));
        }
        let stored_bytes = self.spool.write(id, contents)?;
        self.created.push(id);
        let entry = SpoolEntry {
            id,
            kind,
            plain_bytes: u64::try_from(contents.len()).unwrap_or(u64::MAX),
            stored_bytes,
            created_wall_ms,
        };
        self.transaction
            .prepare_cached(
                "INSERT INTO spool_files (id, kind, plain_bytes, stored_bytes, created_wall_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                id.as_uuid().as_bytes(),
                kind.as_str(),
                i64::try_from(entry.plain_bytes).unwrap_or(i64::MAX),
                i64::try_from(entry.stored_bytes).unwrap_or(i64::MAX),
                created_wall_ms,
            ])?;
        Ok(entry)
    }

    /// Removes a spool file from the index; the file is deleted once the transaction commits.
    /// Returns whether the file was indexed.
    ///
    /// # Errors
    ///
    /// Fails when the database fails.
    pub fn spool_remove(&mut self, id: SpoolId) -> Result<bool, StorageError> {
        let removed = self.transaction.execute(
            "DELETE FROM spool_files WHERE id = ?1",
            [id.as_uuid().as_bytes()],
        )?;
        if removed > 0 {
            self.deleted.push(id);
        }
        Ok(removed > 0)
    }

    pub(crate) fn transaction(&self) -> &rusqlite::Transaction<'_> {
        &self.transaction
    }

    pub(crate) fn defer_file_removal(&mut self, id: SpoolId) {
        self.deleted.push(id);
    }
}

fn limit_param(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn timestamp_columns(
    row: &Row<'_>,
    first: usize,
) -> rusqlite::Result<Result<Timestamp, StorageError>> {
    Ok(Timestamp::new(
        row.get(first)?,
        row.get(first + 1)?,
        row.get::<_, String>(first + 2)?,
    ))
}

fn decode_payload(text: &str) -> Result<Payload, StorageError> {
    serde_json::from_str(text).map_err(|_| StorageError::invalid("payload", "is not a JSON object"))
}

pub(crate) fn spool_id_column(row: &Row<'_>, index: usize) -> rusqlite::Result<SpoolId> {
    let bytes: Vec<u8> = row.get(index)?;
    Uuid::from_slice(&bytes)
        .ok()
        .and_then(|uuid| SpoolId::from_uuid(uuid).ok())
        .ok_or(rusqlite::Error::InvalidColumnType(
            index,
            "id".to_owned(),
            rusqlite::types::Type::Blob,
        ))
}

pub(crate) fn spool_entry_row(row: &Row<'_>) -> rusqlite::Result<Result<SpoolEntry, StorageError>> {
    let id = spool_id_column(row, 0)?;
    let kind: String = row.get(1)?;
    let plain_bytes: i64 = row.get(2)?;
    let stored_bytes: i64 = row.get(3)?;
    let created_wall_ms: i64 = row.get(4)?;
    Ok(SpoolKind::parse(&kind)
        .ok_or(StorageError::invalid("spool kind", "is unknown"))
        .map(|kind| SpoolEntry {
            id,
            kind,
            plain_bytes: u64::try_from(plain_bytes).unwrap_or(0),
            stored_bytes: u64::try_from(stored_bytes).unwrap_or(0),
            created_wall_ms,
        }))
}

/// Creates a directory only the current user can enter (on Unix), or tightens an existing one.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(dir)
    }
}

fn acquire_lock(path: &Path) -> Result<File, StorageError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(StorageError::InUse),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

/// Moves the database and spool into `quarantine/<unix ms>/`, replacing any earlier quarantine.
/// The data cannot be read by this agent, but support may want it for diagnosis.
fn quarantine(dir: &Path) -> Result<PathBuf, StorageError> {
    let root = dir.join(QUARANTINE_DIR);
    match fs::remove_dir_all(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let target = root.join(millis.to_string());
    create_private_dir(&target)?;
    for name in std::iter::once(DATABASE_FILE)
        .chain(DATABASE_COMPANIONS)
        .chain([SPOOL_DIR])
    {
        match fs::rename(dir.join(name), target.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    sync_dir(dir)?;
    Ok(target)
}
