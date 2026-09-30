# Desktop agent decisions

Decisions made while scaffolding the desktop agent (E00-S09). The stack (Tauri 2, Rust core,
per-OS adapters) is fixed by ADR-006.

## Layout

The shell crate lives in `apps/desktop/src-tauri` (Tauri's convention, which its CLI and docs
assume) and the webview UI in `apps/desktop/frontend`, matching the architecture's `frontend/`.
The architecture's domain crates are siblings: `core`, `sync`, `storage` and `platform`
(E09-S01). All crates are members of the root Cargo workspace and share its lints and release
profile.

## Crate boundaries

`scripts/crate-boundaries.mjs` holds the rules and `pnpm lint` checks them against
`cargo metadata`:

- Workspace layering: `platform` depends on nothing of ours; `storage` on `platform`; `core` on
  `platform` and `storage`; `sync` on those and `core`; the shell on all of them. A new crate
  fails the check until it gets a rule.
- Only the shell may use Tauri or Specta, so the domain crates know nothing about the webview.
- Only `platform` may use OS API crates (objc2, windows, x11rb, zbus, ashpd, the keyring
  stores and similar).
- Only `storage` may use a database library (rusqlite).
- Only `sync` may use HTTP clients.

Dev-dependencies are exempt, so tests can use Tauri's mock runtime. This is the desktop
counterpart of the API's module-boundary tests.

## Typed IPC and the allowlist

- Commands are declared once in Rust. `tauri-specta` generates `frontend/src/ipc/bindings.gen.ts`
  from them; `tests/bindings.rs` fails when the committed file is stale.
- Listing commands in the app manifest (`build.rs`) puts app commands under Tauri's ACL.
  `capabilities/main.json` grants the main window its app commands and `core:event`'s
  `allow-listen` and `allow-unlisten`, for the agent's `agent-changed` event, and nothing else:
  it may not emit events. The allowlist test checks that refused calls fail with the ACL's "not
  allowed" error, not merely any error: with `core:default` granted, several core commands get
  past the ACL and fail later on missing arguments, which a looser test would miss.
- `tauri-specta` and `specta` are still release candidates. They are pinned with `=` as their
  authors require; upgrade them together and regenerate the bindings.
- IPC types live in the shell (`src-tauri/src/ipc.rs`) and map from domain types, so the domain
  crates stay independent of the transport.

## Single instance

`tauri-plugin-single-instance` is the first plugin registered. A second launch hands over to the
running agent, which shows its window, and exits. A headless check on Linux showed the second
process exiting at once while the first kept running. The plugin stays on 2.4.x because 2.5
requires Tauri 2.12.

## Encrypted local storage

E09-S02 built `worklume-storage` and the `SecretStore` trait in `worklume-platform`. Since
E09-S09 the agent runtime opens the store in `<local data>/com.worklume.agent/store`, on the
ticking thread, and the tracker owns what it holds.

**Keys.**

- One random 256-bit master key per device, stored in the OS credential store under the app
  identifier (`com.worklume.agent`, entry `local-store-key`).
- The database key and the spool key derive from it with HKDF-SHA256 and separate labels. One
  keychain entry means one access prompt on macOS, and neither derived key reveals the other.
- The key is stored as hex text, because KDE Wallet stores only UTF-8.
- Keys are wiped from memory after use (`zeroize`).

**Credential stores.**

- Built on `keyring-core` 1.x with one store crate per OS, each linked only on its own target:
  the macOS login keychain, Windows Credential Manager, and the Secret Service over D-Bus on Linux
  (GNOME Keyring or KDE Wallet).
- The `keyring` 4.x crate is now a sample wrapper, and its maintainers recommend depending on
  the core and the stores directly.
- The macOS data-protection keychain needs a signed app with a keychain-access-groups
  entitlement, so it is left for signing (E10).
- `OsSecretStore` refuses stores that forget secrets before they are deleted.
- Only a definite "no such entry" counts as missing. A locked, denied or unreachable keychain is
  an error, and the store stays closed. Treating those as missing would replace a key that may come
  back, and lose the data it protects.

**Database.**

- SQLCipher 4.14 (SQLite 3.51) through `rusqlite` with `bundled-sqlcipher-vendored-openssl`, so
  every OS encrypts with the same OpenSSL build and no system library decides whether data is
  encrypted.
- The cost is compile time: OpenSSL builds from source. On Windows it needs Perl, which the
  runners have, and without NASM it builds with `no-asm`.
- Opening checks `cipher_version`, because plain SQLite would silently ignore the key.
- A raw key (`x'…'`) skips the passphrase KDF: the key is already random.

**Durability.** The acceptance criterion is that a crash mid-write loses nothing journaled.

- WAL with `synchronous = FULL`: every commit is flushed before `write` returns. The WAL default
  (`NORMAL`) can lose recent commits on power loss.
- `fullfsync` on macOS, where `fsync` stops at the drive cache.
- A spool file is written under its final name with `create_new`, flushed (Rust's `sync_all` uses
  `F_FULLFSYNC` on macOS), and its directory flushed, all before the transaction that indexes it
  commits.
- Opening deletes files without a row and reports rows without a file.
- `tests/crash.rs` runs the test binary as a writer, kills it at varying moments over 12 rounds,
  and checks every reported commit, the integrity checks, and the spool.
- A process kill cannot imitate a power cut. The power-cut guarantee rests on SQLite's documented
  behaviour with these settings; a real power-cut check belongs in the release test matrix.

**Schema.**

- Migrations are SQL files applied in order, tracked by `user_version`, each in its own
  transaction. They are append-only.
- A schema newer than the agent is refused, not downgraded.
- Tables are `STRICT` with CHECK constraints: dotted event names, JSON-object payloads up to
  256 KiB, 16-byte IDs, non-negative clocks.
- `seq` and journal IDs use AUTOINCREMENT, so they are never reused after rows are deleted.
- `store_info.store_id` names the outbox sequence. A store created from scratch gets a new one,
  so the server can tell a restarted `seq` from a replay (E09-S05).

**Recovery.**

- If the credential store answers that it has no key while a database exists, or the entry is
  not a key, or the key does not decrypt the database, the database and spool move to
  `quarantine/<ms>/` and a new store starts. `OpenReport::recovered` says why, so the agent can
  tell the member.
- Only the latest quarantine is kept.
- The data in quarantine cannot be decrypted without its key; it is kept for diagnosis.

**Other rules.**

- A lock file (`File::try_lock`) keeps a second process out.
- On Unix the store directory is `0700` and spool files `0600`.
- Temporary tables stay in memory.

**Caps.**

- The defaults are 2 GB and 30 days (architecture §13). The server's policy can override them
  (E09-S06).
