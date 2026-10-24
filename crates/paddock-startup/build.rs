//! Windows: embeds the Common Controls v6 manifest into every binary this crate links (its test binaries; a library
//! links none of its own). `tauri-plugin-dialog` (feature `dialog`) imports `TaskDialogIndirect`, which only Common
//! Controls v6 exports, and Tauri embeds that manifest into an app binary only: without it the tests do not load
//! (STATUS_ENTRYPOINT_NOT_FOUND). Dependents are not affected (link arguments do not propagate); the app's own
//! `tauri-build` embeds the manifest into the app. Checked on every OS by `tests/common_controls_manifest.rs`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
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
