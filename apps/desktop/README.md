# @worklume/desktop

The Worklume desktop agent: a Tauri 2 app that lives in the menu bar (macOS) or system tray
(Windows, Linux). Right now it is a shell: a tray icon with **Open Worklume** and **Quit
Worklume**, a window that starts hidden and hides again when closed, a single running instance,
and one typed command (`agent_status`) behind an IPC allowlist. Tracking, storage and sync
arrive in the rest of epic E09. The encrypted local store (`storage/`) exists and is tested,
but the app does not open it until the tracker arrives (E09-S04).

## Layout

| Path                               | Holds                                                                  |
| ---------------------------------- | ---------------------------------------------------------------------- |
| `frontend/`                        | React UI rendered in the webview (Vite root)                           |
| `frontend/src/ipc/bindings.gen.ts` | Typed command bindings, generated from `src-tauri/src/ipc.rs`          |
| `src-tauri/`                       | The Tauri shell crate `worklume-desktop`: tray, window, IPC, lifecycle |
| `src-tauri/capabilities/`          | IPC allowlist for the main window                                      |
| `src-tauri/tests/`                 | Integration tests, including Tauri mock-runtime and allowlist tests    |
| `core/`                            | `worklume-core`: tracker, slots, screenshots, idle, policy (E09-S04+)  |
| `storage/`                         | `worklume-storage`: SQLCipher journal and outbox, encrypted spool      |
| `storage/migrations/`              | Local schema migrations, applied in order, append-only                 |
| `sync/`                            | `worklume-sync`: outbox drain and uploads (E09-S05)                    |
| `platform/`                        | `worklume-platform`: OS integration behind traits; OS credential store |
| `scripts/`                         | Crate dependency rules, checked on every lint                          |
| `src-tauri/icons/`, `icon.svg`     | App icons, generated from `icon.svg`                                   |

## Adding a command the UI can call

1. Write the command in `src-tauri/src/ipc.rs` with `#[tauri::command]` and `#[specta::specta]`,
   and add it to `builder()`.
2. List its name in the app manifest in `src-tauri/build.rs`.
3. Grant `allow-<command-name>` in `src-tauri/capabilities/main.json`.
4. Regenerate the bindings: `UPDATE_BINDINGS=1 cargo test -p worklume-desktop --test bindings`.
5. Call it from the UI through `commands` in `frontend/src/ipc/bindings.gen.ts`.

A command missing from step 2 or 3 is refused by Tauri's ACL; stale bindings fail the tests.

## Prerequisites

Node and pnpm as for the rest of the repo, plus Rust (installed automatically from
`rust-toolchain.toml` by `rustup`) and the platform's webview libraries:

- **macOS:** Xcode Command Line Tools (`xcode-select --install`).
- **Windows:** Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11).
- **Ubuntu:** `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libdbus-1-dev pkg-config build-essential`.

The store's SQLCipher build compiles OpenSSL from source, which needs Perl (preinstalled on
macOS and Ubuntu; on Windows, Strawberry Perl). The first build takes a few minutes longer.

## Commands

| Command                                     | Does                                                    |
| ------------------------------------------- | ------------------------------------------------------- |
| `pnpm --filter @worklume/desktop dev`       | Run the app with the UI hot-reloading                   |
| `pnpm --filter @worklume/desktop build`     | Build the UI into `dist/` (the shell embeds it)         |
| `pnpm --filter @worklume/desktop build:app` | Build unsigned installers into `target/release/bundle/` |
| `pnpm --filter @worklume/desktop lint`      | ESLint, crate boundaries, `cargo fmt --check`, Clippy   |
| `pnpm --filter @worklume/desktop test`      | Vitest, script tests and `cargo test` for all crates    |

Lint and test build the UI first, because the Rust crate embeds `dist/` at compile time.

## CI

`.github/workflows/desktop.yml` lints, tests and builds unsigned installers on macOS (Apple
silicon), Windows and Ubuntu 22.04 when desktop code changes, and on demand from the Actions
tab. It also runs the credential store tests against each OS's real keychain (GNOME Keyring on
Ubuntu); run them locally with `cargo test -p worklume-platform --test os_secret_store -- --ignored`.
Artifacts (`.dmg`, `.msi`, `.exe`, `.deb`, `.rpm`, `.AppImage`) are kept for 14 days.
Signing, notarisation and the updater arrive with E09-S10 and the per-OS epics.

## Regenerating icons

```sh
rsvg-convert -w 1024 -h 1024 icon.svg -o /tmp/icon.png
pnpm tauri icon /tmp/icon.png --output src-tauri/icons
```

Then delete the generated `android/`, `ios/`, `Square*` and `StoreLogo` files, which desktop
bundles don't use.
