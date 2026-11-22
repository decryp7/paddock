//! The planner's recorded answers (`./dev plan`) and the fixture repository they are recorded on.
//!
//! This file holds what other test modules share: the fixture repository (`fixture_repo`, also read by `cli` and `gates_planner`),
//! the `git` helper, the bless switch (`check` and `check_units` bless through it) and `record` (also used by `recorded`). The planner's own tests are in
//! [`planner`], which is not built on Windows (see its doc comment).
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The planner's tests: the recorded cases on the fixture repository, the command line cases, the real-tree samples and the
/// narrowing tests on copies of this repository.
///
/// Not built on Windows (user, 2026-11-22): the planner runs only on Linux (ci.yml's plan job) and macOS (the push gate and `./dev`),
/// never on Windows, and on the Windows runner these tests cost minutes for nothing it runs (run 8554: the three copy tests 179 s,
/// 129 s and 75 s; the fixture and real-tree tests 54 s, 32 s and 25 s; run 8555 timed the narrowing test out on both tries; Linux took
/// at most 4 s and macOS 14 s for the same tests; docs/benchmarks.md). The planner's Windows-specific parts are covered by pure tests
/// that run on every OS: the root spelling (`plan::tests::a_graph_root_over_the_limit_is_refused_and_a_short_verbatim_one_is_plain`,
/// with `Os::Windows` passed in), a CRLF workflow (`plan_gitea::tests::a_crlf_workflow_splits_like_its_lf_text`) and a `--files` path
/// with `\` (`plan_ci::tests::a_files_entry_with_a_backslash_is_refused_with_the_next_step`). Build it on Windows again if a Windows
/// host ever runs `./dev plan`.
#[cfg(not(windows))]
mod planner;

/// Whether `PADDOCK_GOLDEN_BLESS=1` asks the recorded tests to rewrite their files from what the planner answers now.
pub(crate) fn bless() -> bool {
    bless_allowed(|k| std::env::var(k).ok())
}

/// The rule behind [`bless`], over an injectable environment: blessing is on only when asked for, and never in CI (a run that rewrites
/// its own reference would pass whatever the planner answered).
fn bless_allowed(env: impl Fn(&str) -> Option<String>) -> bool {
    let asked = env("PADDOCK_GOLDEN_BLESS").is_some_and(|v| v == "1");
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| env(k).is_some_and(|v| v == "true"));
    assert!(
        !(asked && ci),
        "PADDOCK_GOLDEN_BLESS=1 is refused in CI: bless on a workstation, review the diff and commit it"
    );
    asked
}

#[test]
fn blessing_is_refused_in_ci_and_off_unless_asked_for() {
    let env =
        |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    assert!(bless_allowed(env(&[("PADDOCK_GOLDEN_BLESS", "1")])), "asked for, on a workstation");
    assert!(!bless_allowed(env(&[])), "not asked for");
    assert!(!bless_allowed(env(&[("CI", "true")])), "CI alone blesses nothing");
    let refused = std::panic::catch_unwind(|| bless_allowed(env(&[("PADDOCK_GOLDEN_BLESS", "1"), ("GITHUB_ACTIONS", "true")])));
    assert!(refused.is_err(), "asked for in CI is refused");
}

/// Writes `value` as the recorded files are written (one space per level, a final newline).
pub(crate) fn record(path: &Path, value: &Value) {
    // serde_json indents by two spaces; the recorded files use one (structural indentation only: a string never breaks a line).
    let mut out: String = serde_json::to_string_pretty(value)
        .unwrap()
        .lines()
        .map(|l| {
            let indent = l.len() - l.trim_start().len();
            format!("{}{}\n", " ".repeat(indent / 2), l.trim_start())
        })
        .collect();
    if out.is_empty() {
        out.push('\n');
    }
    std::fs::write(path, out).unwrap();
}

// ---------------------------------------------------------------------------------------------------------------------
// The fixture repository

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// A lockfile stored in the fixture as `package-lock.fixture.json`, so the repository's own "one lockfile" check ignores it.
fn placed_name(name: &str) -> &str {
    if name == "package-lock.fixture.json" {
        "package-lock.json"
    } else {
        name
    }
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(placed_name(&entry.file_name().to_string_lossy()));
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn files_below(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_below(&path, base, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_path_buf());
        }
    }
}

pub(crate) fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// The fixture repository: the overlay over the fixture is commit `fixture-base`, the fixture alone (overlay-only files removed) is `fixture-head`.
pub(crate) fn fixture_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = paddock_env::winpath::real(dir.path()).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    copy_tree(&golden_dir().join("fixture"), &root);
    copy_tree(&golden_dir().join("base-overlay"), &root);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["tag", "fixture-base"]);
    copy_tree(&golden_dir().join("fixture"), &root);
    let mut overlay = Vec::new();
    files_below(&golden_dir().join("base-overlay"), &golden_dir().join("base-overlay"), &mut overlay);
    for rel in overlay.into_iter().filter(|r| !golden_dir().join("fixture").join(r).exists()) {
        std::fs::remove_file(root.join(rel)).unwrap(); // overlay-only files carry no renamed names
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "head"]);
    git(&root, &["tag", "fixture-head"]);
    // The steps: one commit each on a side branch, so `main` and HEAD stay at `fixture-head` for the command line cases.
    git(&root, &["checkout", "-q", "-b", "steps"]);
    let mut steps: Vec<PathBuf> = std::fs::read_dir(golden_dir().join("steps"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    steps.sort();
    for step in steps {
        copy_tree(&step, &root);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "step"]);
        git(&root, &["tag", &format!("step-{}", step.file_name().unwrap().to_string_lossy())]);
    }
    git(&root, &["checkout", "-q", "main"]);
    (dir, root)
}
