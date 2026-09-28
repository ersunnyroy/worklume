//! Exercises the real OS credential store, so the tests are ignored by default. CI runs them
//! with `--ignored` on macOS, Windows, and Ubuntu with an unlocked GNOME Keyring; see
//! `.github/workflows/desktop.yml`.

use std::time::{SystemTime, UNIX_EPOCH};

use worklume_platform::{OsSecretStore, SecretStore};

/// Test entries live under their own service so they never touch the agent's real entries.
const SERVICE: &str = "com.worklume.agent.tests";

/// Deletes the entry however the test ends.
struct Cleanup<'a> {
    store: &'a OsSecretStore,
    name: String,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = self.store.delete(&self.name);
    }
}

fn unique_name(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{label}-{}-{nanos}", std::process::id())
}

#[test]
#[ignore = "uses the real OS credential store; run with --ignored"]
fn round_trips_a_secret() {
    let store = OsSecretStore::new(SERVICE).unwrap();
    let cleanup = Cleanup {
        store: &store,
        name: unique_name("round-trip"),
    };
    let name = cleanup.name.as_str();

    assert_eq!(store.get(name).unwrap(), None);

    store.set(name, "first value").unwrap();
    assert_eq!(
        store.get(name).unwrap().as_deref().map(String::as_str),
        Some("first value")
    );

    store.set(name, "replaced value").unwrap();
    assert_eq!(
        store.get(name).unwrap().as_deref().map(String::as_str),
        Some("replaced value")
    );

    store.delete(name).unwrap();
    assert_eq!(store.get(name).unwrap(), None);
}

#[test]
#[ignore = "uses the real OS credential store; run with --ignored"]
fn deleting_a_missing_entry_succeeds() {
    let store = OsSecretStore::new(SERVICE).unwrap();
    store.delete(&unique_name("missing")).unwrap();
}

#[test]
#[ignore = "uses the real OS credential store; run with --ignored"]
fn keeps_a_key_sized_hex_value_exactly() {
    let store = OsSecretStore::new(SERVICE).unwrap();
    let cleanup = Cleanup {
        store: &store,
        name: unique_name("hex"),
    };
    let value = "0123456789abcdef".repeat(4);
    store.set(&cleanup.name, &value).unwrap();
    assert_eq!(store.get(&cleanup.name).unwrap().as_deref(), Some(&value));
}
