//! Which Cargo workspaces hold Linux-only code: a `cfg` that names `target_os = "linux"`. The Mac Clippy and the xwin Clippy never
//! compile such code, so the pre-push Linux Clippy (in the CI image) runs for a touched workspace that has any (ER-9).
//! The scan is a text match, on purpose: a mention in a comment only makes the check run when it did not have to.

use std::collections::BTreeSet;
use std::path::Path;

/// Folders that hold no source of the workspace.
const SKIP: &[&str] = &["target", "node_modules", ".git", "dist"];

/// Whether `source` names Linux as a target OS in a cfg (spacing and quotes as rustfmt or a person writes them).
pub fn has_linux_cfg(source: &str) -> bool {
    if !source.contains("linux") {
        return false;
    }
    let squeezed: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    squeezed.contains("target_os=\"linux\"")
}

/// Whether any `.rs` file under `dir` (symlinks are not followed) has a Linux-only cfg. Unreadable entries are not Linux code.
pub fn dir_has_linux_cfg(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for e in entries.flatten() {
        let Ok(kind) = e.file_type() else { continue };
        let path = e.path();
        if kind.is_dir() {
            let name = e.file_name().to_string_lossy().to_string();
            if !SKIP.contains(&name.as_str()) && dir_has_linux_cfg(&path) {
                return true;
            }
        } else if kind.is_file()
            && path.extension().is_some_and(|x| x == "rs")
            && std::fs::read_to_string(&path).is_ok_and(|t| has_linux_cfg(&t))
        {
            return true;
        }
    }
    false
}

/// The workspaces among `candidates` (repository-relative folders under `root`) that hold Linux-only code.
pub fn linux_workspaces(root: &Path, candidates: &BTreeSet<String>) -> BTreeSet<String> {
    candidates.iter().filter(|w| dir_has_linux_cfg(&root.join(w))).cloned().collect()
}
