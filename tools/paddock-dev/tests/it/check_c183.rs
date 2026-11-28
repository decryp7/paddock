//! C183 (a file git sees renamed or deleted is not still named by a path register): the Rust host over a fixture git repository
//! (`paddock_testkit::git_fixture`, rule 102) with a moved file and a stale register line, the positive control, a tree git cannot
//! read, a tree with no register, the registry and CI wiring that make the check unskippable (ER-33), and this working tree. Rust only:
//! there is no JavaScript twin.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::config::Config;
use paddock_dev::check::finding::Severity;
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use paddock_testkit::TestCommand;
use paddock_testkit::git_fixture::{copy_tree, template};
use std::path::Path;

const OLD: &str = "tools/x/old/moved.rs";
const NEW: &str = "tools/x/new/moved.rs";
const GONE: &str = "tools/x/gone.rs";
/// The check's own file: where it reports about itself (a register no file matches), not about a register line.
const SELF_FILE: &str = "tools/paddock-dev/src/check/c183.rs";

/// A body long enough that git pairs the old and the new path as a rename.
const BODY: &str = "pub fn moved() -> u32 {\n    let a = 1;\n    let b = 2;\n    let c = 3;\n    a + b + c\n}\n// more lines keep the two files similar for git's rename detection\n// and a few more\n";

const OWNERS: &str = "# owners\ntools/x/old/moved.rs @o\ntools/x/gone.rs @o\n";
const COPIES: &str = "[[copy]]\nfiles = [\"tools/x/old/moved.rs\", \"tools/x/gone.rs\"]\n";
const PROFILES: &str = "bootstrap tools\nmention tools/x/old/moved.rs a writer\n";
const CONTRACT: &str = "{\n  \"hostTests\": {\"x\": [\"tools/x/old/moved.rs\"]}\n}\n";

#[allow(clippy::disallowed_methods)]
fn git(dir: &Path, args: &[&str]) {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = TestCommand::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn put(root: &Path, rel: &str, text: &str) {
    let to = root.join(rel);
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, text).unwrap();
}

/// A repository on `main` with the registers naming a file and a second one, copied for the test; the test branches from it.
fn repo() -> tempfile::TempDir {
    let t = template("moved-paths", include_str!("check_c183.rs"), |b, _| {
        git(b, &["init", "-q", "-b", "main"]);
        paddock_testkit::git_fixture::lf_only(b);
        put(b, "CODEOWNERS", OWNERS);
        put(b, "docs/known-copies.toml", COPIES);
        put(b, "conformance/ci/checkout-profiles.txt", PROFILES);
        put(b, "apps/a/paddock.ci.json", CONTRACT);
        put(b, OLD, BODY);
        put(b, GONE, "pub fn gone() {}\n");
        git(b, &["add", "--all"]);
        git(b, &["commit", "-q", "-m", "base"]);
        // A parent for the base: on the target branch itself the check reads the commit against its first parent, and a branch with
        // nothing committed yet is that case (a move not committed yet is still seen, against the parent).
        put(b, "docs/readme.md", "x\n");
        git(b, &["add", "--all"]);
        git(b, &["commit", "-q", "-m", "second"]);
    });
    let d = tempfile::tempdir().unwrap();
    copy_tree(&t, d.path());
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    d
}

fn mv(d: &Path) {
    std::fs::create_dir_all(d.join(NEW).parent().unwrap()).unwrap();
    git(d, &["mv", OLD, NEW]);
}

fn commit(d: &Path) {
    git(d, &["add", "--all"]);
    git(d, &["commit", "-q", "-m", "topic"]);
}

fn options(root: &Path) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: Config::bare("fixture"),
        scope: None,
        only: Some(vec!["C183".into()]),
        files: None,
        today: "2026-11-28".into(),
        base_ref: Some("main".into()),
        ci: Some(false),
    }
}

/// The register-line findings of `root` against `main`: `(file, line, message)`, without the check's own notes about the list.
fn findings(root: &Path) -> Vec<(String, usize, String)> {
    let r = run_checks(&options(root), &registry()).unwrap();
    assert_eq!(r.errors(), 0, "report-only, no error: {:?}", r.findings);
    assert!(r.findings.iter().all(|f| f.severity == Severity::Warn), "{:?}", r.findings);
    r.findings
        .iter()
        .filter(|f| f.file != SELF_FILE)
        .map(|f| (f.file.clone(), f.line, f.message.clone()))
        .collect()
}

/// The positive control of the brief: a moved file and a stale register line. Every register that still names the old path is
/// reported at its line with "moved to <new path>".
#[test]
fn c183_names_each_register_line_that_still_has_a_moved_path() {
    let d = repo();
    mv(d.path());
    commit(d.path());
    let got = findings(d.path());
    let at: Vec<(&str, usize)> = got.iter().map(|f| (f.0.as_str(), f.1)).collect();
    assert_eq!(
        at,
        vec![
            ("CODEOWNERS", 2),
            ("apps/a/paddock.ci.json", 2),
            ("conformance/ci/checkout-profiles.txt", 2),
            ("docs/known-copies.toml", 2),
        ],
        "{got:?}"
    );
    assert!(got.iter().all(|f| f.2.contains(&format!("moved to `{NEW}`"))), "{got:?}");
}

