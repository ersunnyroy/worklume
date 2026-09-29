//! The `SQLCipher` connection and schema migrations.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior};
use zeroize::Zeroizing;

use crate::StorageError;
use crate::keys::{KeyBytes, encode_hex};

/// A schema migration: `sql` moves the schema from `version - 1` to `version`.
pub(crate) struct Migration {
    pub(crate) version: u32,
    pub(crate) sql: &'static str,
}

/// Every migration, oldest first. Migrations are append-only: a released one never changes.
pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("../migrations/0001_initial.sql"),
}];

/// Why a database could not be opened.
#[derive(Debug)]
pub(crate) enum OpenFailure {
    /// Wrong key, or not a database: the file cannot be used with this key.
    Unreadable,
    Other(StorageError),
}

impl From<rusqlite::Error> for OpenFailure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Other(error.into())
    }
}

/// Opens (or creates) the encrypted database and configures it for durability.
pub(crate) fn open(path: &Path, key: &KeyBytes) -> Result<Connection, OpenFailure> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)?;

    // A raw 256-bit key (x'…') skips SQLCipher's passphrase KDF: the key is already random.
    // The key must be the first statement on the connection.
    let hex = encode_hex(key.as_slice());
    let statement = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", hex.as_str()));
    connection.execute_batch(&statement)?;

    // Without SQLCipher, `PRAGMA key` is silently ignored and data would be written in plain
    // text. SQLCipher answers `cipher_version`; plain SQLite returns no row.
    let cipher_version: Option<String> = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .ok();
    if cipher_version.is_none_or(|version| version.is_empty()) {
        return Err(OpenFailure::Other(StorageError::NotEncrypted));
    }

    // The first read decrypts page 1: a wrong key or a damaged header fails here.
    match connection.query_row("SELECT count(*) FROM sqlite_schema", [], |row| {
        row.get::<_, i64>(0)
    }) {
        Ok(_) => {}
        Err(error) if is_unreadable(&error) => return Err(OpenFailure::Unreadable),
        Err(error) => return Err(error.into()),
    }

    configure(&connection).map_err(OpenFailure::Other)?;
    Ok(connection)
}

fn is_unreadable(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

/// Durability settings (architecture §13: nothing journaled may be lost):
/// - WAL, with `synchronous = FULL` so every commit is flushed to storage before it returns.
///   The default for WAL (`NORMAL`) can lose the last commits on power loss.
/// - `fullfsync`: on macOS, `fsync` leaves data in the drive's cache; `F_FULLFSYNC` flushes it.
///   Other systems ignore these two pragmas.
/// - Temporary tables and indices stay in memory, never in unencrypted temp files.
fn configure(connection: &Connection) -> Result<(), StorageError> {
    let mode: String = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StorageError::invalid(
            "journal mode",
            "the database refused WAL",
        ));
    }
    connection.execute_batch(
        "PRAGMA synchronous = FULL;
         PRAGMA fullfsync = ON;
         PRAGMA checkpoint_fullfsync = ON;
         PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA trusted_schema = OFF;
         PRAGMA journal_size_limit = 67108864;",
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

/// Brings the schema up to date, one transaction per migration.
pub(crate) fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    apply(connection, MIGRATIONS)
}