- `enforce_caps` evicts spool files only: first files past the age limit, then the oldest until
  the store fits. Outbox events and the journal record time worked and are never evicted.
- The eviction report lists what went, so the agent can warn the member and report the gaps
  (E09-S07).

**Tests against real keychains.** `platform/tests/os_secret_store.rs` is ignored by default. The
desktop workflow runs it with `--ignored` on macOS and Windows, and on Ubuntu inside
`dbus-run-session` with GNOME Keyring unlocked.

## Watchdog and diagnostics

E09-S11. The code is in `src-tauri/src/watchdog.rs`, `src-tauri/src/diagnostics/` and
`src-tauri/src/startup.rs`.

**Watchdog.**

- Release builds start as a supervisor process that runs the agent as a child. The child is the
  same executable with `WORKLUME_SUPERVISED=1` set, so arguments pass through unchanged.
- The agent's exit status decides what happens:
  - success means the member quit, or a second launch handed over to the running agent, so the
    supervisor exits too;
  - exit code 75 asks for an immediate restart (the updater, E09-S10), which is not a crash;
  - anything else, including being killed, is a crash, restarted after 1 second.
- Crash-loop protection: a crash that follows 5 restarts within 5 minutes stops the supervisor.
  It logs the crash loop and reports it.
- The member sees the tray icon disappear. An in-app explanation belongs to the agent UI
  (E09-S09).
- A failed launch, for example an executable removed mid-update, counts as a crash.
- Debug builds run the agent directly, so `tauri dev` can restart it freely.
  `WORKLUME_WATCHDOG=on` tries the supervisor in a debug build; release builds ignore the
  variable, so the watchdog cannot be switched off there.
- A 1-second delay leaves time for a logout, which also stops the supervisor, to win the race
  against a pointless restart.
- If the supervisor itself is killed, the agent keeps running unsupervised until the next launch.
  The optional Windows service (E11-S07) covers that case on Windows.
- Checked end to end under Xvfb: `kill -9` on the agent brought a new agent after about one
  second, and the sixth kill within the window stopped the supervisor.

**Logs.**

- One JSON object per line, in daily files per process (`agent.<date>.log`,
  `supervisor.<date>.log`), with 7 days kept.
- They go to Tauri's app log folder: `~/Library/Logs/com.worklume.agent` on macOS, and
  `<local data>/com.worklume.agent/logs` elsewhere. The supervisor computes the folder without a
  Tauri context, and a test keeps it equal to Tauri's `app_log_dir`. On Unix the folder is
  `0700`.
- Writes are synchronous, not through a background thread, so the lines just before a crash are
  on disk when the watchdog restarts the agent.
- The default filter is `info` for Worklume's crates and `warn` for dependencies. `log` records
  from Tauri and the webview are bridged in. `WORKLUME_LOG` overrides the filter.
- Privacy: log states, counts, durations and error kinds, never window titles, URLs, file names,
  payloads, keys or tokens. As a safety net, the home folder path is replaced with `~`, both as
  written and in the JSON-escaped form it takes on Windows.
- Panics are logged with their location.

**`SQLCipher`'s log.**

- It goes to `sqlcipher.log` in the log folder at warning level, instead of stderr.
- The level is set before the destination, so `SQLCipher`'s informational line quoting the path
  (which contains the account name) is never written.
- At warning level, `SQLCipher` names files only during `cipher_migrate`, which the agent does not
  use.
- If the file cannot be opened (on Windows, `SQLCipher` opens paths through the ANSI code page),
  its log is switched off and a warning is logged.

**Crash reports.**

- Sentry, sent through `worklume_sync::http`, the agent's HTTPS client. It uses native TLS with
  the operating system's certificate store (Keychain, the Windows store, `/etc/ssl`), so reports
  get through corporate proxies that inspect TLS with their own root certificate. Sentry's
  default client would trust only a bundled list. Sign-in and sync (E09-S03, S05) reuse the same
  client, and HTTP stays in `sync`, as the crate boundaries require.
- Two kinds are sent: Rust panics in either process, and the supervisor's report of an agent that
  died (exit code or signal, uptime, recent restarts, grouped by way of dying). The supervisor's
  reports cover native crashes in the webview or OS libraries.
- Nothing identifies the member or machine: `send_default_pii` is off, there are no breadcrumbs,
  and a `before_send` hook removes the hostname, user, request, device name, module list and
  debug images, and shortens home paths in messages, tags, extras and stack frames.
- The DSN comes from the build environment (`WORKLUME_SENTRY_DSN`), so nothing is hard-coded and
  builds without it send nothing. Release builds get it from a CI secret with the updater
  (E09-S10).
- Letting an organization switch crash reports off belongs with policy config (E09-S06).

**Scope changes from the backlog.**

- Native memory dumps (crashpad or breakpad) are not collected. The memory of an
  employee-monitoring agent can hold screenshots, URLs and window titles, so dumps need a privacy
  decision first. Until then, the supervisor's crash reports record native crashes.
- The log bundle upload waits for the server command channel (E09-S05) and
  `POST /v1/desktop/logs/upload-url`. The server asks for the bundle with an `upload_logs`
  command; the bundle is the files described above, capped at 20 MB.

## Tracker

E09-S04: `worklume-core::tracker`, the timer's state machine. The timekeeper (next section)
drives it on the device's clock, and since E09-S09 the agent runtime ticks it every second in
the app (see "Agent UI"). The one-store-per-organization setup arrives with sign-in (E09-S03).

**States.** Stopped, running, idle-pending, paused and on-break.

- `start`, `switch` and `stop` are the member's timer controls.
- `idle_detected` and `resolve_idle` (keep, discard, reassign or stop, decided by the member or
  by the policy) record idle time.
- `pause` and `resume` handle sleep or a locked session, and are safe to repeat. A pause while
  idle time awaits a decision keeps the decision waiting: the idle period ends with the entry,
  and `resume` and `stop` are refused until the decision.
- `start_break` and `end_break` handle breaks.
- `tick(now, activity)` accounts time.
- Refused actions change nothing and write nothing. `tests/transitions.rs` holds the full table.
- The tracker takes idle detection and sleep as inputs; the timekeeper decides when they happen.

**Journal first.**

- Each call works on a copy of the tracker. The copy's snapshot and the events it produced are
  saved in one store transaction, and only then does the copy go live.
- A failed save changes nothing: the next tick accounts the same time again, and slot IDs are
  kept, so a retry never duplicates a slot.
- The journal holds a single snapshot. While time counts, a new one is written at least every
  30 seconds (the heartbeat), which bounds what a crash can lose.

**Time.**

- Durations come only from the monotonic clock.
- Wall time is derived from one anchor per boot: `anchor.wall + (now.mono − anchor.mono)`. Slot
  windows and event times use it, so changing the system clock neither adds nor removes time;
  it only flags the slot `clock_adjusted` (more than 60 seconds apart).
