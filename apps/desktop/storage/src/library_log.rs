//! `SQLCipher`'s own log. By default it prints decryption and crypto-provider errors to stderr,
//! where nobody sees them in a tray app. The agent sends them to a file in its log folder.

use std::path::Path;

use rusqlite::Connection;

use crate::StorageError;

/// Sends `SQLCipher`'s warnings and errors to `target`, appending, or turns its log off with
/// `None`. The setting is process-wide.
///
/// Only warnings and errors are written. At those levels `SQLCipher` names files only when
/// migrating a database with `cipher_migrate`, which the agent never does, so the log holds no
/// account names. Setting the level first also keeps out `SQLCipher`'s informational line that
/// quotes the new path.
///
/// # Errors
///
/// Fails when the path is not UTF-8 or `SQLCipher` cannot open the file (on Windows it opens
/// files through the ANSI code page, so some non-ASCII paths fail). Callers should log the
/// error and carry on; the log is a diagnostic aid, not a requirement.
pub fn route_sqlcipher_log(target: Option<&Path>) -> Result<(), StorageError> {
    let destination = match target {
        Some(path) => path
            .to_str()
            .ok_or(StorageError::invalid("log path", "must be valid UTF-8"))?,
        None => "off",
    };
    let connection = Connection::open_in_memory()?;
    pragma(&connection, "PRAGMA cipher_log_level = WARN")?;
    // Pragma values cannot be bound parameters; quote the path as an SQL string literal.
    let status = pragma(
        &connection,
        &format!("PRAGMA cipher_log = '{}'", destination.replace('\'', "''")),
    )?;
    // SQLCipher answers with the status code of the change: "0" is SQLITE_OK.
    if status.as_deref() == Some("0") {
        Ok(())
    } else {
        Err(StorageError::invalid(
            "log path",
            "SQLCipher could not open it",
        ))
    }
}

/// Runs a pragma and returns the first column of its first row, if it returned one.
fn pragma(connection: &Connection, sql: &str) -> Result<Option<String>, StorageError> {
    let mut statement = connection.prepare(sql)?;
    let mut rows = statement.query([])?;
    Ok(match rows.next()? {
        Some(row) => row
            .get::<_, rusqlite::types::Value>(0)
            .ok()
            .map(|value| match value {
                rusqlite::types::Value::Text(text) => text,
                rusqlite::types::Value::Integer(number) => number.to_string(),
                _ => String::new(),
            }),
        None => None,
    })
}
