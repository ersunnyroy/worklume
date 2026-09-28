//! Size and age caps (architecture §13: offline data is kept up to 30 days or 2 GB, then the
//! oldest screenshots are dropped first, with a warning to the member and an admin).
//!
//! Only spool files are ever evicted. Outbox events and the journal record time worked, which
//! must reach the server however long the device stays offline.

use std::fs;
use std::io;
use std::time::Duration;

use crate::store::spool_entry_row;
use crate::{SpoolEntry, SpoolId, StorageError, Store};

/// Limits on local data. The defaults are the architecture's; the server's policy may override
/// them (E09-S06).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageCaps {
    /// Database plus spool, in bytes.
    pub max_total_bytes: u64,
    /// Spool files older than this are evicted.
    pub max_spool_age: Duration,
}

impl Default for StorageCaps {
    fn default() -> Self {
        Self {
            max_total_bytes: 2 * 1024 * 1024 * 1024,
            max_spool_age: Duration::from_hours(30 * 24),
        }
    }
}

/// How much space the store uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageUsage {
    /// The database file and its write-ahead log.
    pub database_bytes: u64,
    /// Spool files on disk, as recorded in the index.
    pub spool_bytes: u64,
    pub spool_files: u64,
}

impl StorageUsage {
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.database_bytes.saturating_add(self.spool_bytes)
    }
}

/// What [`Store::enforce_caps`] evicted. The agent tells the member and reports each evicted
/// screenshot to the server, so the gap is explained (E09-S07).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvictionReport {
    /// Evicted files, oldest first.
    pub evicted: Vec<SpoolEntry>,
    pub bytes_freed: u64,
    /// Still over the size cap after evicting every spool file: the database alone exceeds it,
    /// and time data is never dropped.
    pub still_over_cap: bool,
}

impl Store {
    /// Measures the store's disk use.
    ///
    /// # Errors
    ///
    /// Fails when the database or the file system fails.
    pub fn usage(&self) -> Result<StorageUsage, StorageError> {
        let (spool_bytes, spool_files): (i64, i64) = self.connection().query_row(
            "SELECT coalesce(sum(stored_bytes), 0), count(*) FROM spool_files",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut database_bytes = 0_u64;
        for path in self.database_files() {
            match fs::metadata(&path) {
                Ok(metadata) => database_bytes = database_bytes.saturating_add(metadata.len()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(StorageUsage {
            database_bytes,
            spool_bytes: u64::try_from(spool_bytes).unwrap_or(0),
            spool_files: u64::try_from(spool_files).unwrap_or(0),
        })
    }

    /// Evicts spool files older than the age cap, then the oldest remaining files until the store
    /// fits the size cap. `now_wall_ms` is the device clock, in UTC milliseconds.
    ///
    /// # Errors
    ///
    /// Fails when the database or the file system fails. Nothing is evicted then.
    pub fn enforce_caps(
        &self,
        caps: &StorageCaps,
        now_wall_ms: i64,
    ) -> Result<EvictionReport, StorageError> {
        let usage = self.usage()?;
        let max_age_ms = i64::try_from(caps.max_spool_age.as_millis()).unwrap_or(i64::MAX);
        let cutoff = now_wall_ms.saturating_sub(max_age_ms);

        self.write(|tx| {
            let entries = {
                let mut statement = tx.transaction().prepare(
                    "SELECT id, kind, plain_bytes, stored_bytes, created_wall_ms
                     FROM spool_files ORDER BY created_wall_ms, id",
                )?;
                let rows = statement.query_map([], spool_entry_row)?;
                rows.map(|row| row?)
                    .collect::<Result<Vec<_>, StorageError>>()?
            };

            let mut total = usage.total_bytes();
            let mut report = EvictionReport::default();
            for entry in entries {
                if entry.created_wall_ms >= cutoff && total <= caps.max_total_bytes {
                    break;
                }
                total = total.saturating_sub(entry.stored_bytes);
                report.bytes_freed = report.bytes_freed.saturating_add(entry.stored_bytes);
                report.evicted.push(entry);
            }
            report.still_over_cap = total > caps.max_total_bytes;

            let mut statement = tx
                .transaction()
                .prepare("DELETE FROM spool_files WHERE id = ?1")?;
            let evicted: Vec<SpoolId> = report.evicted.iter().map(|entry| entry.id).collect();
            for id in &evicted {
                statement.execute([id.as_uuid().as_bytes()])?;
            }
            drop(statement);
            for id in evicted {
                tx.defer_file_removal(id);
            }
            Ok(report)
        })
    }
}
