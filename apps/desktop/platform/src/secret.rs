//! Secret storage in the operating system's credential store (the `SecretStore` trait in
//! architecture §5): the macOS login keychain, Windows Credential Manager, and the Secret Service
//! (GNOME Keyring or KDE Wallet) on Linux.

use std::fmt;
use std::sync::Arc;

use keyring_core::{CredentialPersistence, CredentialStore, Entry, Error as KeyringError};
use zeroize::{Zeroize, Zeroizing};

/// A secret read from a store. Its memory is wiped when it is dropped.
pub type Secret = Zeroizing<String>;

/// Keeps small secrets, such as keys and tokens, in a credential store.
///
/// Values are text because KDE Wallet, one of the Linux Secret Service providers, stores only
/// UTF-8. Callers encode binary secrets, for example as hex.
pub trait SecretStore: Send + Sync + fmt::Debug {
    /// Reads a secret.
    ///
    /// `Ok(None)` means the store answered and holds no such entry. Every other failure,
    /// including a locked or unreachable keychain, is an error. Callers rely on the difference:
    /// only a definite "no entry" may lead them to create a replacement secret.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be reached or fails.
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError>;

    /// Creates or replaces a secret.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be reached or refuses the write.
    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError>;

    /// Deletes a secret. Deleting an entry that does not exist succeeds.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be reached or refuses the deletion.
    fn delete(&self, name: &str) -> Result<(), SecretStoreError>;
}

/// Why a secret store operation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretStoreErrorKind {
    /// The store cannot be used right now: it is locked, access was denied, or no provider is
    /// running. Retrying later may succeed.
    Unavailable,
    /// The store answered with an error or with data that is not a valid secret.
    Failed,
}

/// A failed secret store operation. The message never contains secret material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SecretStoreError {
    kind: SecretStoreErrorKind,
    message: String,
}

impl SecretStoreError {
    /// Creates an error of the given kind. The message must not contain secret material.
    #[must_use]
    pub fn new(kind: SecretStoreErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn kind(&self) -> SecretStoreErrorKind {
        self.kind
    }
}

/// The operating system's credential store, with every entry filed under one service name
/// (the app identifier, for example `com.worklume.agent`).
pub struct OsSecretStore {
    service: String,
    store: Arc<CredentialStore>,
}

impl fmt::Debug for OsSecretStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OsSecretStore")
            .field("service", &self.service)
            .field("vendor", &self.store.vendor())
            .finish()
    }
}

impl OsSecretStore {
    /// Connects to the credential store of the operating system the agent was built for.
    ///
    /// # Errors
    ///
    /// Returns an error when the store cannot be reached (on Linux, when no Secret Service
    /// provider runs in the session) or when it would not keep secrets until they are deleted.
    pub fn new(service: &str) -> Result<Self, SecretStoreError> {
        if service.is_empty() {
            return Err(SecretStoreError::new(
                SecretStoreErrorKind::Failed,
                "the service name is empty",
            ));
        }
        // Failing to reach the store at all (no D-Bus session, no provider running) is
        // something a later retry can fix, whatever the keyring crate calls it.
        let store = native_store().map_err(|error| {
            let error = describe("connect to the credential store", error);
            SecretStoreError::new(SecretStoreErrorKind::Unavailable, error.message)
        })?;
        // A store that forgets secrets (at logout, reboot or process exit) would lose the key to
        // data that must survive all three.
        if !matches!(store.persistence(), CredentialPersistence::UntilDelete) {
            return Err(SecretStoreError::new(
                SecretStoreErrorKind::Failed,
                "the credential store does not keep secrets until they are deleted",
            ));
        }
        Ok(Self {
            service: service.to_owned(),
            store,
        })
    }

    fn entry(&self, name: &str) -> Result<Entry, SecretStoreError> {
        self.store
            .build(&self.service, name, None)
            .map_err(|error| describe("address the secret", error))
    }
}

impl SecretStore for OsSecretStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError> {
        match self.entry(name)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(describe("read the secret", error)),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        self.entry(name)?
            .set_password(value)
            .map_err(|error| describe("write the secret", error))
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(describe("delete the secret", error)),
        }
    }
}

#[cfg(target_os = "macos")]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    // The login keychain. The data-protection keychain needs a signed app with a
    // keychain-access-groups entitlement, which unsigned development builds lack.
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(target_os = "windows")]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(target_os = "linux")]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Ok(dbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn native_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Err(KeyringError::NotSupportedByStore(
        "no credential store for this operating system".to_owned(),
    ))
}

/// Turns a keyring error into one that is safe to log: variants carrying the stored bytes have
/// them wiped, and only descriptions are kept.
fn describe(action: &str, error: KeyringError) -> SecretStoreError {
    let (kind, detail) = match error {
        KeyringError::NoStorageAccess(source) => (
            SecretStoreErrorKind::Unavailable,
            format!("access was refused: {source}"),
        ),
        KeyringError::PlatformFailure(source) => (
            SecretStoreErrorKind::Failed,
            format!("platform failure: {source}"),
        ),
        KeyringError::BadEncoding(mut bytes) => {
            bytes.zeroize();
            (
                SecretStoreErrorKind::Failed,
                "the stored value is not text".to_owned(),
            )
        }
        KeyringError::BadDataFormat(mut bytes, _) => {
            bytes.zeroize();
            (
                SecretStoreErrorKind::Failed,
                "the stored value is malformed".to_owned(),
            )
        }
        KeyringError::Ambiguous(entries) => (
            SecretStoreErrorKind::Failed,
            format!("{} entries match", entries.len()),
        ),
        other => (SecretStoreErrorKind::Failed, other.to_string()),
    };
    SecretStoreError::new(kind, format!("could not {action}: {detail}"))
}

#[cfg(test)]
mod tests {
    use super::{KeyringError, SecretStoreErrorKind, describe};

    #[test]
    fn locked_store_is_unavailable() {
        let error = describe(
            "read the secret",
            KeyringError::NoStorageAccess("locked".into()),
        );
        assert_eq!(error.kind(), SecretStoreErrorKind::Unavailable);
        assert_eq!(
            error.to_string(),
            "could not read the secret: access was refused: locked"
        );
    }

    #[test]
    fn platform_failure_is_failed() {
        let error = describe(
            "write the secret",
            KeyringError::PlatformFailure("boom".into()),
        );
        assert_eq!(error.kind(), SecretStoreErrorKind::Failed);
    }

    #[test]
    fn stored_bytes_never_reach_the_message() {
        let secret = b"0123456789abcdef-secret".to_vec();
        for error in [
            KeyringError::BadEncoding(secret.clone()),
            KeyringError::BadDataFormat(secret.clone(), "bad".into()),
        ] {
            let described = describe("read the secret", error);
            assert_eq!(described.kind(), SecretStoreErrorKind::Failed);
            assert!(
                !described.to_string().contains("0123456789abcdef"),
                "{described}"
            );
            assert!(!format!("{described:?}").contains("0123456789abcdef"));
        }
    }
}
