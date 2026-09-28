//! Encrypted spool files (architecture §14, step 3).
//!
//! Each file is `WLS1 ‖ nonce (12 bytes) ‖ AES-256-GCM ciphertext and tag`. The associated data
//! is the magic plus the file's ID, so a file renamed to another ID fails authentication rather
//! than being uploaded as the wrong screenshot.
//!
//! A file is written under its final name with `create_new`, flushed to storage, and its
//! directory entry flushed, all before the index row that makes it visible commits. A crash at
//! any point leaves either a committed row with a complete file, or a file without a row, which
//! the next open deletes.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, Nonce, Payload};
use aes_gcm::{Aes256Gcm, Key};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::keys::KeyBytes;
use crate::{SpoolId, StorageError};

/// Largest file the spool accepts. Screenshots are WebP images with a longest edge of 1,920 px,
/// well under this; the limit bounds the memory used to encrypt and decrypt.
pub const MAX_SPOOL_FILE_BYTES: usize = 32 * 1024 * 1024;

const MAGIC: &[u8; 4] = b"WLS1";
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const EXTENSION: &str = "wls";

/// Bytes a file takes on disk beyond its contents.
pub(crate) const OVERHEAD_BYTES: usize = MAGIC.len() + NONCE_BYTES + TAG_BYTES;

pub(crate) struct Spool {
    dir: PathBuf,
    cipher: Aes256Gcm,
}

impl Spool {
    pub(crate) fn new(dir: PathBuf, key: &KeyBytes) -> Self {
        let key: &Key<Aes256Gcm> = (&**key).into();
        let cipher = Aes256Gcm::new(key);
        Self { dir, cipher }
    }

    pub(crate) fn path(&self, id: SpoolId) -> PathBuf {
        self.dir
            .join(format!("{}.{EXTENSION}", id.as_uuid().as_simple()))
    }

    /// Encrypts `plaintext` into a new file and flushes it and its directory entry to storage.
    /// Returns the file's size on disk.
    pub(crate) fn write(&self, id: SpoolId, plaintext: &[u8]) -> Result<u64, StorageError> {
        if plaintext.len() > MAX_SPOOL_FILE_BYTES {
            return Err(StorageError::invalid("spool file", "is larger than 32 MiB"));
        }
        let mut nonce_bytes = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce_bytes).map_err(|_| {
            StorageError::Io(io::Error::other("the random number generator failed"))
        })?;
        let aad = associated_data(id);
        let ciphertext = self
            .cipher
            .encrypt(
                (&nonce_bytes).into(),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| StorageError::invalid("spool file", "could not be encrypted"))?;

        let path = self.path(id);
        let mut file = match create_private(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(StorageError::DuplicateSpoolFile(id));
            }
            Err(error) => return Err(error.into()),
        };
        let written = (|| {
            file.write_all(MAGIC)?;
            file.write_all(&nonce_bytes)?;
            file.write_all(&ciphertext)?;
            file.sync_all()?;
            sync_dir(&self.dir)
        })();
        if let Err(error) = written {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(u64::try_from(OVERHEAD_BYTES + plaintext.len()).unwrap_or(u64::MAX))
    }

    /// Reads and decrypts a file. The contents are wiped from memory when dropped.
    pub(crate) fn read(&self, id: SpoolId) -> Result<Zeroizing<Vec<u8>>, StorageError> {
        let bytes = match fs::read(self.path(id)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(StorageError::SpoolFileMissing(id));
            }
            Err(error) => return Err(error.into()),
        };
        if bytes.len() < OVERHEAD_BYTES || !bytes.starts_with(MAGIC) {
            return Err(StorageError::SpoolFileCorrupt(id));
        }
        let (nonce, ciphertext) = bytes[MAGIC.len()..].split_at(NONCE_BYTES);
        let nonce =
            <&Nonce<Aes256Gcm>>::try_from(nonce).map_err(|_| StorageError::SpoolFileCorrupt(id))?;
        let aad = associated_data(id);
        self.cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| StorageError::SpoolFileCorrupt(id))
    }

    /// IDs of the spool files on disk. Files with other names are not the spool's and are left
    /// alone.
    pub(crate) fn list(&self) -> io::Result<Vec<SpoolId>> {
        let mut ids = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some(EXTENSION) {
                continue;
            }
            let parsed = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| Uuid::try_parse(stem).ok())
                .and_then(|uuid| SpoolId::from_uuid(uuid).ok());
            match parsed {
                Some(id) if path == self.path(id) => ids.push(id),
                // A spool-looking name this agent never writes: remove it with the orphans.
                _ => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        Ok(ids)
    }

    /// Deletes files, ignoring any already gone. A file that cannot be deleted now is an orphan
    /// the next open removes.
    pub(crate) fn remove_all(&self, ids: impl IntoIterator<Item = SpoolId>) {
        for id in ids {
            let _ = fs::remove_file(self.path(id));
        }
    }

    /// Deletes files on disk that have no index row, and reports rows whose file is gone.
    pub(crate) fn reconcile(
        &self,
        indexed: &HashSet<SpoolId>,
    ) -> io::Result<(usize, Vec<SpoolId>)> {
        let on_disk: HashSet<SpoolId> = self.list()?.into_iter().collect();
        let orphans: Vec<SpoolId> = on_disk.difference(indexed).copied().collect();
        let orphan_count = orphans.len();
        self.remove_all(orphans);
        let mut missing: Vec<SpoolId> = indexed.difference(&on_disk).copied().collect();
        missing.sort_unstable();
        Ok((orphan_count, missing))
    }
}

