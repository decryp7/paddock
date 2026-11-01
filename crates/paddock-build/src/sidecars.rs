//! Keeping tauri-build's `externalBin` copy away from the programs cargo links.
//!
//! tauri-build copies every `bundle.externalBin` file to `target/<profile>/<name>`, over the binary of that name that
//! cargo placed there. While a sidecar is only an empty placeholder (tests, `cargo check`), that replaces a real program
//! with a 0-byte file. So while any sidecar is a placeholder tauri-build is told there is no `externalBin` ([`hide_external_bin`]),
//! and empty copies an older build left behind are removed ([`stale_copies`], [`remove_files`]). Everything here is a pure
//! function of its arguments or a plain file-system call, so it is tested without Tauri.

use std::fs;
use std::path::{Path, PathBuf};

use crate::BuildError;

/// Whether a sidecar file is a placeholder: missing or empty (a real program is never empty).
pub fn is_placeholder(file: &Path) -> bool {
    fs::metadata(file).map(|m| m.len() == 0).unwrap_or(true)
}

/// Whether any of the sidecar files is a placeholder.
pub fn any_placeholder(files: &[PathBuf]) -> bool {
    files.iter().any(|f| is_placeholder(f))
}

/// The value for `TAURI_CONFIG` that removes `bundle.externalBin` from tauri-build's view. `current` is the existing value:
/// Tauri merges it as a JSON merge patch, where `null` removes a key, so its other keys are kept. A value that is not a JSON
/// object is replaced (Tauri would refuse it anyway).
pub fn hide_external_bin(current: Option<&str>) -> String {
    let mut patch = current
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    if !patch["bundle"].is_object() {
        patch["bundle"] = serde_json::json!({});
    }
    patch["bundle"]["externalBin"] = serde_json::Value::Null;
    patch.to_string()
}

/// `target/<profile>`, from Cargo's `OUT_DIR` (`target/<profile>/build/<package>-<hash>/out`). `None` for a shorter path.
pub fn profile_dir(out_dir: &Path) -> Option<PathBuf> {
    out_dir.ancestors().nth(3).map(Path::to_path_buf)
}

/// The files tauri-build writes for the sidecars: `<profile_dir>/<name><ext>`.
pub fn copy_paths(profile_dir: &Path, names: &[String], ext: &str) -> Vec<PathBuf> {
    names
        .iter()
        .map(|n| profile_dir.join(format!("{n}{ext}")))
        .collect()
}

/// The copies that exist as empty files, left by a build from before the placeholders were hidden.
pub fn stale_copies(copies: &[PathBuf]) -> Vec<PathBuf> {
    copies
        .iter()
        .filter(|c| fs::metadata(c).is_ok_and(|m| m.is_file() && m.len() == 0))
        .cloned()
        .collect()
}

/// Removes the files; a file that is already gone is fine. Returns the files removed.
pub fn remove_files(files: &[PathBuf]) -> Result<Vec<PathBuf>, BuildError> {
    let mut removed = Vec::new();
    for file in files {
        match fs::remove_file(file) {
            Ok(()) => removed.push(file.clone()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(BuildError::Io {
                    path: file.clone(),
                    source,
                });
            }
        }
    }
    Ok(removed)
}
