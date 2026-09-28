//! `SQLCipher`'s log goes to a file, without the notice that quotes the file's path. The setting
//! is process-wide, so this binary holds a single test that runs its steps in order.

#![allow(
    clippy::unwrap_used,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::fs;

use common::MemorySecrets;
use tempfile::TempDir;
use worklume_storage::{MASTER_KEY_ENTRY, Store, route_sqlcipher_log};

#[test]
fn sqlcipher_log_goes_to_a_file_without_paths() {
    let logs = TempDir::new().unwrap();
    let log_file = logs.path().join("sqlcipher.log");
    route_sqlcipher_log(Some(&log_file)).unwrap();

    // A key that does not decrypt the database makes SQLCipher log an HMAC failure.
    let dir = TempDir::new().unwrap();
    let secrets = MemorySecrets::default();
    drop(Store::open(dir.path(), &secrets).unwrap());
    secrets.put_raw(MASTER_KEY_ENTRY, &"ab".repeat(32));
    drop(Store::open(dir.path(), &secrets).unwrap());

    route_sqlcipher_log(None).unwrap();
    let log = fs::read_to_string(&log_file).unwrap();
    assert!(log.contains("hmac check failed"), "{log}");
    assert!(!log.contains("set log to"), "{log}");
    assert!(!log.contains(logs.path().to_str().unwrap()), "{log}");
    assert!(!log.contains(dir.path().to_str().unwrap()), "{log}");

    // Off means off: nothing more is appended.
    let length = log.len();
    secrets.put_raw(MASTER_KEY_ENTRY, &"cd".repeat(32));
    drop(Store::open(dir.path(), &secrets).unwrap());
    assert_eq!(fs::read_to_string(&log_file).unwrap().len(), length);

    // A file that cannot be opened is reported, so the caller can log it and carry on.
    let missing_folder = logs.path().join("no-such-folder").join("sqlcipher.log");
    assert!(route_sqlcipher_log(Some(&missing_folder)).is_err());
    route_sqlcipher_log(None).unwrap();
}
