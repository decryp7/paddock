//! C174 (a test fixture that builds a git repository with many git starts uses `paddock_testkit::git_fixture`): the Rust host over fixture
//! trees, the positive control, a tree with nothing to read and this working tree. Rust only: there is no JavaScript twin.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use std::path::Path;

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C174".into()]),
        files: None,
        today: "2026-11-26".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    }
}

fn tree_of(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    d
}

/// A fixture that starts git seven times, as `trial::world` did before it used the shared template.
fn builder() -> String {
    format!("fn world() {{\n{}}}\n", "    git(&dir, &[\"commit\"]);\n".repeat(7))
}

/// Reported, never blocking (report-only, ER-41): a warning, no error, and the exit does not fail.
#[test]
fn c174_reports_a_per_test_repository_builder_as_a_warning() {
    let tree = tree_of(&[("tools/x/src/tests.rs", &builder()), ("tools/x/src/lib.rs", "pub fn f() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, vec![("tools/x/src/tests.rs", 1)], "{:?}", r.findings);
    assert!(r.findings[0].message.contains("starts git 7 times"), "{:?}", r.findings);
    assert_eq!((r.errors(), r.warnings()), (0, 1));
}

/// The same builder written through the shared module is clean.
#[test]
fn c174_passes_a_builder_that_uses_the_shared_template() {
    let text = "fn shape() {\n    paddock_testkit::git_fixture::template(\"n\", \"r\", |b, _| {\n        git(b);\n        git(b);\n        git(b);\n        git(b);\n        git(b);\n        git(b);\n        git(b);\n    });\n}\n";
    let tree = tree_of(&[("tools/x/src/tests.rs", text)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the builders.
#[test]
fn c174_control_sees_a_builder() {
    let tree = tree_of(&[("tools/x/src/tests.rs", "fn a() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C174").expect("a run record").record;
    assert!(record.control, "the control found its planted builders");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the files": a whole-tree run that reads no Rust file did not look at anything, which is never a pass.
#[test]
fn c174_without_a_rust_file_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "git(a);\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// This working tree: the check runs over every source file and never fails the run (report-only); the count is what the register records.
#[test]
fn live_c174_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C174 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
