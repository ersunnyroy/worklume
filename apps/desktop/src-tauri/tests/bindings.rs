//! The committed TypeScript bindings match the Rust commands.
//!
//! Run `UPDATE_BINDINGS=1 cargo test -p worklume-desktop --test bindings` after changing a
//! command or its types, and commit the regenerated file.

// Shared helpers are not #[test] functions, so clippy.toml's test exemption does not reach them.
#![allow(clippy::expect_used, reason = "test setup failures should fail loudly")]

use std::path::PathBuf;

use specta_typescript::Typescript;
use tauri::test::MockRuntime;

fn committed_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../frontend/src/ipc/bindings.gen.ts")
}

#[test]
fn committed_bindings_match_the_commands() {
    let generated =
        std::env::temp_dir().join(format!("worklume-bindings-{}.ts", std::process::id()));
    worklume_desktop::ipc::builder::<MockRuntime>()
        .export(
            Typescript::default().header(
                "// Generated from src-tauri/src/ipc.rs by tests/bindings.rs. Do not edit.\n",
            ),
            &generated,
        )
        .expect("bindings export");
    let fresh = std::fs::read_to_string(&generated).expect("generated bindings are readable");
    std::fs::remove_file(&generated).ok();

    if std::env::var_os("UPDATE_BINDINGS").is_some() {
        std::fs::write(committed_path(), &fresh).expect("committed bindings are writable");
        return;
    }
    let committed = std::fs::read_to_string(committed_path()).unwrap_or_default();
    assert!(
        committed == fresh,
        "frontend/src/ipc/bindings.gen.ts is stale: run \
         UPDATE_BINDINGS=1 cargo test -p worklume-desktop --test bindings and commit the result"
    );
}
