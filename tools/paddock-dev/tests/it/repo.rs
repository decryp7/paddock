//! The shared precondition of the tests that run against this very working tree: it must be a git checkout (or worktree) of
//! Paddock, not an exported copy (`git archive`, a copied folder). Without one, `git` finds no repository, or an unrelated one
//! further up, and the tests would fail with confusing messages or crawl over the wrong tree; this fails at once and names the fix.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root this crate lives in (`<root>/tools/paddock-dev`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Why `root` is not a git working tree of its own (no `.git` there or above, or a repository above that is not `root`), or
/// `None` when it is. `ceiling` stops git's upward search above that folder (for a test that must not see the real repository).
pub fn repo_problem(root: &Path, ceiling: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--show-toplevel"]).current_dir(root);
    if let Some(c) = ceiling {
        cmd.env("GIT_CEILING_DIRECTORIES", c);
    }
    let fix = "Run these tests in a git checkout or worktree, not in an exported copy: git worktree add <folder> origin/main";
    let out = match cmd.output() {
        Ok(out) => out,
        Err(e) => return Some(format!("{} is not usable: git cannot be run ({e}). {fix}", root.display())),
    };
    if !out.status.success() {
        return Some(format!(
            "{} is not inside a git repository (no .git folder found). {fix}",
            root.display()
        ));
    }
    let top = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    match (top.canonicalize(), root.canonicalize()) {
        (Ok(top), Ok(here)) if top == here => None,
        _ => Some(format!(
            "{} is not the root of its git repository (git reports {}), so it is a copy inside another repository. {fix}",
            root.display(),
            top.display()
        )),
    }
}

/// The repository root, or a panic naming the fix when this tree is not a git working tree. Use it first in every test that
/// needs the repository.
pub fn require_repo() -> PathBuf {
    let root = repo_root();
    if let Some(problem) = repo_problem(&root, None) {
        panic!("{problem}");
    }
    root
}

#[test]
fn a_folder_without_a_repository_fails_at_once_and_names_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    let problem = repo_problem(dir.path(), dir.path().parent()).expect("a bare folder is not a repository");
    assert!(
        problem.contains("not inside a git repository") && problem.contains("git worktree add"),
        "{problem}"
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "discovery must not crawl");
}

#[test]
fn a_folder_inside_another_repository_is_refused_and_a_repository_root_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let init = Command::new("git").args(["init", "-q"]).current_dir(dir.path()).status().unwrap();
    assert!(init.success());
    assert_eq!(repo_problem(dir.path(), dir.path().parent()), None);
    let inner = dir.path().join("copy/tools/paddock-dev");
    std::fs::create_dir_all(&inner).unwrap();
    let problem = repo_problem(&dir.path().join("copy"), dir.path().parent()).expect("inside another repository");
    assert!(problem.contains("not the root of its git repository"), "{problem}");
}
