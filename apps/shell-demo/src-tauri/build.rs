//! Tauri's build step: the context (tauri.conf.json, the capabilities with the shell plugin's permission sets) and the
//! bundle's resources. It also hands the tests the shell plugin's permission files (the plugin's `links` metadata), so
//! they resolve the capabilities through Tauri's ACL wherever the plugin comes from (a path or the registry).
fn main() {
    if let Ok(list) = std::env::var("DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH") {
        println!("cargo:rustc-env=PADDOCK_SHELL_PERMISSION_FILES={list}");
    }
    // tauri-build requires the agent (bundle.externalBin) to exist even for `cargo check` and tests. Bundles build the
    // real one first (scripts/shell-demo/sidecar.mjs in beforeBuildCommand); elsewhere an empty placeholder stands in,
    // which the app reports as a missing program if it is ever asked to register it.
    if let Ok(target) = std::env::var("TARGET") {
        let ext = if target.contains("windows") {
            ".exe"
        } else {
            ""
        };
        let dir = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
            .join("binaries");
        let agent = dir.join(format!("shell-demo-agent-{target}{ext}"));
        if !agent.exists() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&agent, b"");
        }
    }
    tauri_build::build();
}
