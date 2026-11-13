//! A sparse checkout is named by one `::error::` line in `dev gates check` and `dev plan` (local runs), never an ENOENT or a plan
//! made from a tree with folders missing. The check itself is `conformance/src/util/full-checkout.mjs`, copied into the fixture.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]
// windows-twin: none: owed; the test came Unix-only with the worktree gates profile (73fad2308) and has no Windows run yet
use paddock_testkit::TestCommand;
use std::path::Path;
use std::process::Command;

// Starts git by program name in a throw-away fixture, never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repository with the real check script, a `ui` folder and a `crates` folder; sparse on `crates` and the script's folders.
fn sparse_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in ["full-checkout.mjs", "is-main.mjs"] {
        let to = root.join("conformance/src/util").join(f);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(repo.join("conformance/src/util").join(f), to).unwrap();
    }
    for d in ["crates", "ui"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
        std::fs::write(root.join(d).join("f.txt"), "x").unwrap();
    }
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    git(root, &["sparse-checkout", "set", "--cone", "crates", "conformance"]);
    dir
}

// Starts the tool under test by its cargo-built path (a throw-away fixture), never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
fn dev(dir: &Path, args: &[&str]) -> std::process::Output {
    TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(args)
        .current_dir(dir)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("PADDOCK_ALLOW_SPARSE")
        .output()
        .unwrap()
}

fn assert_one_line(out: &std::process::Output) {
    let err = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!out.status.success(), "{err}");
    assert_eq!(lines.len(), 1, "just the ::error:: line: {err}");
    assert!(
        lines[0].contains("::error::") && lines[0].contains("ui") && lines[0].contains("git sparse-checkout disable"),
        "{err}"
    );
    assert!(!err.contains("node must be on PATH") && !err.contains("ENOENT"), "{err}");
}

#[test]
fn gates_check_in_a_sparse_checkout_prints_only_the_error_line() {
    let fx = sparse_fixture();
    assert_one_line(&dev(fx.path(), &["gates", "check"]));
}

#[test]
fn plan_in_a_sparse_checkout_prints_only_the_error_line() {
    let fx = sparse_fixture();
    assert_one_line(&dev(fx.path(), &["plan", "--all"]));
}

#[test]
fn a_sparse_checkout_without_conformance_says_so_in_one_line_too() {
    let fx = sparse_fixture();
    git(fx.path(), &["sparse-checkout", "set", "--cone", "crates"]);
    let out = dev(fx.path(), &["gates", "check"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("::error::conformance/ is not in this checkout") && err.lines().filter(|l| !l.trim().is_empty()).count() == 1,
        "{err}"
    );
}
