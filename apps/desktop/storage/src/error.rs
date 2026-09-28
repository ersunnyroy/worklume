use worklume_platform::SecretStoreError;

use crate::SpoolId;

/// A failed storage operation.
///
/// Messages never contain event payloads, file contents or keys, so they are safe to log.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// The OS credential store could not be used. The store stays closed rather than create a
    /// new key, because the old one may come back (a locked keychain, a Secret Service provider
    /// that starts after the agent).
    #[error("the key to the local store is unavailable: {0}")]
    KeyUnavailable(#[source] SecretStoreError),

    /// A newly created key did not read back from the credential store.
    #[error("the credential store did not keep the new key to the local store")]
    KeyNotPersisted,

    /// Another process holds the store's lock.
    #[error("another process is using the local store")]
    InUse,

    /// The database library has no encryption, so data would be written in plain text.
    #[error("this build has no SQLCipher; refusing to write an unencrypted store")]
    NotEncrypted,

    /// The database cannot be decrypted with the key, or is not a database. Opening sets such a
    /// store aside and starts a new one; this error means a new store failed the same way.
    #[error("the local store cannot be decrypted")]
    Unreadable,

    /// The store was written by a newer agent. Opening it could lose data the newer schema
    /// holds, so it is left untouched.
    #[error("the local store has schema {found}; this agent supports up to {supported}")]
    SchemaTooNew { found: u32, supported: u32 },

    /// Input that breaks a documented rule. The reason never quotes the input.
    #[error("invalid {field}: {reason}")]
    InvalidInput {
        field: &'static str,
        reason: &'static str,
    },

    /// The outbox already holds an event with this ID.
    #[error("the outbox already holds an event with this ID")]
    DuplicateEvent,

    /// The spool already holds a file with this ID.
    #[error("the spool already holds file {0}")]
    DuplicateSpoolFile(SpoolId),

    /// The spool has no file with this ID.
    #[error("the spool has no file {0}")]
    SpoolFileMissing(SpoolId),

    /// A spool file failed authentication: it was damaged, altered or swapped.
    #[error("spool file {0} failed its integrity check")]
    SpoolFileCorrupt(SpoolId),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("file system error: {0}")]
    Io(#[from] std::io::Error),
}

impl StorageError {
    pub(crate) const fn invalid(field: &'static str, reason: &'static str) -> Self {
        Self::InvalidInput { field, reason }
    }
}
