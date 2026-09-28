//! Test doubles for the platform traits, for tests in other crates and the simulation harness
//! (E09-S14). Built only with the `fake` feature, which production builds never enable.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use zeroize::Zeroizing;

use crate::{Secret, SecretStore, SecretStoreError, SecretStoreErrorKind};

/// A credential store in memory. It can be switched to fail like a locked keychain.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    entries: Mutex<HashMap<String, String>>,
    unavailable: Mutex<bool>,
}

impl MemorySecretStore {
    /// Makes every call fail as `Unavailable`, or work again.
    pub fn set_unavailable(&self, unavailable: bool) {
        *self
            .unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = unavailable;
    }

    fn check(&self) -> Result<(), SecretStoreError> {
        if *self
            .unavailable
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
        {
            Err(SecretStoreError::new(
                SecretStoreErrorKind::Unavailable,
                "the fake keychain is locked",
            ))
        } else {
            Ok(())
        }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, name: &str) -> Result<Option<Secret>, SecretStoreError> {
        self.check()?;
        Ok(self.entries().get(name).cloned().map(Zeroizing::new))
    }

    fn set(&self, name: &str, value: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.entries().insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        self.check()?;
        self.entries().remove(name);
        Ok(())
    }
}