pub(crate) fn apply(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), StorageError> {
    let current: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let latest = migrations.last().map_or(0, |migration| migration.version);
    if current > latest {
        return Err(StorageError::SchemaTooNew {
            found: current,
            supported: latest,
        });
    }
    for migration in migrations
        .iter()
        .filter(|migration| migration.version > current)
    {
        // IMMEDIATE takes the write lock up front. `user_version` lives in the database header,
        // so it commits or rolls back together with the migration.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql)?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::{MIGRATIONS, Migration, OpenFailure, apply, migrate, open};
    use crate::StorageError;
    use crate::keys::MasterKey;

    /// An open, migrated database in a temporary directory.
    pub(crate) fn test_database() -> (TempDir, Connection) {
        let dir = TempDir::new().unwrap();
        let mut connection = open_with(&dir.path().join("store.db"), 1).unwrap();
        migrate(&mut connection).unwrap();
        (dir, connection)
    }

    fn open_with(path: &Path, key_byte: u8) -> Result<Connection, OpenFailure> {
        open(path, &MasterKey::from_bytes([key_byte; 32]).database_key())
    }

    #[test]
    fn migration_versions_are_consecutive_from_one() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version as usize, index + 1);
        }
    }

    #[test]
    fn database_is_sqlcipher_with_durable_settings() {
        let (_dir, connection) = test_database();
        let pragma = |name: &str| -> String {
            connection
                .query_row(&format!("PRAGMA {name}"), [], |row| {
                    row.get::<_, rusqlite::types::Value>(0)
                })
                .map(|value| match value {
                    rusqlite::types::Value::Integer(number) => number.to_string(),
                    rusqlite::types::Value::Text(text) => text,
                    other => format!("{other:?}"),
                })
                .unwrap()
        };
        assert!(
            pragma("cipher_version").starts_with("4."),
            "SQLCipher 4 is linked"
        );
        assert_eq!(pragma("cipher_provider"), "openssl");
        assert_eq!(pragma("journal_mode"), "wal");
        assert_eq!(pragma("synchronous"), "2", "FULL");
        assert_eq!(pragma("foreign_keys"), "1");
        assert_eq!(pragma("temp_store"), "2", "MEMORY");
        assert_eq!(pragma("user_version"), MIGRATIONS.len().to_string());
    }

    #[test]
    fn wrong_key_is_unreadable() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("store.db");
        let mut connection = open_with(&path, 1).unwrap();
        migrate(&mut connection).unwrap();
        drop(connection);

        assert!(matches!(open_with(&path, 2), Err(OpenFailure::Unreadable)));
        assert!(open_with(&path, 1).is_ok());
    }

    #[test]
    fn plain_sqlite_file_is_unreadable() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("store.db");
        std::fs::write(&path, b"SQLite format 3\0 but not really").unwrap();
        assert!(matches!(open_with(&path, 1), Err(OpenFailure::Unreadable)));
    }

    /// The schema enforces its rules even for writes that bypass the Rust types.
    #[test]
    fn schema_rejects_rows_that_break_the_rules() {
        let (_dir, connection) = test_database();
        let rejected = |sql: &str| {
            let error = connection.execute(sql, []).expect_err(sql);
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation),
                "{sql}: {error}"
            );
        };
        let journal = |kind: &str, mono: &str, payload: &str| {
            format!(
                "INSERT INTO journal (kind, wall_ms, mono_ms, boot_id, payload)
                 VALUES ('{kind}', 1, {mono}, 'boot', '{payload}')"
            )
        };
        connection
            .execute(&journal("tracker.started", "0", "{}"), [])
            .unwrap();
        for kind in [
            "timer",
            "Timer.started",
            "timer..x",
            "timer.",
            "timer.2x",
            "timer.a-b",
        ] {
            rejected(&journal(kind, "0", "{}"));
        }
        rejected(&journal("tracker.started", "-1", "{}"));
        rejected(&journal("tracker.started", "'soon'", "{}"));
        rejected(&journal("tracker.started", "0", "not json"));
        rejected(&journal("tracker.started", "0", "[1, 2]"));

        let outbox = |event_id: &str, event_type: &str, payload: &str| {
            format!(
                "INSERT INTO outbox (event_id, event_type, wall_ms, mono_ms, boot_id, payload)
                 VALUES ({event_id}, '{event_type}', 1, 1, 'boot', {payload})"
            )
        };
        let id = "x'0192f0a47c1e7d3a9b2f5d8e6a4c3b21'";
        connection
            .execute(&outbox(id, "timer.started", "'{}'"), [])
            .unwrap();
        rejected(&outbox(id, "timer.stopped", "'{}'"));
        rejected(&outbox(
            "x'0192f0a47c1e7d3a9b2f5d8e6a4c3b'",
            "timer.started",
            "'{}'",
        ));
        rejected(&outbox(
            "x'0192f0a47c1e7d3a9b2f5d8e6a4c3b22'",
            "timer",
            "'{}'",
        ));
        rejected(&outbox(
            "x'0192f0a47c1e7d3a9b2f5d8e6a4c3b23'",
            "timer.started",
            "json_object('data', printf('%.262144c', 'x'))",
        ));

        let spool = |id: &str, kind: &str, plain: i64, stored: i64| {
            format!(
                "INSERT INTO spool_files (id, kind, plain_bytes, stored_bytes, created_wall_ms)
                 VALUES ({id}, '{kind}', {plain}, {stored}, 1)"
            )
        };
        connection
            .execute(&spool(id, "screenshot", 10, 42), [])
            .unwrap();
        rejected(&spool(
            "x'0192f0a47c1e7d3a9b2f5d8e6a4c3b22'",
            "video",
            10,
            42,
        ));
        rejected(&spool(
            "x'0192f0a47c1e7d3a9b2f5d8e6a4c3b23'",
            "screenshot",
            10,
            10,
        ));
        rejected(&spool("x'0192f0a4'", "screenshot", 10, 42));

        rejected("INSERT INTO store_info VALUES (2, randomblob(16), 1)");
        rejected("INSERT INTO store_info VALUES (1, randomblob(16), 1)");
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let (_dir, mut connection) = test_database();
        migrate(&mut connection).unwrap();
        let rows: i64 = connection
            .query_row("SELECT count(*) FROM store_info", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn refuses_a_schema_from_a_newer_agent() {
        let (_dir, mut connection) = test_database();
        let newer = u32::try_from(MIGRATIONS.len()).unwrap() + 1;
        connection
            .pragma_update(None, "user_version", newer)
            .unwrap();
        assert!(matches!(
            migrate(&mut connection),
            Err(StorageError::SchemaTooNew { found, .. }) if found == newer
        ));
    }

    #[test]
    fn a_failing_migration_rolls_back_entirely() {
        let dir = TempDir::new().unwrap();
        let mut connection = open_with(&dir.path().join("store.db"), 1).unwrap();
        let migrations = [
            Migration {
                version: 1,
                sql: "CREATE TABLE first (id INTEGER PRIMARY KEY) STRICT;",
            },
            Migration {
                version: 2,
                sql: "CREATE TABLE second (id INTEGER PRIMARY KEY) STRICT; SELECT no_such_function();",
            },
        ];
        assert!(apply(&mut connection, &migrations).is_err());

        let version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1, "the first migration committed");
        let second: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'second'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(second, 0, "the failed migration left nothing behind");
    }
}
