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

/// The packages of one workspace, split by whether the container can lint them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WsCrates {
    /// The Tauri crates (a `tauri-build` or `paddock-build` build dependency), sorted: left out of the run.
    pub left_out: Vec<String>,
    /// The packages that remain after them. Zero means `cargo clippy --workspace --exclude ..` has nothing to compile (cargo:
    /// "no packages to compile"), so the row skips the workspace.
    pub lintable: usize,
}

/// Splits the packages of one `cargo metadata --no-deps` JSON text (the workspace members) into the Tauri crates and the rest.
pub fn split_crates(metadata: &str) -> Result<WsCrates, String> {
    let v: Value = serde_json::from_str(metadata).map_err(|e| format!("cargo metadata is not JSON: {e}"))?;
    let packages = v["packages"].as_array().ok_or("cargo metadata has no \"packages\" list")?;
    let writes = |p: &Value| {
        p["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["kind"] == "build" && d["name"].as_str().is_some_and(|n| WRITERS.contains(&n)))
    };
    let mut left_out: Vec<String> = packages
        .iter()
        .filter(|p| writes(p))
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    left_out.sort();
    let lintable = packages.iter().filter(|p| !writes(p)).count();
    Ok(WsCrates { left_out, lintable })
}

/// The crates of workspace `ws` (repository-relative) the container leaves out and how many remain, or why cargo could not say
/// (the row is then broken, not skipped: ER-45).
pub fn left_out(root: &Path, ws: &str) -> Result<WsCrates, String> {
    let json = CargoCli::new(root, Duration::from_secs(120))
        .metadata(&format!("{ws}/Cargo.toml"))
        .map_err(|e| format!("cargo metadata for {ws} failed: {e}"))?;
    split_crates(&json)
}

/// What `linux-container.sh --ci-image --image-ready` said about the local CI image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageProbe {
    /// Exit 0: built (or the registry image, which is pulled when it runs).
    Ready,
    /// Exit 3: not built; `./dev check --warm-linux` builds it.
    Missing,
    /// Any other exit, or no exit: the probe itself is broken (ER-33), which is not "missing" and not a pass.
    Broken(String),
}

/// Reads the probe's exit code: 0 ready, 3 missing, anything else broken (a signal, exit 2 for a usage error, 127 for no bash).
pub fn image_probe(code: Option<i32>) -> ImageProbe {
    match code {
        Some(0) => ImageProbe::Ready,
        Some(3) => ImageProbe::Missing,
        Some(n) => ImageProbe::Broken(format!("exit {n}")),
        None => ImageProbe::Broken("it was ended by a signal or could not start".into()),
    }
}

/// The advice for a broken probe.
pub const PROBE_NEXT: &str = "run `bash tools/linux-container.sh --id paddock --ci-image --image-ready` yourself and read what it prints (exit 0 built, 3 not built); report this, the image check is broken";