- The server corrects the anchor's offset with the clock offset the sync engine measures
  (E09-S05).

**Gaps and restarts.**

- If no tick arrives for over a minute while time counts, the gap isn't counted: the entry
  closes where ticks stopped (`timer.stopped`, reason `gap`) and a new one opens
  (`timer.started`, trigger `gap_end`).
- On restart, `Tracker::open` restores the snapshot. If time was counting, the entry closes at
  the snapshot (reason `agent_restart`), a new one opens (trigger `restart`) and the timer
  carries on. `Recovery` reports the unaccounted time.
- After a reboot, derived time re-anchors to the wall clock, but never before the last
  accounted moment, so no 10-minute window comes around twice, even when the clock was set back
  while the device was off (the slot is flagged `clock_adjusted`). Open slots stay open: if the
  new boot lands in the same window, it adds to the same slot, dated in the new boot.

**Slots.**

- 10-minute UTC windows, at most one per window, matching the server's unique key.
- A slot closes only once its window has passed, so stopping and restarting within a window adds
  to the same slot.
- Each slot holds tracked, active, keyboard, mouse and idle seconds. Idle counts from detection,
  and activity is clamped to tracked time.
- It is attributed to the entry with the most time in it (the later entry on a tie).
- It is flagged `partial_slot` when not fully tracked; slots under one whole second aren't sent.
- A small ledger (E09-S09) keeps the tracked time per window for two days, for the agent's
  "today" total. Idle time is pending from where inactivity began until the decision: keeping
  or reassigning it keeps it, discarding or stopping drops it. Within an entry the timer runs
  without gaps, so the idle time already counted in a window is exactly its part from where
  inactivity began. A window counts towards the day it starts in.
- Seconds are rounded on running totals, not per slot, so a day's slots add up to the tracked
  time within one second. That is the acceptance criterion, checked by
  `tests/simulation.rs`: two seeded 24-hour runs with jitter, missed ticks, random actions,
  clock changes and three crashes.

**Events and the server.**

- The event vocabulary is the contract's `DesktopEvent` schema (`POST /v1/desktop/sync`).
  `docs/conventions/api-contract.md` explains how the agent and the contract are kept in step.
- Every `timer.stopped` stops the server's timer, and every `timer.started` starts it, so a
  paused member shows as not tracking, and stopping while paused sends nothing.
- Idle resolutions are applied by the server, as `POST /v1/idle-periods/{id}/resolve` would, so
  a resolution made on the web cannot double-count. `decided_by` says whether the member or the
  keep-idle policy decided, and the server checks the decision against the member's policy.
- An idle period covers all tracked time from its start until the decision. When a restart or
  a gap splits the entry meanwhile, it carries on into the new entry, and a `stop` decision
  ends whichever entry is open.
- The server's `timers.stop_reason` needs the agent's reasons: `sleep`, `session_locked`,
  `break`, `gap` and `agent_restart` (E07-S03, E08-S10).

**Test support.**

- `worklume-platform`'s `fake` feature provides `MemorySecretStore`, `FakeClock` and `FakeIdle`.
- `worklume-storage`'s `fault-injection` feature adds `Store::fail_next_writes`.
- Both are dev-dependency features only; the app never enables them.

## Idle and sleep

E09-S08: `worklume-core::timekeeper` runs the tracker on the device's clock, detects idle time
and handles sleep. `worklume-platform` provides the `Clock` and `IdleSource` traits.

**One timeline.** Every timer action goes through the `Timekeeper`. Before any action it reads
the clock, handles a sleep that happened since the previous reading, and accounts time up to
now. A click just after waking therefore lands after the sleep, never before it.

**The clock.** The tracker needs a monotonic clock that keeps counting while the device sleeps:
wall time is derived from it (anchor plus monotonic delta), and Rust's `Instant` stops during
sleep on Linux and macOS.

- `OsClock` reads, on each OS, one counter that includes sleep and one that doesn't. Both start
  at boot, so their difference is exactly the time spent asleep.
  - Linux: `CLOCK_BOOTTIME` and `CLOCK_MONOTONIC`.
  - macOS: `CLOCK_MONOTONIC_RAW` and `CLOCK_UPTIME_RAW`.
  - Windows: `QueryInterruptTime` and `QueryUnbiasedInterruptTime`.
- Boot IDs:
  - Linux uses `/proc/sys/kernel/random/boot_id`.
  - macOS uses `kern.bootsessionuuid`.
  - Windows documents no boot ID, so the agent keeps one in a volatile key,
    `HKCU\Software\Worklume\BootSession`. Windows drops volatile keys at sign-out and restart.
    A sign-out also changes the ID, which only closes open slots early.
- The tracker also treats a monotonic clock that went backwards as a new boot. That covers a
  boot ID that failed to change, for example a tampered registry key.

**Sleep comes from the clock, not from power notifications.** The architecture names a
`PowerEvents` trait for sleep and wake. The agent reads sleep from the clock instead, for four
reasons:

- The first reading after waking knows how long the sleep lasted.
- It can't be missed while the agent is busy or restarting.
- It needs no message loop, D-Bus connection or permission.
- It works the same on every OS.

A locked session is read the same way, as a state on every tick rather than through events
(the `SessionLock` trait, E12-S03): see "Locked session" below.

**Idle.** While the timer runs, inactivity is the time since the last input. It counts at the
earliest from the last decision about idle time, or from the timer's start. When inactivity
reaches the timeout, idle time is recorded from where it began. The idle source's adapters come
with the per-OS input stories (E10-S03, E11-S03, E12-S03, E13-S04). When the source can't say
(`IdleError`), nothing is detected: unknown is never read as idle.

**Sleep.** The entry closes at the last reading before the sleep (`timer.stopped`, reason
`sleep`), and the second or so around the sleep isn't tracked. On waking, the member's absence
is the inactivity before the sleep plus the sleep itself.

- Shorter than the idle timeout: the timer resumes in a new entry.
- Longer: the timer waits for the member, who is asked once.
  - Inactivity of a second or more before the sleep, inside the entry, becomes idle time
    awaiting a decision. Answering it also resumes the timer, unless the answer is `stop`.
  - Otherwise the only question is whether to resume.
  - If idle time was already awaiting a decision, that question stands; it is not asked again.

**Locked session** (E12-S03). A lock is an absence like a sleep, except that the device stays
awake, and it matters for privacy: on X11 the lock screen's input, a password being typed,
still reaches the agent (`docs/spikes/E12-S01-x11.md`).

- The timekeeper reads `SessionLock::locked()` on every call. A reading that fails keeps the
  state last known, so a flaky reader never unlocks; `Unsupported` never locks.
