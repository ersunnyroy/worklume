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
- Listing commands in the app manifest (`build.rs`) puts app commands under Tauri's ACL, and
  `capabilities/main.json` grants the main window only `allow-agent-status`. The allowlist test
  checks that refused calls fail with the ACL's "not allowed" error, not merely any error: with
  `core:default` granted, several core commands get past the ACL and fail later on missing
  arguments, which a looser test would miss.
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

E09-S02 built `worklume-storage` and the `SecretStore` trait in `worklume-platform`. The agent
does not open the store yet; the tracker (E09-S04) owns its lifecycle.

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
drives it on the device's clock. The loop that ticks every second, and the
one-store-per-organization setup, arrive with sign-in (E09-S03) and the agent UI (E09-S09).
Until then both run in tests against controllable clocks.

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
- After a reboot, slots from the old boot close at once and are dated in that boot.

**Slots.**

- 10-minute UTC windows, at most one per window, matching the server's unique key.
- A slot closes only once its window has passed, so stopping and restarting within a window adds
  to the same slot.
- Each slot holds tracked, active, keyboard, mouse and idle seconds. Idle counts from detection,
  and activity is clamped to tracked time.
- It is attributed to the entry with the most time in it (the later entry on a tie).
- It is flagged `partial_slot` when not fully tracked; slots under one whole second aren't sent.
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

`PowerEvents` is still the place for session lock and unlock, which clocks can't see. The
tracker already supports `pause(SessionLocked)`, and the per-OS stories wire the events.

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
  - Inactivity of a second or more before the sleep becomes idle time awaiting a decision, and
    answering it also resumes the timer, unless the answer is `stop`.
  - Otherwise the only question is whether to resume.
  - If idle time was already awaiting a decision, that question stands; it is not asked again.

**Keep-idle policy** (`idle.keep`, delivered with the config in E09-S06):

| Policy   | Idle while awake                                | Long absence with a sleep                      |
| -------- | ----------------------------------------------- | ---------------------------------------------- |
| `prompt` | Asks the member                                 | Asks about the idle time, or whether to resume |
| `always` | Kept when input returns, without asking         | Idle kept; asks whether to resume              |
| `never`  | Discarded, and the timer stops where idle began | The timer stops; nothing is asked              |

- A policy decision sends `idle.resolved` with `decided_by: policy`.
- A timeout of `None` turns idle detection off. Sleep is still never tracked, and every sleep
  then resumes on its own.
- The policy comes from the server and is never hardcoded. `set_policy` applies a new policy
  from the next call.

**Prompts.** `Timekeeper::prompt()` is the question the idle dialog (E09-S09) shows.

- `Prompt::Idle` offers keep, discard, reassign or stop.
- `Prompt::Resume` offers resume or stop.
- Each call's `Outcome` reports a new question, for a notification. A question that outlives a
  sleep counts as the same question.

**Tested.** The acceptance criterion is that a two-hour sleep tracks no time and asks once at
most.

- `core/tests/timekeeper.rs` checks it under all three policies, for a member who was working,
  briefly away or long away before the sleep.
- Other tests cover short sleeps, answers, clicks just after waking, restarts, policy changes,
  and failed clock reads and saves.
- `platform/tests/os_clock.rs` checks the real clocks on each OS in CI.
- A real sleep can't happen in CI, so sleep detection on real hardware belongs in the release
  test matrix.

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
manually and by the per-OS tray stories (E10-S07, E11-S05, E12-S05).

## Webview security

- **IPC allowlist:** `capabilities/main.json` grants the window no permissions. E09-S01 adds typed
  commands one at a time, which is its acceptance criterion.
- **CSP:** everything `'self'` only, including styles; IPC through `ipc:` and
  `http://ipc.localhost`; no objects, frames or form targets. Unlike the web app, the agent UI uses
  no Radix scroll lock yet, so inline styles stay blocked. Revisit if E09-S09 adds components that
  inject styles.
- `freezePrototype` is on and `withGlobalTauri` is off, so page scripts cannot patch built-ins or
  reach the Tauri API through a global.

## Linux packaging

Tauri adds `libayatana-appindicator3-1`, `libwebkit2gtk-4.1-0` and `libgtk-3-0` to the `.deb`
dependencies automatically when the tray is enabled, which satisfies risk R5's "AppIndicator
dependency declared". CI builds on Ubuntu 22.04 so the packages also run on newer releases.

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
- A monochrome template tray icon for the macOS menu bar: E10-S07.
- Code signing, notarisation and the updater: E09-S10 and E10–E13. The updater restarts the
  agent through exit code 75, so the watchdog stays in charge.
- Log bundle upload and native memory dumps: see "Watchdog and diagnostics".
