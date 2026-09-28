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
- Only `platform` may use OS API crates (objc2, windows, x11rb, zbus, ashpd and similar).
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
- Logging of best-effort failures (hiding or focusing a window): E09-S11.
- A monochrome template tray icon for the macOS menu bar: E10-S07.
- Code signing, notarisation and the updater: E09-S10 and E10–E13.
