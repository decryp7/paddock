//! The plugin's build step: Tauri's `allow-<command>`/`deny-<command>` permissions for every shell command, and the
//! permission sets per window class (`permissions/window-classes.toml`, written by this crate's tests from the table).
//! The command list is generated from the composed table and checked by a test (`src/command_names.rs`).
//!
//! Windows: also embeds the Common Controls v6 manifest into this crate's test binaries. With `native-dialogs` they can
//! link `tauri-plugin-dialog`, which imports `TaskDialogIndirect` (Common Controls v6 only); Tauri embeds that manifest
//! into an app binary only, so without it a test binary does not load (STATUS_ENTRYPOINT_NOT_FOUND). Dependents are not
//! affected. Checked on every OS by `paddock-startup/tests/it/common_controls_manifest.rs`.

include!("src/command_names.rs");

fn main() {
    tauri_plugin::Builder::new(COMMAND_NAMES).build();
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
