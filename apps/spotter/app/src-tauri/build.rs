//! Tauri's build step, plus the Windows application manifest for every binary of this crate.

fn main() {
    // Tauri embeds its app manifest into the app binary only; test binaries then fail to load on Windows
    // (STATUS_ENTRYPOINT_NOT_FOUND, Common Controls v6 missing). Embed the same manifest into every binary
    // this crate links (`rustc-link-arg` covers tests too) and tell tauri-build not to add its own.
    let msvc =
        std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let mut windows = tauri_build::WindowsAttributes::new();
    if msvc {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    }
    println!("cargo:rerun-if-env-changed=TAURI_UPDATER_PUBKEY");
    if let Err(e) = tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)) {
        panic!("tauri-build failed: {e:#}");
    }
}
