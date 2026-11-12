//! The one deleting path (user decision 2026-11-12): code under `tools/` that deletes folders goes through
//! `paddock_dev::safe_remove`. This test finds every other call of `remove_dir_all`, `fs::remove_dir(` and
//! `git worktree remove` in the non-test sources of `tools/` and fails unless the file is listed below with its reason. A new
//! deleting call elsewhere must use `safe_remove::remove`; the listed callers adopt it in follow-ups (the first two are the
//! worktree drop of `fix/worktree-drop-after-release` and the CI slot sweep).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

/// (file, why it may delete for now).
const ALLOWED: &[(&str, &str)] = &[
    ("tools/paddock-dev/src/safe_remove.rs", "the shared check itself"),
    (
        "tools/paddock-dev/src/ci_isolation/tmpdir.rs",
        "a job's own temp and socket folders, not a dev-targets root",
    ),
    (
        "tools/paddock-dev/src/ci_isolation/slotflow.rs",
        "FOLLOW-UP: the CI slot sweep of kept folders (idle-run) adopts safe_remove",
    ),
    (
        "tools/paddock-dev/src/worktree/ops.rs",
        "FOLLOW-UP: `worktree drop` (git worktree remove) adopts safe_remove (fix/worktree-drop-after-release)",
    ),
    (
        "tools/paddock-dev/src/worktree/inventory.rs",
        "FOLLOW-UP: `worktree clean` (git worktree remove) adopts safe_remove",
    ),
    (
        "tools/paddock-dev/src/release_isolation/fixture.rs",
        "a test fixture's own temp folder",
    ),
    ("tools/heavy/src/lock.rs", "heavy.sh's lock folders, not work folders"),
];

const PATTERNS: &[&str] = &["remove_dir_all", "fs::remove_dir(", "\"worktree\", \"remove\""];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "target" && name != "tests" && name != "node_modules" && !name.starts_with('.') {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

#[test]
fn only_safe_remove_and_the_listed_callers_delete_folders_under_tools() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    rust_files(&repo.join("tools"), &mut files);
    assert!(files.len() > 30, "the scan found only {} files: wrong root?", files.len());
    let mut unlisted = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    for f in files {
        let rel = f.strip_prefix(&repo).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&f).unwrap();
        if !PATTERNS.iter().any(|p| text.contains(p)) {
            continue;
        }
        if ALLOWED.iter().any(|(a, _)| *a == rel) {
            used.insert(rel);
        } else {
            unlisted.push(rel);
        }
    }
    assert!(
        unlisted.is_empty(),
        "these files delete folders without paddock_dev::safe_remove: {unlisted:?}. Call `safe_remove::remove` (or list the file in ALLOWED with the follow-up that adopts it)."
    );
    let stale: Vec<_> = ALLOWED.iter().filter(|(a, _)| !used.contains(*a)).map(|(a, _)| *a).collect();
    assert!(
        stale.is_empty(),
        "listed but no longer deleting (remove them from ALLOWED): {stale:?}"
    );
}
