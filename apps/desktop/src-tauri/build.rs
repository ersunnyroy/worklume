fn main() {
    tauri_build::build();
    embed_common_controls_manifest_in_tests();
}

/// Tauri's tray and menus need version 6 of the Windows Common Controls, which an executable
/// requests through its manifest. `tauri_build` embeds that manifest in the app binary only, so
/// test executables fail to start with `STATUS_ENTRYPOINT_NOT_FOUND`. This adds the same
/// dependency to the integration tests when linking with MSVC.
fn embed_common_controls_manifest_in_tests() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        println!("cargo::rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
