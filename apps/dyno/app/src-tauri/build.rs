//! Tauri's build step, plus the Windows application manifest for every binary of this crate.

fn main() {
    // The shell plugin's permission files (its `links` metadata), so the tests resolve the capabilities through Tauri's ACL.
    if let Ok(list) = std::env::var("DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH") {
        println!("cargo:rustc-env=PADDOCK_SHELL_PERMISSION_FILES={list}");
    }
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
    // tauri-build requires the sidecars (bundle.externalBin: the agent and the dyno command) to exist even for `cargo check` and tests. Bundles
    // always build the real one first (scripts/sidecar.mjs in beforeBuildCommand); elsewhere an empty placeholder stands
    // in, which the app treats as "missing" (it then looks for a dyno-agent next to itself, or DYNO_AGENT_BIN).
    if let Ok(target) = std::env::var("TARGET") {
        let ext = if target.contains("windows") { ".exe" } else { "" };
        let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("binaries");
        let mut placeholders = false;
        for name in ["dyno-agent", "dyno", "dyno-bench", "dyno-bench-gpu"] {
            let sidecar = dir.join(format!("{name}-{target}{ext}"));
            if !sidecar.exists() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(&sidecar, b"");
            }
            placeholders |= std::fs::metadata(&sidecar).map(|m| m.len() == 0).unwrap_or(true);
            println!("cargo:rerun-if-changed={}", sidecar.display());
        }
        // tauri-build copies every externalBin to target/<profile>/<name>, over the `dyno` and `dyno-agent` that cargo links
        // there; with placeholders that left 0-byte programs in the build folder (a test copied an empty `dyno`, and
        // codesign named it `dyno-da39a3ee…`, the SHA-1 of nothing). So while any sidecar is a placeholder, tauri-build sees
        // no externalBin: TAURI_CONFIG is merged as a JSON merge patch, where null removes the key. Bundles and `tauri dev`
        // build the real sidecars first (scripts/sidecar.mjs) and keep the copy.
        if placeholders {
            let mut patch: serde_json::Value = std::env::var("TAURI_CONFIG")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            if !patch.is_object() {
                patch = serde_json::json!({});
            }
            patch["bundle"]["externalBin"] = serde_json::Value::Null;
            std::env::set_var("TAURI_CONFIG", patch.to_string());
            // A build folder from before this fix still holds the 0-byte copies; a real program is never empty.
            // OUT_DIR is target/<profile>/build/<package>-<hash>/out.
            let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
            if let Some(profile_dir) = out.ancestors().nth(3) {
                for name in ["dyno-agent", "dyno", "dyno-bench", "dyno-bench-gpu"] {
                    let copy = profile_dir.join(format!("{name}{ext}"));
                    if std::fs::metadata(&copy).is_ok_and(|m| m.is_file() && m.len() == 0) {
                        let _ = std::fs::remove_file(&copy);
                    }
                }
            }
        }
    }
    if let Err(e) = tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows)) {
        panic!("tauri-build failed: {e:#}");
    }
}