- On locking, the entry closes at the reading before the lock was seen (`timer.stopped`, reason
  `session_locked`), so locked time is never tracked. Idle time awaiting a decision stays the
  question, paused.
- On unlocking, the absence (inactivity before the lock plus the lock) decides as after a
  sleep: shorter than the idle timeout, the timer resumes in a new entry (trigger `resume`);
  longer, the member is asked whether to resume, and under `never` the timer stops. A lock that
  began before the agent started has an unknown length, so the member is asked. Without idle
  detection every unlock resumes.
- A device that sleeps while locked, or locks as it sleeps (a closed lid), resumes only once
  unlocked.
- Questions about resuming, and the `never` policy's stop, wait until the session is unlocked:
  nobody could see a question behind the lock screen, and the absence's length isn't known yet.
- **No input from locked time is kept.** Every event up to a reading that found the session
  locked, or still locked, is dropped, including one the reader hands over late. So is any
  input in the second the unlock is seen: it can't be told from the password.

**Keep-idle policy** (`idle.keep`, delivered with the config in E09-S06):

| Policy   | Idle while awake                                | Long absence with a sleep                      |
| -------- | ----------------------------------------------- | ---------------------------------------------- |
| `prompt` | Asks the member                                 | Asks about the idle time, or whether to resume |
| `always` | Kept when input returns, without asking         | Idle kept; asks whether to resume              |
| `never`  | Discarded, and the timer stops where idle began | The timer stops; nothing is asked              |

A long lock is decided as a long absence with a sleep.

- A policy decision sends `idle.resolved` with `decided_by: policy`.
- A timeout of `None` turns idle detection off. Sleep is still never tracked, and every sleep
  then resumes on its own.
- The policy comes from the server and is never hardcoded. `set_policy` applies a new policy
  from the next call.

**Prompts.** `Timekeeper::prompt()` is the question the idle dialog (E09-S09) shows.

- `Prompt::Idle` offers keep, discard, reassign or stop.
- `Prompt::Resume` offers resume or stop, after a long sleep or lock.
- Each call's `Outcome` reports a new question, for a notification. A question that outlives a
  sleep counts as the same question.

**Tested.** The acceptance criterion is that a two-hour sleep tracks no time and asks once at
most.

- `core/tests/timekeeper.rs` checks it under all three policies, for a member who was working,
  briefly away or long away before the sleep.
- Other tests cover short sleeps, answers, clicks just after waking, restarts, policy changes,
  and failed clock reads and saves.
- `core/tests/lock.rs` covers locks: short and long, each keep-idle policy, inactivity before
  the lock, a pending idle question, sleeps while locked, failed readings, restarts, no idle
  detection, and no input kept from locked time even when handed over late.
- `platform/tests/os_clock.rs` checks the real clocks on each OS in CI.
- A real sleep can't happen in CI, so sleep detection on real hardware belongs in the release
  test matrix.

## App and URL usage

E09-S12: `worklume-core::usage`, with the `ActiveWindow` and `BrowserUrl` traits in
`worklume-platform`. It records what the member had in front of them while the timer ran,
per 10-minute slot, reduced to what the organization allows before anything is stored.

