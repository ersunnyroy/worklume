# Desktop agent decisions

Decisions made while scaffolding the desktop agent (E00-S09). The stack (Tauri 2, Rust core,
per-OS adapters) is fixed by ADR-006.

## Layout

The shell crate lives in `apps/desktop/src-tauri` (Tauri's convention, which its CLI and docs
assume) and the webview UI in `apps/desktop/frontend`, matching the architecture's `frontend/`.
The domain crates from the architecture (`core`, `sync`, `storage`, `platform`) join as siblings
of `src-tauri` in E09-S01. All crates are members of the root Cargo workspace and share its
lints and release profile.

## Rust tasks in Turborepo

The desktop `package.json` wraps Cargo: `lint` runs ESLint, `cargo fmt --check` and Clippy with
warnings as errors, and `test` runs Vitest and `cargo test`. Both depend on the package's own
`build`, because `tauri::generate_context!` embeds `dist/` at compile time. `pnpm check` therefore
needs Rust and, on Linux, the WebKitGTK development libraries. Anyone who only works on the web
app can skip the desktop with `pnpm turbo run lint typecheck test build --filter=!@worklume/desktop`.

Turborepo runs tasks in strict environment mode, so the desktop tasks pass through `CARGO_HOME`,
`RUSTUP_HOME`, `RUSTUP_TOOLCHAIN` and `CARGO_TARGET_DIR` explicitly.

Tests may unwrap, expect and panic (`clippy.toml`); production code may not (workspace lints).

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

## Deferred

- Localised tray labels and UI strings: E09-S09 (agent UI).
- Logging of best-effort failures (hiding or focusing a window): E09-S11.
- A monochrome template tray icon for the macOS menu bar: E10-S07.
- Single-instance enforcement: E09-S01.
- Code signing, notarisation and the updater: E09-S10 and E10–E13.