fn associated_data(id: SpoolId) -> [u8; 20] {
    let mut aad = [0_u8; 20];
    aad[..4].copy_from_slice(MAGIC);
    aad[4..].copy_from_slice(id.as_uuid().as_bytes());
    aad
}

/// Creates a file only the current user can read (on Unix; Windows profile folders are already
/// private to the user).
fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// Flushes a directory's entries, so a new file's name survives a power cut: an `fsync` of the
/// directory.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Windows offers no directory handle through the standard library. NTFS journals the new entry,
/// and the reconcile on open deletes any file whose row never committed.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "keeps the signature of the Unix version, which can fail"
)]
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;
    use uuid::Uuid;

    use super::{MAX_SPOOL_FILE_BYTES, OVERHEAD_BYTES, Spool};
    use crate::keys::MasterKey;
    use crate::{EventId, SpoolId, StorageError};

    fn spool(dir: &TempDir, key_byte: u8) -> Spool {
        Spool::new(
            dir.path().to_path_buf(),
            &MasterKey::from_bytes([key_byte; 32]).spool_key(),
        )
    }

    fn id() -> SpoolId {
        EventId::new_v7().into()
    }

    #[test]
    fn round_trips_and_hides_contents() {
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let id = id();
        let contents = b"RIFF....WEBPVP8 screenshot bytes".repeat(100);
        let stored = spool.write(id, &contents).unwrap();

        let on_disk = fs::read(spool.path(id)).unwrap();
        assert_eq!(usize::try_from(stored).unwrap(), on_disk.len());
        assert_eq!(on_disk.len(), contents.len() + OVERHEAD_BYTES);
        assert!(on_disk.starts_with(b"WLS1"));
        assert!(!on_disk.windows(12).any(|window| window == b"WEBPVP8 scre"));
        assert_eq!(*spool.read(id).unwrap(), contents);
    }

    #[test]
    fn same_contents_encrypt_differently() {
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let (first, second) = (id(), id());
        spool.write(first, b"same").unwrap();
        spool.write(second, b"same").unwrap();
        assert_ne!(
            fs::read(spool.path(first)).unwrap(),
            fs::read(spool.path(second)).unwrap()
        );
    }

    #[test]
    fn detects_tampering_swapping_and_the_wrong_key() {
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let (tampered, original, swapped) = (id(), id(), id());

        spool.write(tampered, b"contents").unwrap();
        let mut bytes = fs::read(spool.path(tampered)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(spool.path(tampered), bytes).unwrap();
        assert!(matches!(
            spool.read(tampered),
            Err(StorageError::SpoolFileCorrupt(_))
        ));

        spool.write(original, b"contents").unwrap();
        fs::rename(spool.path(original), spool.path(swapped)).unwrap();
        assert!(matches!(
            spool.read(swapped),
            Err(StorageError::SpoolFileCorrupt(_))
        ));

        let other = id();
        spool.write(other, b"contents").unwrap();
        let other_key = super::Spool::new(
            dir.path().to_path_buf(),
            &MasterKey::from_bytes([2; 32]).spool_key(),
        );
        assert!(matches!(
            other_key.read(other),
            Err(StorageError::SpoolFileCorrupt(_))
        ));

        fs::write(spool.path(other), b"WLS").unwrap();
        assert!(matches!(
            spool.read(other),
            Err(StorageError::SpoolFileCorrupt(_))
        ));
    }

    #[test]
    fn never_overwrites_a_file() {
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let id = id();
        spool.write(id, b"first").unwrap();
        assert!(matches!(
            spool.write(id, b"second"),
            Err(StorageError::DuplicateSpoolFile(_))
        ));
        assert_eq!(*spool.read(id).unwrap(), b"first");
    }

    #[test]
    fn rejects_oversized_files_and_reports_missing_ones() {
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let too_big = vec![0_u8; MAX_SPOOL_FILE_BYTES + 1];
        assert!(matches!(
            spool.write(id(), &too_big),
            Err(StorageError::InvalidInput { .. })
        ));
        assert!(matches!(
            spool.read(id()),
            Err(StorageError::SpoolFileMissing(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let id = id();
        spool.write(id, b"contents").unwrap();
        let mode = fs::metadata(spool.path(id)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn reconcile_removes_orphans_and_reports_missing_files() {
        let dir = TempDir::new().unwrap();
        let spool = spool(&dir, 1);
        let (kept, orphan, missing) = (id(), id(), id());
        spool.write(kept, b"kept").unwrap();
        spool.write(orphan, b"orphan").unwrap();
        fs::write(dir.path().join("not-a-uuid.wls"), b"junk").unwrap();
        fs::write(dir.path().join("README.txt"), b"left alone").unwrap();
        let uppercase = Uuid::now_v7().as_simple().to_string().to_uppercase();
        fs::write(dir.path().join(format!("{uppercase}.wls")), b"junk").unwrap();

        let indexed = [kept, missing].into_iter().collect();
        let (orphans, missing_files) = spool.reconcile(&indexed).unwrap();

        assert_eq!(orphans, 1);
        assert_eq!(missing_files, vec![missing]);
        assert!(spool.path(kept).exists());
        assert!(!spool.path(orphan).exists());
        assert!(!dir.path().join("not-a-uuid.wls").exists());
        assert!(!dir.path().join(format!("{uppercase}.wls")).exists());
        assert!(dir.path().join("README.txt").exists());
    }
}
