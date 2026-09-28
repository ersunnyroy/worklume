# @worklume/desktop

The Worklume desktop agent: a Tauri 2 app that lives in the menu bar (macOS) or system tray
(Windows, Linux). Right now it is a scaffold: a tray icon with **Open Worklume** and **Quit
Worklume**, and a window that starts hidden and hides again when closed. Tracking, storage and
sync arrive in epic E09.

## Layout

| Path                           | Holds                                                              |
| ------------------------------ | ------------------------------------------------------------------ |
| `frontend/`                    | React UI rendered in the webview (Vite root)                       |
| `src-tauri/`                   | The Tauri shell crate `worklume-desktop`, a Cargo workspace member |
| `src-tauri/src/tray.rs`        | Tray icon and menu                                                 |
| `src-tauri/src/window.rs`      | Window behaviour: close hides, "Open" restores                     |
| `src-tauri/capabilities/`      | IPC allowlist for the webview (empty for now)                      |
| `src-tauri/tests/`             | Rust integration tests, including Tauri mock-runtime tests         |
| `src-tauri/tauri.conf.json`    | App identity, window, CSP and bundle settings                      |
| `src-tauri/icons/`, `icon.svg` | App icons, generated from `icon.svg`                               |

E09-S01 adds the `core`, `sync`, `storage` and `platform` crates next to `src-tauri/`, as the
architecture's desktop layout describes.

## Prerequisites

Node and pnpm as for the rest of the repo, plus Rust (installed automatically from
`rust-toolchain.toml` by `rustup`) and the platform's webview libraries:

- **macOS:** Xcode Command Line Tools (`xcode-select --install`).
- **Windows:** Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11).
- **Ubuntu:** `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev build-essential`.

## Commands

| Command                                     | Does                                                    |
| ------------------------------------------- | ------------------------------------------------------- |
| `pnpm --filter @worklume/desktop dev`       | Run the app with the UI hot-reloading                   |
| `pnpm --filter @worklume/desktop build`     | Build the UI into `dist/` (the shell embeds it)         |
| `pnpm --filter @worklume/desktop build:app` | Build unsigned installers into `target/release/bundle/` |
| `pnpm --filter @worklume/desktop lint`      | ESLint, `cargo fmt --check` and Clippy                  |
| `pnpm --filter @worklume/desktop test`      | Vitest for the UI and `cargo test` for the shell        |

Lint and test build the UI first, because the Rust crate embeds `dist/` at compile time.

## CI

`.github/workflows/desktop.yml` lints, tests and builds unsigned installers on macOS (Apple
silicon), Windows and Ubuntu 22.04 when desktop code changes, and on demand from the Actions
tab. Artifacts (`.dmg`, `.msi`, `.exe`, `.deb`, `.rpm`, `.AppImage`) are kept for 14 days.
Signing, notarisation and the updater arrive with E09-S10 and the per-OS epics.

## Regenerating icons

```sh
rsvg-convert -w 1024 -h 1024 icon.svg -o /tmp/icon.png
pnpm tauri icon /tmp/icon.png --output src-tauri/icons
```

Then delete the generated `android/`, `ios/`, `Square*` and `StoreLogo` files, which desktop
bundles don't use.
