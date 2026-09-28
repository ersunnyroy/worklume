//! The E09-S02 acceptance criterion: a crash mid-write loses nothing that was journaled.
//!
//! The test runs its own binary as a child process that writes journal entries, outbox events
//! and spool files in a loop, printing each commit only after `write` returns. The parent kills
//! the child at varying moments, then reopens the store and checks that every reported commit is
//! intact, that the database passes its integrity checks, and that files from unfinished
//! writes were removed.
//!
//! Killing the process loses everything the operating system had not yet been told. A power cut
//! can also lose what the OS had not yet flushed to the disk; `synchronous = FULL` and
//! `F_FULLFSYNC` (macOS) flush every commit, so a commit that returned is on the disk.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "helpers in a test crate fail the test on bad setup, like the tests themselves"
)]

mod common;

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::{FileSecrets, journal};
use serde_json::json;
use tempfile::TempDir;
use worklume_storage::{
    EventId, EventType, NewEvent, OpenReport, Payload, SpoolId, SpoolKind, Store, Timestamp,
};

const CHILD_DIR: &str = "WORKLUME_STORAGE_CRASH_DIR";
const CHILD_ROUND: &str = "WORKLUME_STORAGE_CRASH_ROUND";
const ROUNDS: u64 = 12;
const SPOOL_FILE_BYTES: usize = 64 * 1024;

/// One commit the child reported.
#[derive(Debug)]
struct Commit {
    round: u64,
    n: u64,
    journal: i64,
    seq: i64,
    spool: Option<SpoolId>,
}

fn open(dir: &Path) -> (Store, OpenReport) {
    let secrets = FileSecrets {
        dir: dir.join("secrets"),
    };
    Store::open(&dir.join("store"), &secrets).unwrap()
}

fn spool_contents(round: u64, n: u64) -> Vec<u8> {
    let fill = u8::try_from((round * 31 + n) % 251).unwrap();
    vec![fill; SPOOL_FILE_BYTES]
}

fn marker(round: u64, n: u64) -> Payload {
    match json!({ "round": round, "n": n }) {
        serde_json::Value::Object(map) => map,
        _ => Payload::new(),
    }
}

/// The child: writes until killed. A no-op in a normal test run.
#[test]
fn crash_writer_child() {
    let (Ok(dir), Ok(round)) = (std::env::var(CHILD_DIR), std::env::var(CHILD_ROUND)) else {
        return;
    };
    let round: u64 = round.parse().unwrap();
    let (store, _) = open(Path::new(&dir));
    let mut out = std::io::stdout().lock();
    for n in 0.. {
        let spool = (n % 3 == 0).then(|| SpoolId::from(EventId::new_v7()));
        let wall_ms = i64::try_from(n).unwrap();
        let (journal_id, seq) = store
            .write(|tx| {
                let mut entry = journal("tracker.heartbeat", wall_ms);
                entry.payload = marker(round, n);
                let journal_id = tx.append_journal(&entry)?;
                let seq = tx.enqueue(&NewEvent {
                    event_id: EventId::new_v7(),
                    event_type: EventType::new("slot.closed")?,
                    time: Timestamp::new(wall_ms, wall_ms, "boot-crash")?,
                    clock_offset_ms: None,
                    payload: marker(round, n),
                })?;
                if let Some(id) = spool {
                    tx.spool_insert(
                        id,
                        SpoolKind::Screenshot,
                        wall_ms,
                        &spool_contents(round, n),
                    )?;
                }
                Ok((journal_id, seq))
            })
            .unwrap();
        let spool = spool.map_or_else(|| "-".to_owned(), |id| id.as_uuid().to_string());
        writeln!(
            out,
            "committed {n} {} {} {spool}",
            journal_id.get(),
            seq.get()
        )
        .unwrap();
        out.flush().unwrap();
    }
}

fn parse(round: u64, line: &str) -> Option<Commit> {
    let mut parts = line.strip_prefix("committed ")?.split(' ');
    let n = parts.next()?.parse().ok()?;
    let journal = parts.next()?.parse().ok()?;
    let seq = parts.next()?.parse().ok()?;
    let spool = match parts.next()? {
        "-" => None,
        id => Some(SpoolId::from_uuid(id.parse().ok()?).ok()?),
    };
    Some(Commit {
        round,
        n,
        journal,
        seq,
        spool,
    })
}

/// Runs one child until it has committed a few times, then kills it after `delay`. Returns the
/// commits it reported.
fn run_child(dir: &Path, round: u64, delay: Duration) -> Vec<Commit> {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "crash_writer_child",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR, dir)
        .env(CHILD_ROUND, round.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (lines_sender, lines) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if lines_sender.send(line).is_err() {
                break;
            }
        }
    });

    // The test harness prints its own lines too; only commit lines count.
    let mut commits = Vec::new();
    // Wait until the child is writing, so the kill lands during writes, not during startup.
    while commits.len() < 3 {
        let line = lines.recv_timeout(Duration::from_mins(1)).unwrap();
        commits.extend(parse(round, &line));
    }
    thread::sleep(delay);
    child.kill().unwrap();
    child.wait().unwrap();
    reader.join().unwrap();
    commits.extend(lines.try_iter().filter_map(|line| parse(round, &line)));
    commits
}

#[test]
fn committed_writes_survive_the_writer_being_killed() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("secrets")).unwrap();
    let mut all: Vec<Commit> = Vec::new();

    for round in 0..ROUNDS {
        // Delays from 0 to 55 ms, so kills land at different points of a write.
        let delay = Duration::from_millis((round * 17) % 60);
        let commits = run_child(dir.path(), round, delay);
        assert!(
            commits.len() >= 3,
            "round {round}: the child reported its commits"
        );
        all.extend(commits);

        let (store, report) = open(dir.path());
        assert_eq!(
            report.recovered, None,
            "round {round}: the store stayed readable"
        );
        assert!(
            report.spool_files_missing.is_empty(),
            "round {round}: every committed spool row has its file"
        );
        assert!(
            report.spool_orphans_removed <= 1,
            "round {round}: at most one unfinished file"
        );
        assert_eq!(
            store.verify_integrity().unwrap(),
            Vec::<String>::new(),
            "round {round}"
        );
        verify(&store, &all);
    }
}

fn verify(store: &Store, commits: &[Commit]) {
    let journal = store.journal_after(None, usize::MAX).unwrap();
    let outbox = store.outbox_pending(usize::MAX).unwrap();
    assert!(journal.len() >= commits.len());
    assert!(
        journal
            .windows(2)
            .all(|pair| pair[1].id.get() == pair[0].id.get() + 1)
    );
    assert!(
        outbox
            .windows(2)
            .all(|pair| pair[1].seq.get() == pair[0].seq.get() + 1)
    );

    for commit in commits {
        let expected = marker(commit.round, commit.n);
        let entry = journal
            .iter()
            .find(|entry| entry.id.get() == commit.journal)
            .unwrap_or_else(|| panic!("journal entry for {commit:?} is missing"));
        assert_eq!(entry.payload, expected);
        let event = outbox
            .iter()
            .find(|event| event.seq.get() == commit.seq)
            .unwrap_or_else(|| panic!("outbox event for {commit:?} is missing"));
        assert_eq!(event.payload, expected);
        if let Some(id) = commit.spool {
            assert_eq!(
                *store.spool_read(id).unwrap(),
                spool_contents(commit.round, commit.n)
            );
        }
    }
}