**Levels** (`UsagePolicy`, from the agent's configuration, E09-S06; `OFF` until it arrives):

| Level     | Kept                                                                                 |
| --------- | ------------------------------------------------------------------------------------ |
| `off`     | Nothing, and no sensor is read                                                       |
| `apps`    | The foreground app: identifier, name                                                 |
| `domains` | Also the website, as its registrable domain (`mail.example.co.uk` → `example.co.uk`) |
| `paths`   | Also the URL path                                                                    |

- **Never kept:** query strings, fragments, credentials, addresses that aren't web pages
  (`file:`, `chrome:`, `about:`…), and a private window's title or path.
- **Window titles:** only when the organization turns them on (off by default, ADR-014).
- **Private windows:** keep their domain only, or nothing, by policy (the acceptance
  criterion). The browser still counts as the app in front.
- **Domains:** reduced on the device with the Public Suffix List (the `psl` crate, compiled
  in). A host with no public suffix (`localhost`, intranet names) or an IP address is kept
  whole. Internationalised names are kept in their ASCII (punycode) form.
- **Limits:** titles are cut at 256 characters and paths at 512. Each slot keeps up to 50
  distinct apps and 50 sites in full detail. Beyond that, a new one loses its title or path
  and merges with the same app or site, up to 100 records.

**Only what's kept is read.** The window title is asked for only when titles are on. The
browser tab is read from `domains` up, and also at `apps` when titles are on, to learn
whether the window is private. A tab that can't be read counts as private, so its title is
dropped. An unreadable foreground app records nothing for that tick; tracking goes on.

**When usage counts.** Only while the timer runs and the member isn't idle. The timekeeper
reads the sensors only then, and the tracker also leaves idle time out.

**Stored with the slot.** Usage is tallied in the tracker's open slots, so it is part of the
journal snapshot and survives a crash like the slot's time. When a slot closes, it is followed
in the same transaction by an `app_usage.batch` and a `url_usage.batch`, sent only when not
empty. Each references the slot by `slot_id`, and holds whole seconds per record, rounded down,
so usage never exceeds the slot.

**Contract.** Both event types are in `packages/api-contract/openapi.yaml`. They state the
server's duties (E16-S02): catalog matching, re-applying the member's current policy, and who
may read the data (`activity.view_apps`, `activity.view_urls`).

**Open decision.** The data model keeps websites as registrable domains, so `docs.google.com`
and `mail.google.com` both become `google.com`, even at the `paths` level. The agent follows
the model. Whether to keep the host as well is for E16-S01 to decide.

**Tested.** `core/tests/usage.rs` covers each level, titles, private windows, non-web
addresses, idle, switching apps, unreadable sensors, a restart, many titles and
project switches. `usage::tests` covers URL reduction. The simulation harness checks usage in
every generated script. Mutation checks: 10 of 10 caught.

## Activity

E12-S03: `worklume-core::activity`. A second is active when the member used the keyboard or
mouse in it (Phase 1, "Activity %"), and a slot's activity is its active time over its tracked
time. Keyboard and mouse are measured the same way on their own.

- **From input events** where the OS reports them. An input makes the second after it active.
  Counting from each input rather than on whole clock seconds means a tick boundary never
  splits or double-counts a second: a single key press is one active second however the ticks
  fall, and continuous input makes the whole time active. Keys count for the keyboard;
  pointer moves, clicks and scrolls for the mouse.
- **From the idle source** where there is no working event reader (Wayland, a denied
  permission, a reader that failed): the combined figure. Each reading contributes the moment
  of the last input, so continuous input still covers the whole time. Keyboard and mouse are
  unknown, which makes the slot's `keyboard_seconds` and `mouse_seconds` null.
- The meter remembers the last input of each kind, so a second that began in one tick counts in
  the next. A sleep, a lock or a new boot makes it forget.
- `Timekeeper::tick_measured` and every timer action measure activity this way; the app and the
  simulation harness use them. `Timekeeper::tick(activity)` takes a given figure, for tests.
- Slots report how input was measured: `hooks` with an event reader, `idle_poll` without.

## Input timing features

E09-S13: `worklume-core::input`, with the `InputActivity` trait in `worklume-platform`. Each
slot carries statistics about the rhythm of input, so the server's automated-input detector
(E33-S03) can tell a person from a mouse jiggler or an auto-clicker. No keystroke log is
needed: no event time, key or pointer position ever leaves the device (the acceptance
criterion).

**The reader.** `InputActivity::drain` returns the events since the last call: kind (key,
pointer, click, scroll) and monotonic time, nothing else. The X11 reader arrived with E12-S03
(see "Linux sensors"); macOS and Windows follow with E10-S03 and E11-S03. They must stamp events with the agent's clock (OS event times count on
other clocks: macOS's stop during sleep), skip key auto-repeat, and bound their buffer.
Wayland has no global input events, so its reader reports `Unsupported`.

**What counts as one input.** A key press, a click, a pointer movement or a scroll gesture.
Pointer and scroll reports less than 250 ms apart are one movement: a mouse reports its
position many times a second at a steady rate, which would make every hand look like a
machine.

**Intervals** are measured between consecutive inputs of any kind, and of each kind, only
while the timer runs and the member isn't idle. One over 60 s is a pause, not a rhythm, and
isn't counted. Idle time, a pause, a break, a gap, a reboot or an agent restart forgets the
previous input, so no interval spans one. An interval belongs to the slot of the input that
ends it.

**Kept and sent.** A slot keeps only totals: inputs per kind, and per interval kind a histogram
of 16 log2 buckets of milliseconds with count, sum and sum of squares. The last input's time,
needed to measure the next interval, stays in memory and is never saved. `slot.closed` sends
less still, as `input_features` (version 1):

| Field            | Meaning                                                                   |
| ---------------- | ------------------------------------------------------------------------- |
| `events`         | Inputs per kind                                                           |
| `keyboard_share` | Key presses as a share of all inputs                                      |
| `intervals.*`    | For `any` and each kind: `count`, `entropy_bits` (0–4) and `cv` (SD/mean) |

Statistics are null below 5 intervals, too few to judge. The whole object is null when the
slot had no input the agent could see. Values are rounded to 3 decimals. The histogram itself
isn't sent: typing rhythm is a biometric, so only what the detector needs leaves the device.
A jiggler or an auto-clicker repeats one interval, so its entropy and variation are 0; a
person's are well above.

**The timekeeper** drains the reader on every call, whatever the state, so the reader never
holds more than a call's worth. Events are handed to the tracker with the time they fall in.
If the call fails, they wait for the next one, up to 120,000 events. Events drained after a
sleep happened around it, in untracked time, are dropped, and so are events from locked time
(see "Locked session").

**Open decision.** The architecture has the agent always send the features. Whether they
should be sent, or kept by the server, only when an organization turns automated-input
detection on is for E33-S01 (the organization's AI switch) to decide.

**Tested.** `core/tests/input.rs`:

- no input's time, monotonic or wall, is in the journal or the outbox, across a restart and a
  sleep (the acceptance criterion);
- the saved state doesn't grow with input;
- a jiggler and a person give different features;
- input counts only while the timer runs and the member isn't idle;
- no interval spans a stop, a restart or a sleep;
- an interval belongs to the slot of its later input;
- a failed save counts its input once, and input around a sleep is dropped;
- a reader that can't see input sends null.

`input::tests` covers movements, pauses, entropy and variation. The contract tests refuse
extra fields such as event times or buckets. The harness types and points unevenly on every
`Work` step, has a `Jiggle` step, and checks the features in every script. Mutation checks: 11
of 11 caught.

## Linux sensors

E12-S03 and E12-S04, after the spike in `docs/spikes/E12-S01-x11.md`.
`worklume-platform::linux`:

- **`X11Idle`**: the MIT-SCREEN-SAVER extension's `ms_since_user_input`, under 0.3 ms a
  reading.
- **`X11Input`**: `XInput` 2.2 raw events on the root window, read on a thread of its own with
  its own connection.
  - Kinds: a key press is `Key`; buttons 4 to 7 (the wheel) are `Scroll`, the others `Click`;
    motion is `Pointer`, unless it moves only a device's scroll valuators (touchpads,
    high-resolution wheels), which is `Scroll`. The buttons the server emulates for such
    scrolling are skipped, so one scroll counts once. Scroll valuators are read per device
    and again when devices change.
  - The key code is never read. Auto-repeat doesn't reach raw events; a press flagged as a
    repeat is skipped anyway.
  - Each event is stamped with the agent's clock when it arrives, and kept until the agent
    drains it, up to 20,000 (the oldest go first).
  - The thread waits on the connection with a half-second timeout (`rustix::event::poll`), so
    dropping the reader stops it promptly. A reader whose connection fails reports it on the
    next drain, and activity falls back to the idle source.
  - Absolute XTEST motion, which some remote-desktop servers inject, produces no raw motion.
- **`X11Window`** (E12-S04): the window in front, for app usage (E09-S12).
  - `_NET_ACTIVE_WINDOW` on the root, which EWMH window managers set. A 0, or no value yet from
    a window manager whose `_NET_SUPPORTED` lists it (Openbox sets it only once a window is
    first activated), means nothing is in front. A window manager that doesn't list it doesn't
    follow EWMH, and the reading is `Unsupported`.
  - The app is its desktop-file ID and `Name`, from the XDG entries (`linux::desktop`):
    `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`, then the Flatpak and Snap exports, the earlier
    folder winning and a `Hidden` entry deleting the app. A window matches, strongest first, by
    the toolkit's app ID (`_GTK_APPLICATION_ID`, `_KDE_NET_WM_DESKTOP_FILE`), by
    `StartupWMClass` against its class or instance, by an ID equal to its class or instance in
    lower case, or by the one entry whose `Exec` runs its executable. The executable comes from
    X-Resource, the server's record of the client, not the client-set `_NET_WM_PID`.
  - Without an entry, the class names the app (`chromium-browser`), and without a class the
    executable. **The instance is used only to match**: Chromium puts its profile folder there.
  - Each window's app is cached with the `WM_CLASS` it had, and looked up again when that
    changes, because the server reuses closed windows' IDs. When nothing matches, the entries
    are read again at most every five minutes, for apps installed since.
  - The title (`_NET_WM_NAME`, else `WM_NAME`, `STRING` read as Latin-1) is read only when the
    policy keeps titles.
  - A reading takes 0.15–0.6 ms for a new window and about 35–80 µs cached. Checked on Xvfb with
    Openbox against a real xterm (`debian-xterm`, "XTerm", from its installed entry) and
    Chromium.
  - Browser addresses on X11 wait on the decision about the browser extension (E35); see the
    spike.
- **`LinuxSessionLock`** (X11 and Wayland alike): logind's `LockedHint` on
  `/org/freedesktop/login1/session/auto`, the caller's own session or the member's graphical
  one, falling back to `org.freedesktop.ScreenSaver` or `org.gnome.ScreenSaver` `GetActive` on
  the session bus. One D-Bus call per reading, with a 250 ms timeout; a failed call reconnects
  on the next reading. No service at all reads as `Unsupported`.
- **`DeviceSensors::start`** picks the adapters for this session: on Linux the lock always, and
  on X11 idle time, input and the window in front too. What fails to start is logged once per attempt and reads as
  `Unsupported`, and the agent tracks without it.

**Decision: the `dbus` crate, not `zbus`.** The architecture names `zbus` for the Linux
modules. The keychain adapter already links the system `libdbus` through the `dbus` crate, and
the lock needs one blocking property read a second, so the agent keeps one D-Bus stack. `zbus`
remains the choice if the Wayland work (E13) needs async D-Bus or portals.

**Tested.**

- Unit tests classify raw events: repeats, buttons, the wheel, smooth scrolling and valuator
  masks. Others read desktop entries (only the `[Desktop Entry]` group, quoted `Exec` paths,
  shared launchers) and match windows in order, with folder precedence and hidden entries.
- `platform/tests/x11.rs` runs against a real X server, with input faked through XTEST: idle
  time, every kind of input stamped with the agent's clock, input still arriving while
  another client holds the keyboard (why locked time drops input), the reader stopping when
  dropped, and `DeviceSensors` picking the X11 adapters. The window tests act as the window
  manager, creating windows and setting `_NET_ACTIVE_WINDOW` themselves: the app from its
  entry, the toolkit's ID and the executable, the class without an entry, a Latin-1 title, a
  title only when asked, a window whose class changes, nothing in front, and no EWMH. A path in
  the instance never comes out. The tests keep one connection open
  throughout, because Xvfb resets when its last client leaves and refuses connections while it
  does.
- The lock is tested against stand-in logind and screen saver services on a private session
  bus.
- These tests are ignored in ordinary runs. The `Desktop` workflow runs them on Ubuntu with
  `xvfb-run -a dbus-run-session -- cargo test -p worklume-platform --features fake --lib --test
x11 -- --ignored`.
- Not covered yet: real GNOME and KDE sessions, real mice and touchpads, and remote desktops
  (the E12 test matrix).

## Agent UI

E09-S09, part 1: the timer screen. The window shows what the agent reports and asks it to act;
it holds no tracking state and decides nothing (architecture §5).

**Core.**

- `worklume-core::catalog`: the projects and tasks the member may track, and the organization's
  break policies. Every start, switch, break or reassignment from the UI names its target by ID,
  and the agent checks the ID against the catalog before acting. The webview is a client like
  any other. The server checks again when the events arrive (`project_unavailable`).
- `worklume-core::view::timer_view`: the `TimerView` the UI shows. It carries the status, the
  work with names, today's total, the question being asked, and the actions allowed now (stop is
  refused while idle awaits an answer; start needs a catalogued project).
