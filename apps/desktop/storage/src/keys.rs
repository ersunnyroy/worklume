//! The store's keys.
//!
//! One random 256-bit master key per device lives in the OS credential store. The database and
//! the spool each use a key derived from it with HKDF-SHA256, so a single keychain entry (one
//! access prompt on macOS) covers both, and neither derived key reveals the other or the master.

use hkdf::Hkdf;
use sha2::Sha256;
use worklume_platform::{SecretStore, SecretStoreError};
use zeroize::Zeroizing;

use crate::StorageError;

/// Name of the credential store entry holding the master key, under the app's service name.
pub(crate) const MASTER_KEY_ENTRY: &str = "local-store-key";

const KEY_BYTES: usize = 32;
const DATABASE_KEY_INFO: &[u8] = b"worklume/local-store/sqlcipher/v1";
const SPOOL_KEY_INFO: &[u8] = b"worklume/local-store/spool/v1";

pub(crate) type KeyBytes = Zeroizing<[u8; KEY_BYTES]>;

/// What the credential store holds for the master key.
pub(crate) enum StoredKey {
    Present(MasterKey),
    /// The store answered that there is no entry.
    Missing,
    /// There is an entry, but it is not a key this agent wrote.
    Malformed,
}

pub(crate) struct MasterKey(KeyBytes);

impl MasterKey {
    /// Derives the raw key `SQLCipher` encrypts the database with.
    pub(crate) fn database_key(&self) -> KeyBytes {
        self.derive(DATABASE_KEY_INFO)
    }

    /// Derives the AES-256-GCM key for spool files.
    pub(crate) fn spool_key(&self) -> KeyBytes {
        self.derive(SPOOL_KEY_INFO)
    }

    #[expect(
        clippy::expect_used,
        reason = "HKDF-SHA256 fails only above 8,160 output bytes; this asks for 32"
    )]
    fn derive(&self, info: &[u8]) -> KeyBytes {
        // The master key is uniformly random, so HKDF needs no salt.
        let hkdf = Hkdf::<Sha256>::new(None, self.0.as_slice());
        let mut okm = Zeroizing::new([0_u8; KEY_BYTES]);
        hkdf.expand(info, okm.as_mut_slice())
            .expect("32 bytes is within HKDF-SHA256's output limit");
        okm
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: [u8; KEY_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }
}

/// Reads the master key. Any credential store failure is an error, never "missing".
pub(crate) fn load(secrets: &dyn SecretStore) -> Result<StoredKey, StorageError> {
    let Some(encoded) = secrets
        .get(MASTER_KEY_ENTRY)
        .map_err(StorageError::KeyUnavailable)?
    else {
        return Ok(StoredKey::Missing);
    };
    Ok(decode_hex(&encoded).map_or(StoredKey::Malformed, |bytes| {
        StoredKey::Present(MasterKey(bytes))
    }))
}

/// Creates a new master key, stores it, and reads it back to prove the store kept it.
pub(crate) fn create(secrets: &dyn SecretStore) -> Result<MasterKey, StorageError> {
    let mut bytes = Zeroizing::new([0_u8; KEY_BYTES]);
    getrandom::fill(bytes.as_mut_slice()).map_err(|_| {
        StorageError::KeyUnavailable(SecretStoreError::new(
            worklume_platform::SecretStoreErrorKind::Failed,
            "the operating system's random number generator failed",
        ))
    })?;
    let encoded = encode_hex(bytes.as_slice());
    secrets
        .set(MASTER_KEY_ENTRY, &encoded)
        .map_err(StorageError::KeyUnavailable)?;
    match load(secrets)? {
        StoredKey::Present(stored) if stored.0 == bytes => Ok(MasterKey(bytes)),
        _ => Err(StorageError::KeyNotPersisted),
    }
}

/// Lowercase hex: every credential store keeps text (KDE Wallet keeps nothing else).
pub(crate) fn encode_hex(bytes: &[u8]) -> Zeroizing<String> {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = Zeroizing::new(String::with_capacity(bytes.len() * 2));
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn decode_hex(text: &str) -> Option<KeyBytes> {
    let digits = text.as_bytes();
    if digits.len() != KEY_BYTES * 2 {
        return None;
    }
    let mut out = Zeroizing::new([0_u8; KEY_BYTES]);
    for (slot, pair) in out.iter_mut().zip(digits.chunks_exact(2)) {
        *slot = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
    }
    Some(out)
}

const fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{MasterKey, decode_hex, encode_hex};

    #[test]
    fn hex_round_trips() {
        let bytes: [u8; 32] = core::array::from_fn(|index| u8::try_from(index * 8).unwrap());
        let encoded = encode_hex(bytes.as_slice());
        assert_eq!(encoded.len(), 64);
        assert!(encoded.starts_with("0008101820"));
        assert_eq!(*decode_hex(&encoded).unwrap(), bytes);
    }

    #[test]
    fn rejects_anything_but_64_lowercase_hex_digits() {
        let valid = "ab".repeat(32);
        assert!(decode_hex(&valid).is_some());
        assert!(decode_hex(&valid[..62]).is_none());
        assert!(decode_hex(&format!("{valid}00")).is_none());
        assert!(decode_hex(&"AB".repeat(32)).is_none());
        assert!(decode_hex(&"zz".repeat(32)).is_none());
        assert!(decode_hex("").is_none());
    }

    #[test]
    fn derived_keys_are_distinct_and_stable() {
        let master = MasterKey::from_bytes([7; 32]);
        let database = master.database_key();
        let spool = master.spool_key();
        assert_ne!(*database, *spool);
        assert_ne!(*database, [7; 32]);
        assert_eq!(*database, *MasterKey::from_bytes([7; 32]).database_key());
        assert_ne!(*database, *MasterKey::from_bytes([8; 32]).database_key());
    }
}
