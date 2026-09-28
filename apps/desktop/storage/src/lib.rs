//! Encrypted local storage for the Worklume agent (architecture §5 and §13).
//!
//! The agent treats this store as the source of truth until the server acknowledges each event.
//! It holds:
//! - the **journal**: tracker state changes, written before they take effect, so a restart can
//!   resume where the agent stopped;
//! - the **outbox**: events waiting for the server, each with a `UUIDv7` and a per-device `seq`;
//! - the **spool**: encrypted files, such as screenshots, waiting for upload.
//!
//! The database is `SQLCipher` in WAL mode with every commit flushed to storage, and spool files
//! are AES-256-GCM. Both keys derive from one master key kept in the OS credential store; see
//! [`Store::open`] for what happens when that key is missing.
//!
//! On disk, a store directory holds:
//!
//! ```text
//! store.lock            held while a process has the store open
//! store.db, -wal, -shm  the encrypted database
//! spool/<id>.wls        encrypted spool files
//! quarantine/<ms>/      the last store set aside as unreadable, if any
//! ```

mod caps;
mod db;
mod error;
mod keys;
mod model;
mod spool;
mod store;

pub use caps::{EvictionReport, StorageCaps, StorageUsage};
pub use error::StorageError;
pub use model::{
    EventId, EventType, JournalEntry, JournalId, JournalKind, MAX_PAYLOAD_BYTES, NewEvent,
    NewJournalEntry, OutboxEvent, Payload, Seq, SpoolEntry, SpoolId, SpoolKind, Timestamp,
};
pub use spool::MAX_SPOOL_FILE_BYTES;
pub use store::{OpenReport, RecoveryReason, Store, WriteTx};

/// Name of the credential store entry that holds the master key.
pub const MASTER_KEY_ENTRY: &str = keys::MASTER_KEY_ENTRY;