- Today's total comes from the tracker's two-day ledger (see "Tracker"), counted from the
  device's local midnight (`jiff`).

**Shell.**

- `agent::Agent` owns the timekeeper behind a mutex, with the catalog. It is created in the
  `Starting` state and managed by Tauri.
- A background thread (`ticker.rs`) opens the store, so a slow or prompting keychain never holds
  up the window. It then ticks every second and emits `agent-changed` when the view changes, and
  brings the window forward when the agent starts asking a question.
- If the store can't open, the agent is `Unavailable` with a reason the UI explains (keychain,
  store in use, store from a newer version, storage, clock, no data folder). The thread retries
  every 30 seconds, and a failure is logged once until it changes.
- The sensors come from `worklume_platform::DeviceSensors` (see "Linux sensors"). On Linux X11
  the app measures idle time, input and activity (`input_method: hooks`) and pauses when the
  session locks. Elsewhere, sensors without an adapter yet read `Unsupported`: the app tracks
  time without idle detection, app usage or input features, and reports
  `input_method: idle_poll`.
- The idle policy and the catalog start empty. The configuration (E09-S06) sets them through
  `Agent::set_idle_policy` and `Agent::set_catalog`. Until then the window says "No projects
  yet".

**Commands** (typed, in `ipc.rs`): `agent_state`, `work_catalog`, `timer_start`, `timer_switch`,
`timer_stop`, `timer_resume`, `break_start`, `break_end` and `idle_answer`. Timer commands
return the new view, or a `CommandError`: `unavailable`, `not_now`, `unknown_project`,
`unknown_task`, `unknown_break_policy`, `invalid_id` or `failed`.

**UI.**

- React with i18next, as in the web app. English strings live in
  `frontend/src/i18n/locales/en.json`, with typed keys.
- The timer screen has the status (a polite live region), the work, today's total, the
  project/task picker, and stop and break controls, each shown only when the view allows it.
- The idle question and the resume question after a long sleep are native modal dialogs: focus
  starts on the first answer and stays inside, the rest of the window is inert, and Escape
  doesn't dismiss them. A failed answer is reported inside the dialog.
- **Accessibility (the acceptance criterion):** native controls with labels, a visible focus
  ring, no keyboard traps outside the questions. `App.test.tsx` walks the start flow by
  keyboard alone and runs axe on every state.

**Checked end to end.** A headless run on Linux (Xvfb, D-Bus and an unlocked GNOME Keyring)
showed the window following the agent: "can't track" with the keychain locked, and the timer
once the store opened.

**Tested.**

- `core/tests/view.rs`: the view through the states, idle time counting until discarded,
  questions and today's total across a restart.
- Ledger and catalog unit tests.
- `src-tauri/tests/agent.rs`: the runtime on fake sensors.
- `ipc_allowlist.rs`: IDs are checked, events can be listened to but not emitted, and other
  windows are refused.
- Frontend tests with axe and keyboard walks.
- The harness compares the agent's total with the server-side timeline after every script.
- Mutation checks: 13 of 13 caught.

**Part 2** waits on what it shows:

