//! What the Linux Clippy row needs to know before it starts a container, read on the host: the CI image version (one reader for
//! the row and `./dev check --warm-linux`) and the Tauri crates of a workspace that the container cannot lint.
//!
//! The container mounts the repository read-only (ER-14), and a Tauri app crate's build script (`paddock-build`, `tauri-build`)
//! writes into the source tree (the sidecar placeholders in `binaries/`, tauri-build's `gen/`), so it fails there before Clippy
//! runs. Those crates are left out of the run and named in the row; CI's Linux legs lint them. The list comes from the repository's
//! one cargo-metadata accessor ([`paddock_repo_graph::CargoMetadata`]), not from a path or a text match (ER-42).

use paddock_repo_graph::{CargoCli, CargoMetadata};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// The build dependencies that make a crate's build script write into the source tree.
const WRITERS: &[&str] = &["tauri-build", "paddock-build"];

/// The CI image version (`ci-image/VERSION`), or why it cannot be read. The one reader: the row's target folder and the helper's
/// `--ci-image` both key on it, so a bump starts a clean folder.
pub fn ci_version(root: &Path) -> Result<String, String> {
    let v = std::fs::read_to_string(root.join("ci-image/VERSION")).map_err(|e| format!("ci-image/VERSION cannot be read: {e}"))?;
    let v = v.trim();
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return Err(format!("ci-image/VERSION holds {v:?}, not a version"));
    }
    Ok(v.to_string())
}

/// The packages of one `cargo metadata --no-deps` JSON text that have `tauri-build` or `paddock-build` among their build
/// dependencies, sorted.
pub fn tauri_crates(metadata: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(metadata).map_err(|e| format!("cargo metadata is not JSON: {e}"))?;
    let packages = v["packages"].as_array().ok_or("cargo metadata has no \"packages\" list")?;
    let mut out: Vec<String> = packages
        .iter()
        .filter(|p| {
            p["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|d| d["kind"] == "build" && d["name"].as_str().is_some_and(|n| WRITERS.contains(&n)))
        })
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    out.sort();
    Ok(out)
}

/// The crates of workspace `ws` (repository-relative) the container leaves out, or why cargo could not say (the row is then
/// broken, not skipped: ER-45).
pub fn left_out(root: &Path, ws: &str) -> Result<Vec<String>, String> {
    let json = CargoCli::new(root, Duration::from_secs(120))
        .metadata(&format!("{ws}/Cargo.toml"))
        .map_err(|e| format!("cargo metadata for {ws} failed: {e}"))?;
    tauri_crates(&json)
}
