//! Every workspace crate whose binaries can link `tauri-plugin-dialog` embeds the Common Controls v6 manifest into them
//! (its `build.rs`). The dialog plugin's Windows backend imports `TaskDialogIndirect`, which only Common Controls v6
//! exports; Tauri embeds that manifest into an app binary only, so a test binary without it does not even load on
//! Windows (exit 0xc0000139, STATUS_ENTRYPOINT_NOT_FOUND). This runs on every OS so a crate that gains the dependency
//! without the manifest fails here, not first on the Windows runner.

use std::path::{Path, PathBuf};

/// The marker every such `build.rs` carries (the manifest dependency it passes to the MSVC linker).
const MANIFEST_DEPENDENCY: &str = "name='Microsoft.Windows.Common-Controls' version='6.0.0.0'";

/// Ways a crate's manifest can reach `tauri-plugin-dialog`: directly, or through `paddock-startup`'s `dialog` feature.
const DIALOG_PATHS: [&str; 2] = ["tauri-plugin-dialog", "paddock-startup/dialog"];

fn workspace_crates() -> Vec<PathBuf> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate lives in the workspace");
    let mut crates: Vec<PathBuf> = std::fs::read_dir(workspace)
        .expect("the workspace folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    crates.sort();
    crates
}

/// The crates whose `Cargo.toml` can reach `tauri-plugin-dialog`, and whether their `build.rs` embeds the manifest.
fn dialog_crates() -> Vec<(String, bool)> {
    workspace_crates()
        .into_iter()
        .filter(|dir| {
            let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
            DIALOG_PATHS.iter().any(|p| manifest.contains(p))
        })
        .map(|dir| {
            let build = std::fs::read_to_string(dir.join("build.rs")).unwrap_or_default();
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (
                name,
                build.contains(MANIFEST_DEPENDENCY) && build.contains("/MANIFEST:EMBED"),
            )
        })
        .collect()
}

#[test]
fn every_crate_that_can_link_the_dialog_plugin_embeds_the_common_controls_manifest() {
    let crates = dialog_crates();
    let names: Vec<&str> = crates.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"paddock-startup") && names.contains(&"paddock-shell"),
        "found {names:?}"
    );
    let missing: Vec<&str> = crates
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(n, _)| n.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "build.rs does not embed the Common Controls v6 manifest: {missing:?}"
    );
}