- the permission wizard (per-OS permission health, E10-S06 and the Windows and Linux stories);
- "my data" (screenshots, E09-S07, and the member's activity from the server);
- settings (the configuration, E09-S06);
- the offline indicator (the sync engine, E09-S05).

## Simulation harness

E09-S14: `apps/desktop/simulation` (`worklume-simulation`) runs the real agent core on a
simulated device. Its scripted and generated scenarios check the agent end to end, from the
store to what the server received.

**The device.** A `Device` runs the real `Timekeeper`, tracker and encrypted store in a
temporary folder, on `FakeClock`, `FakeIdle`, `FakeWindow`, `FakeBrowser`, `FakeInput`,
`FakeLock` and `MemorySecretStore`. Activity is measured from its input events, as in the app.
It can:

- work (input every tick: uneven typing, clicks, mouse movements and scrolls, the same on
  every run), be away, be away with a jiggler running, or sleep;
- lock the session (E12-S03), while someone types at the lock screen and the member types
  their password to unlock. That typing comes from separate hands, left out of the ledger of
  inputs made, so any of it reaching the server shows as more inputs than were made;
- have its wall clock moved, or reboot;
- lose its agent to a crash, including in the middle of a store write (the store's
  `fault-injection` feature). The watchdog's one-second restart is modelled.
- go offline, or lose sync answers after the server applied a batch.

It records when it slept, was locked or ran no agent, in monotonic time. The member's actions go through
the timekeeper as the UI's would; actions refused in the current state are logged and skipped,
like a disabled button.

**The server.** `FakeServer` stores each event once by `event_id`, reports repeats as
duplicates, and flags a reused ID or a `seq` out of order. The sync engine (E09-S05) doesn't
exist yet, so `Device::sync` stands in for it: ordered batches of up to 500 events, deleted
from the outbox only once the server answered. The real engine replaces this when it arrives.

**The checker.** `check` folds what the server stored into entries, idle periods, breaks and
slots, and reports:

- a `seq` gap, or a repeated event ID;
- an event whose wall time isn't its boot's monotonic time plus one fixed offset (a clock
  change leaking into event times);
- two open entries, a stop for an entry that isn't open, or an entry ending in another boot or
  before it began;
- any entry overlapping a sleep, a locked session or a stretch with no agent;
- an idle period outside its entry, resolved twice, overlapping another, or ended by a pause
  with less than a second in it;
- a break while tracking;
- a second slot for a window, a slot outside 1–600 s or not on a 10-minute boundary;
- a slot's active, keyboard or mouse time outside its tracked time, or only one of keyboard and
  mouse known;
- slots that disagree with the entries by more than a second;
- a usage batch with no slot before it, a second batch for a slot, or more usage than the slot
  tracked;
- usage detail beyond the most any policy in the run allowed: titles or paths when none was
  allowed, a private window's title or path, or a query or fragment in a path;
- input features that contradict themselves (a wrong share, more intervals than inputs,
  statistics from fewer than 5 intervals, values out of range), or slots counting more inputs
  of a kind than were made;
- (in `run`) an agent "today" total that differs by a millisecond from what the server keeps
  over the agent's two days: the entries less the idle time discarded or stopped (E09-S09);
- anything left open at the end, or events that never reached the server.

`tests/checker.rs` damages a clean day in each of these ways and expects the report, so the
checker can't pass everything by accident.

**Scenarios** (`tests/scenarios.rs`):

- a working day;
- a week offline that arrives complete and once, even when answers are lost;
- a sleep under each keep-idle policy;
- clock changes;
- crashes, including one mid-write;
- a reboot;
- a jiggler that keeps the timer running: no idle time, but its slots have entropy and
  variation 0 while the member's don't;
- a long and a short lock: locked time never tracked, every input made at the keyboard counted
  and none from the lock screen;
- a person at the keyboard measuring 90–100% active.

**Generated scripts** (`tests/properties.rs`): proptest builds scripts of up to 40 random
steps under random idle and usage policies, with the member switching between an editor,
browser tabs (some private), the desktop and an app the OS won't describe, and sometimes
leaving a jiggler running. It shrinks a failure to a short script. The report prints the
script and a log of what happened. `PROPTEST_CASES` sets the count: 64 locally, 256 in CI.
10,000 passed before merging (again with E09-S13's input steps, and with E12-S03's locks).

**What it found.** Three defects that the unit tests had missed, fixed with the harness:

- After a reboot within the same 10-minute window, the tracker sent a second slot for that
  window. The server's unique key would have refused it.
- A clock set back while the device was off could repeat windows the same way.
- A `stop` decision after a restart split the entry left the server's new entry open; the
  contract now says an idle period spans such splits. Idle time before a sleep could also
  begin before the entry, giving an empty idle period and a pointless question.
- (E09-S12) At the `apps` level with titles on, a private window's title was kept, because
  the tab wasn't read below `domains`.

**CI.** The `Desktop simulation (Linux)` job in `ci.yml` runs the harness on every pull request,
whatever it changes: the scenarios, the checker tests and 256 generated scripts. The Desktop
workflow runs everything on three OSes when desktop code or the API contract changes.

**Boundaries.** The harness turns on test-only features (the platform fakes and store fault
injection) as normal dependencies. The crate-boundary rules therefore let nothing depend on it
except as a dev-dependency. The installers build only the shell's dependency tree, so those
features never reach the app.

## Rust tasks in Turborepo

The desktop `package.json` wraps Cargo: `lint` runs ESLint, `cargo fmt --check` and Clippy with
warnings as errors, and `test` runs Vitest and `cargo test`. Both depend on the package's own
`build`, because `tauri::generate_context!` embeds `dist/` at compile time. `pnpm check` therefore
needs Rust and, on Linux, the WebKitGTK development libraries. Anyone who only works on the web
app can skip the desktop with `pnpm turbo run lint typecheck test build --filter=!@worklume/desktop`.

Turborepo runs tasks in strict environment mode, so the desktop tasks pass through `CARGO_HOME`,
`RUSTUP_HOME`, `RUSTUP_TOOLCHAIN` and `CARGO_TARGET_DIR` explicitly.

Tests may unwrap, expect and panic (`clippy.toml`); production code may not (workspace lints).

Rust tests live in `src-tauri/tests/` as integration tests against the crate's public API, and
the library and binary have no unit-test harness (`test = false`). On Windows, Tauri's tray and
menus need version 6 of the Common Controls, which an executable requests in its manifest.
`tauri_build` embeds that manifest only in the app binary, so test executables failed to start
with `STATUS_ENTRYPOINT_NOT_FOUND`. `build.rs` adds the manifest dependency to integration tests
through `cargo::rustc-link-arg-tests`, which reaches integration tests but not unit-test
harnesses; hence the layout.

## Tray-resident window behaviour

The window starts hidden. Closing it cancels the close and hides the window, so tracking keeps
running; only **Quit Worklume** in the tray exits. The close is handled in the app's run-event
callback (`window::on_run_event`) rather than a per-window listener. Both work in the real
runtime, but only the run-event path is delivered by Tauri's mock runtime, which lets a unit test
prove the behaviour. A second test runs the same scenario without the handler and expects the
app to exit, so the first test cannot pass by accident.

The mock runtime cannot report visibility, so "Open Worklume" restoring the window is verified
by the per-OS tray stories: on Linux by `tests/tray_linux.rs` (E12-S05, see "Linux tray and
autostart"); on macOS and Windows by E10-S07 and E11-S05.

A second launch shows the running agent's window, **unless it is a launch at login**
(`--autostart`, `worklume_platform::LAUNCHED_AT_LOGIN`, which every OS's autostart passes):
that finds nothing to show, so logging in never pops the window up. The watchdog passes the
argument through to the agent.

## Webview security

- **IPC allowlist:** `capabilities/main.json` grants the window only its typed commands and
  listening to events (see "Typed IPC and the allowlist").
- **CSP:** everything `'self'` only, including styles; IPC through `ipc:` and
  `http://ipc.localhost`; no objects, frames or form targets. The agent UI injects no styles:
  its dialogs are native `<dialog>` elements rather than Radix, so inline styles stay blocked.
- `freezePrototype` is on and `withGlobalTauri` is off, so page scripts cannot patch built-ins or
  reach the Tauri API through a global.

## Linux packaging

Tauri adds `libayatana-appindicator3-1`, `libwebkit2gtk-4.1-0` and `libgtk-3-0` to the `.deb`
dependencies automatically when the tray is enabled, which satisfies risk R5's "AppIndicator
dependency declared". CI builds on Ubuntu 22.04 so the packages also run on newer releases.

What the `.deb` installs (checked on a build, E12-S05):

- `/usr/bin/worklume-desktop`;
- `/usr/share/applications/Worklume.desktop`, with `StartupWMClass=worklume-desktop`. The
  window's class comes from the program name, so the program name must not change, or docks
  lose the window's icon;
- icons under `/usr/share/icons/hicolor`, named `worklume-desktop`;
- `/etc/xdg/autostart/com.worklume.agent.desktop` (E12-S05; the `.rpm` too), from
  `src-tauri/linux/com.worklume.agent.autostart.desktop`: `Exec=/usr/bin/worklume-desktop
--autostart`.

The package metadata (maintainer, long description) and the apt repository are E12-S06's.

## Linux tray and autostart

E12-S05. **The tray** is Tauri's: a `StatusNotifierItem` through `libayatana-appindicator`,
which stock Ubuntu's AppIndicator extension draws. Other GNOME desktops need such an
extension; KDE, Xfce and others host `StatusNotifierItem` themselves.

- The tray library writes the icon to a file. Without `XDG_RUNTIME_DIR` it falls back to a
  shared `/tmp/tray-icon`, which another user may own or hold a planted link in. The agent
  gives it `$XDG_RUNTIME_DIR/com.worklume.agent/tray`, or the cache folder without one, made
  `0700`.
- The item's `Title` is the program name, `worklume-desktop`: changing it through GLib's
  program name would also change the window's class (see "Linux packaging").
- The tray menu is English only, like the window, until a second language arrives; both will
  be localised together.

**Autostart** (`worklume_platform::Autostart`, Linux adapter `linux::XdgAutostart`):

- The packages install the system entry, so the agent starts at login for every member of the
  machine.
- `set_enabled(false)` writes the member's own entry with `Hidden=true`. `set_enabled(true)`
  removes that override, so the package's entry, which updates keep current, applies again.
- Without an enabled system entry (another way of installing, or an administrator who turned
  it off), `set_enabled(true)` writes a member's entry running this executable, or the
  `AppImage` it came from. The path is quoted and escaped as desktop entries require.
- `enabled()` honours `Hidden=true` and GNOME's `X-GNOME-Autostart-enabled=false` (what GNOME's
  Startup Applications writes), in the `[Desktop Entry]` group only.
- Entries are written aside and renamed, so a login never meets half an entry.
- Nothing calls it yet: the organization's `auto-start` policy (E09-S06) and the member's
  setting (E09-S09 part 2) will. macOS and Windows adapters come with E10-S07 and E11-S05.

**Tested.**

- Unit tests cover the entry logic with temporary folders.
- `src-tauri/tests/tray_linux.rs` runs the real app against a stand-in `StatusNotifierWatcher`
  on a private D-Bus session. It checks that the app:
  - registers an active item, with its icon in the private folder and the two menu items;
  - shows no window when launched at login, even at a second such launch;
  - opens the window from "Open Worklume" and exits cleanly from "Quit Worklume", clicked
    through `com.canonical.dbusmenu` as a panel would.
- The `Desktop` workflow runs it on Ubuntu under `xvfb-run -a dbus-run-session`.
- On a built `.deb`, installed in the workspace, launching the autostart entry with
  `gio launch` (as a session does at login) started the supervisor and the agent with
  `--autostart`, registered the tray item and showed no window.
- Not covered: GNOME Shell itself and its AppIndicator extension, which is the release test
  matrix.

## CI cost

The desktop workflow uses macOS and Windows runners, whose minutes cost several times more than
Linux minutes on a private repository. The workflow therefore runs only when desktop paths change
(or by hand) and cancels superseded runs.

## Tauri versions

The Tauri CLI comes from npm and falls under the seven-day release-age policy, so it is on
2.11.5. The `tauri` crate and its internal crates are pinned to the same 2.11 line
(`tauri ~2.11.5`, `tauri-build ~2.6.3`, locked with `cargo update --precise`). A 2.11 CLI with the
2.12 crate built, but warned that the CLI sets a build variable the newer crate deprecates. Upgrade
the CLI and the crates together.

## Deferred

- Localised tray labels and UI strings: E09-S09 (agent UI).
- Running the timekeeper in the app: a loop that ticks every second, one store per
  organization and device registration opened off the main thread, and logging of best-effort
  window failures. This arrives after sign-in (E09-S03) with the agent UI (E09-S09), which also
  shows the idle dialog.
- Idle source adapters, and session lock and unlock through `PowerEvents`: the per-OS input
  stories (E10-S03, E11-S03, E12-S03, E13-S04).
- Foreground-app and browser-tab readers: the per-OS window and URL stories. Each browser
  reader must report private windows for every browser it can read.
- The simulation harness drives the real sync engine and the offline property tests of E08-S10
  once they exist (E09-S05).
- A monochrome template tray icon for the macOS menu bar: E10-S07.
- Code signing, notarisation and the updater: E09-S10 and E10–E13. The updater restarts the
  agent through exit code 75, so the watchdog stays in charge.
- Log bundle upload and native memory dumps: see "Watchdog and diagnostics".
