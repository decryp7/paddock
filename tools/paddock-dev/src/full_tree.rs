//! The guard of the tests that read the whole repository: they check first that the folders they read are there.
//!
//! It looks at the paths, never at the `core.sparseCheckout` key: the host kept clones of CI (lean and bundle profiles,
//! `kept-clone.sh`) set that key with a non-cone pattern and still have `tools/`, `conformance/` and `.gitea/`, so the key says
//! nothing about what a test needs.

use std::path::Path;

/// The one line printed when the tree is sparse.
pub const SPARSE_MESSAGE: &str = "sparse worktree: run `git sparse-checkout disable` or use a full worktree";

/// What the paddock-dev and conformance tests read around themselves.
const NEEDED: [&str; 3] = [
    "tools/paddock-dev/Cargo.toml",
    "conformance/package.json",
    ".gitea/workflows/ci.yml",
];

/// `Some(SPARSE_MESSAGE)` when a folder the tests need is missing from `root`.
pub fn sparse_problem(root: &Path) -> Option<&'static str> {
    NEEDED.iter().any(|p| !root.join(p).exists()).then_some(SPARSE_MESSAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[&str]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for f in files {
            let p = d.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        d
    }

    #[test]
    fn a_tree_missing_a_needed_folder_gets_one_clear_line() {
        let d = tree(&["tools/paddock-dev/Cargo.toml", "ui/package.json"]);
        assert_eq!(sparse_problem(d.path()), Some(SPARSE_MESSAGE));
        assert_eq!(SPARSE_MESSAGE.lines().count(), 1);
    }

    #[test]
    fn a_non_cone_sparse_tree_that_has_the_paths_passes_as_ci_host_clones_are() {
        let d = tree(&NEEDED);
        let git = |args: &[&str]| {
            assert!(
                paddock_proc::command("git")
                    .arg("-C")
                    .arg(d.path())
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "{args:?}"
            );
        };
        git(&["init", "-q"]);
        git(&["config", "core.sparseCheckout", "true"]);
        git(&["config", "core.sparseCheckoutCone", "false"]);
        assert_eq!(sparse_problem(d.path()), None, "the key is set but every needed path exists");
    }
}
