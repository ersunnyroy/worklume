-- Local agent store, schema version 1 (E09-S02). The whole file is encrypted by SQLCipher.
--
-- Times: `wall_ms` is the device clock (UTC milliseconds since the Unix epoch), which can jump;
-- `mono_ms` is the monotonic clock in milliseconds, meaningful only within the boot named by
-- `boot_id` (architecture §13). Durations are computed from `mono_ms`, never from `wall_ms`.
--
-- Event types and journal kinds are lowercase dotted names such as `timer.started`.

-- One row, created with the store. `store_id` names this outbox's `seq` sequence: a store
-- created from scratch (first run, or after an unreadable store was set aside) gets a new ID, so
-- the server can tell a restarted sequence from a replay.
CREATE TABLE store_info (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  store_id BLOB NOT NULL CHECK (length(store_id) = 16),
  created_wall_ms INTEGER NOT NULL
) STRICT;

INSERT INTO store_info (singleton, store_id, created_wall_ms)
VALUES (1, randomblob(16), CAST(unixepoch('subsec') * 1000 AS INTEGER));

-- Tracker state changes, written before they take effect so a restart can resume (E09-S04).
-- AUTOINCREMENT: IDs never repeat after old entries are removed, so "entries after ID n" stays
-- a valid replay cursor.
CREATE TABLE journal (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK (
    length(kind) <= 64
    AND kind GLOB '[a-z]*.[a-z]*'
    AND kind NOT GLOB '*[^a-z0-9_.]*'
    AND kind NOT GLOB '*.[^a-z]*'
    AND kind NOT GLOB '*.'
  ),
  wall_ms INTEGER NOT NULL CHECK (wall_ms >= 0),
  mono_ms INTEGER NOT NULL CHECK (mono_ms >= 0),
  boot_id TEXT NOT NULL CHECK (length(boot_id) BETWEEN 1 AND 64),
  payload TEXT NOT NULL CHECK (
    length(payload) <= 262144 AND json_valid(payload) AND json_type(payload) = 'object'
  )
) STRICT;

-- Events waiting for the server (architecture §13). A row leaves only when the server has
-- acknowledged it. `seq` is the per-device sequence; AUTOINCREMENT guarantees it is never
-- reused, even after acknowledged rows are deleted.
CREATE TABLE outbox (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  -- UUIDv7 made on the device; becomes the row ID on the server.
  event_id BLOB NOT NULL UNIQUE CHECK (length(event_id) = 16),
  event_type TEXT NOT NULL CHECK (
    length(event_type) <= 64
    AND event_type GLOB '[a-z]*.[a-z]*'
    AND event_type NOT GLOB '*[^a-z0-9_.]*'
    AND event_type NOT GLOB '*.[^a-z]*'
    AND event_type NOT GLOB '*.'
  ),
  wall_ms INTEGER NOT NULL CHECK (wall_ms >= 0),
  mono_ms INTEGER NOT NULL CHECK (mono_ms >= 0),
  boot_id TEXT NOT NULL CHECK (length(boot_id) BETWEEN 1 AND 64),
  -- Last known device-minus-server clock offset; unknown until the first sync.
  clock_offset_ms INTEGER,
  payload TEXT NOT NULL CHECK (
    length(payload) <= 262144 AND json_valid(payload) AND json_type(payload) = 'object'
  )
) STRICT;

-- Encrypted files in the spool directory, one row per file. A file without a row is an
-- unfinished write and is deleted on the next open.
CREATE TABLE spool_files (
  id BLOB PRIMARY KEY CHECK (length(id) = 16),
  kind TEXT NOT NULL CHECK (kind IN ('screenshot')),
  plain_bytes INTEGER NOT NULL CHECK (plain_bytes >= 0),
  -- Size on disk: header, nonce, ciphertext and tag.
  stored_bytes INTEGER NOT NULL CHECK (stored_bytes > plain_bytes),
  created_wall_ms INTEGER NOT NULL CHECK (created_wall_ms >= 0)
) STRICT, WITHOUT ROWID;

-- Eviction under the size and age caps drops the oldest files first.
CREATE INDEX spool_files_by_age ON spool_files (created_wall_ms, id);
