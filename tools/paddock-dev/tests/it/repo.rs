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
    let mut cmd = paddock_proc::command("git");
    cmd.args(["rev-parse", "--show-toplevel"]).current_dir(root).env("LC_ALL", "C");
    cmd.env_remove("GIT_DIR").env_remove("GIT_WORK_TREE");
    if let Some(c) = ceiling {
        cmd.env("GIT_CEILING_DIRECTORIES", c);
    }
    let fix = "Run these tests in a git checkout or worktree, not in an exported copy: git worktree add <folder> origin/main";
    let out = match paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(std::time::Duration::from_secs(10))) {
        Ok(out) => out,
        Err(e) => return Some(format!("{} is not usable: git cannot be run ({e}). {fix}", root.display())),
    };
    if !out.success() {
        let said = out.stderr_text();
        return Some(format!(
            "{} is not inside a git repository (git said: {}). {fix}",
            root.display(),
            said.trim()
        ));
    }
    let top = PathBuf::from(out.stdout_text().trim());
    match (top.canonicalize(), root.canonicalize()) {
        (Ok(top), Ok(here)) if top == here => None,
        _ => Some(format!(
            "{} is not the root of its git repository (git reports {}), so it is a copy inside another repository. {fix}",
            root.display(),
            top.display()
        )),
    }
}

/// What a test that needs the repository does with this tree.
#[derive(Debug, PartialEq, Eq)]
pub enum Need {
    /// The tree is a usable git checkout: run.
    Run(PathBuf),
    /// Not a checkout and nothing demands one: print the reason and return.
    Skip(String),
    /// Not a checkout but CI or `PADDOCK_REQUIRE_GIT` demands one: fail with the reason.
    Fail(String),
}

/// Whether a `CI` value means "running in CI": set and not empty, `0` or `false`.
pub fn ci_on(ci: Option<&str>) -> bool {
    ci.is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// Whether a `.git` (folder or file) exists at `root` or any folder above it.
pub fn git_marker_present(root: &Path) -> bool {
    root.ancestors().any(|a| a.join(".git").exists())
}

/// The pure decision. `problem` is from [`repo_problem`]; `git_present` says a `.git` exists at or above the root. Only a tree
/// with no `.git` at all (an export) can skip; a `.git` that git cannot use (broken worktree, bad ownership, corrupt) or a copy
/// inside another repository fails with git's own message. `CI` (see [`ci_on`]) or a non-empty `PADDOCK_REQUIRE_GIT` make even
/// the export fail.
pub fn decide(root: PathBuf, problem: Option<String>, git_present: bool, ci: Option<&str>, require_git: Option<&str>) -> Need {
    let required = ci_on(ci) || require_git.is_some_and(|v| !v.is_empty());
    match problem {
        None => Need::Run(root),
        Some(p) if required || git_present => Need::Fail(p),
        Some(p) => Need::Skip(p),
    }
}

/// The repository root when this tree is a git working tree; otherwise prints `SKIPPED: <test> needs a git checkout: <why>` and
/// returns `None` (the caller returns), or panics when the decision is to fail. The line is written straight to stderr, past the
/// test harness's capture; nextest shows it for these tests through `success-output` overrides in `.config/nextest.toml`.
pub fn require_repo(test: &str) -> Option<PathBuf> {
    use std::io::Write;
    let root = repo_root();
    let problem = repo_problem(&root, None);
    let (ci, req) = (std::env::var("CI").ok(), std::env::var("PADDOCK_REQUIRE_GIT").ok());
    match decide(root.clone(), problem, git_marker_present(&root), ci.as_deref(), req.as_deref()) {
        Need::Run(root) => Some(root),
        Need::Skip(why) => {
            let mut err = std::io::stderr();
            let _ = err.write_all(format!("SKIPPED: {test} needs a git checkout: {why}\n").as_bytes());
            let _ = err.flush();
            None
        }
        Need::Fail(why) => panic!("{test} needs a git checkout: {why}"),
    }
}

#[test]
fn without_a_repository_the_test_skips_unless_ci_or_require_git_is_set() {
    let root = || PathBuf::from("x");
    let why = || Some("no repo".to_string());
    assert_eq!(decide(root(), None, true, Some("true"), Some("1")), Need::Run(root()));
    assert_eq!(decide(root(), why(), false, None, None), Need::Skip("no repo".into()));
    assert_eq!(decide(root(), why(), false, Some(""), Some("")), Need::Skip("no repo".into()));
    assert_eq!(decide(root(), why(), false, Some("false"), Some("")), Need::Skip("no repo".into()));
    assert_eq!(decide(root(), why(), false, Some("0"), None), Need::Skip("no repo".into()));
    assert_eq!(decide(root(), why(), false, Some("true"), None), Need::Fail("no repo".into()));
    assert_eq!(decide(root(), why(), false, None, Some("1")), Need::Fail("no repo".into()));
    // A .git that git cannot use never skips.
    assert_eq!(decide(root(), why(), true, None, None), Need::Fail("no repo".into()));
}

#[test]
fn a_broken_git_file_fails_with_gits_message_and_no_git_at_all_skips() {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("export");
    std::fs::create_dir(&bare).unwrap();
    let problem = repo_problem(&bare, dir.path().parent());
    assert!(!git_marker_present(&bare));
    assert!(matches!(
        decide(bare.clone(), problem, git_marker_present(&bare), None, None),
        Need::Skip(_)
    ));

    let broken = dir.path().join("worktree");
    std::fs::create_dir(&broken).unwrap();
    std::fs::write(broken.join(".git"), "gitdir: /nonexistent/paddock-dev-test/gitdir\n").unwrap();
    let problem = repo_problem(&broken, dir.path().parent());
    assert!(git_marker_present(&broken));
    match decide(broken.clone(), problem, git_marker_present(&broken), None, None) {
        Need::Fail(why) => assert!(why.contains("git said: fatal:"), "{why}"),
        other => panic!("{other:?}"),
    }
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
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .status()
        .unwrap();
    assert!(init.success());
    assert_eq!(repo_problem(dir.path(), dir.path().parent()), None);
    let inner = dir.path().join("copy/tools/paddock-dev");
    std::fs::create_dir_all(&inner).unwrap();
    let problem = repo_problem(&dir.path().join("copy"), dir.path().parent()).expect("inside another repository");
    assert!(problem.contains("not the root of its git repository"), "{problem}");
}