/// A deleted file is reported as deleted, and the lines of a file that did not move are left alone.
#[test]
fn c183_reports_a_deleted_file_as_deleted() {
    let d = repo();
    git(d.path(), &["rm", "-q", GONE]);
    commit(d.path());
    let got = findings(d.path());
    let at: Vec<(&str, usize)> = got.iter().map(|f| (f.0.as_str(), f.1)).collect();
    assert_eq!(at, vec![("CODEOWNERS", 3), ("docs/known-copies.toml", 2)], "{got:?}");
    assert!(got.iter().all(|f| f.2.contains("was deleted")), "{got:?}");
}

/// The move is seen before it is committed too (staged or not): the pre-push gate runs on the working tree.
#[test]
fn c183_sees_a_move_that_is_not_committed_yet() {
    let d = repo();
    mv(d.path());
    assert_eq!(findings(d.path()).len(), 4);
}

/// The registers rewritten to the new path, and a branch that moves nothing, are clean.
#[test]
fn c183_is_quiet_when_the_registers_follow_the_move() {
    let d = repo();
    mv(d.path());
    for (rel, text) in [
        ("CODEOWNERS", OWNERS),
        ("docs/known-copies.toml", COPIES),
        ("conformance/ci/checkout-profiles.txt", PROFILES),
        ("apps/a/paddock.ci.json", CONTRACT),
    ] {
        put(d.path(), rel, &text.replace(OLD, NEW));
    }
    git(d.path(), &["rm", "-q", GONE]);
    put(
        d.path(),
        "CODEOWNERS",
        &OWNERS.replace(OLD, NEW).replace("tools/x/gone.rs @o\n", ""),
    );
    put(
        d.path(),
        "docs/known-copies.toml",
        &COPIES.replace(OLD, NEW).replace(", \"tools/x/gone.rs\"", ""),
    );
    commit(d.path());
    assert_eq!(findings(d.path()), vec![]);
    let quiet = repo();
    assert_eq!(findings(quiet.path()), vec![]);
}

/// A path that exists again (a swap) is no move, and a lookalike (a longer path) is not the old path.
#[test]
fn c183_ignores_a_recreated_path_and_longer_lookalikes() {
    let d = repo();
    mv(d.path());
    put(d.path(), OLD, "pub fn other() {}\n");
    put(
        d.path(),
        "CODEOWNERS",
        "# owners\ntools/x/old/moved.rs.bak @o\ntools/x/old/moved.rsx @o\n",
    );
    for rel in [
        "docs/known-copies.toml",
        "conformance/ci/checkout-profiles.txt",
        "apps/a/paddock.ci.json",
    ] {
        put(d.path(), rel, "nothing here\n");
    }
    commit(d.path());
    assert_eq!(findings(d.path()), vec![]);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the stale lines.
#[test]
fn c183_control_sees_a_stale_line() {
    let d = repo();
    mv(d.path());
    commit(d.path());
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C183").expect("a run record").record;
    assert!(record.control, "the control found its planted stale lines");
    assert!(record.probes >= 1, "a register was read");
}

/// "Delete the files": a whole-tree run that reads no register did not look at anything, which is never a pass.
#[test]
fn c183_without_a_register_is_not_measured() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "docs/other.md", "tools/x/old/moved.rs\n");
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
}

/// A folder git cannot read (no work tree) is "not measured", never a clean pass: the report says so.
#[test]
fn c183_without_git_says_it_could_not_measure() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "CODEOWNERS", OWNERS);
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    assert!(
        r.findings
            .iter()
            .any(|f| f.message.contains("not measured") && f.message.contains("not a git work tree")),
        "{:?}",
        r.findings
    );
}

/// What makes the check unskippable: it is registered, and the conformance contract runs it (CI and the push gate's conformance row).
#[test]
fn c183_is_registered_and_in_the_conformance_contract() {
    assert!(registry().iter().any(|c| c.id() == "C183"), "the registry lost C183");
    let text = std::fs::read_to_string(repo_root().join("conformance/paddock.ci.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let listed = json["conformance"]["rust"]
        .as_array()
        .expect("conformance.rust")
        .iter()
        .any(|v| v == "C183");
    assert!(listed, "conformance/paddock.ci.json no longer lists C183 in conformance.rust");
}

/// This working tree: the check runs over every register and never fails the run (report-only), and no declared register is missing.
#[test]
fn live_c183_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C183 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
    assert!(
        r.findings
            .iter()
            .all(|f| f.file != SELF_FILE || !f.message.contains("matches no tracked file")),
        "a declared register matches no file: {:?}",
        r.findings
    );
}
