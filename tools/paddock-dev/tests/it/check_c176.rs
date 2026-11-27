//! C176 (a test builds its paths in a temp folder it owns, never by joining onto the shared temp parent): the Rust host over fixture
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
        only: Some(vec!["C176".into()]),
        files: None,
        today: "2026-11-27".into(),
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

const SHARED: &str = "fn f() {\n    let bare = std::env::temp_dir().join(format!(\"origin-{}.git\", std::process::id()));\n}\n";
const OWNED: &str = "fn f() {\n    let d = tempfile::tempdir().unwrap();\n    let bare = d.path().join(\"origin.git\");\n}\n";

/// Positive control for the fixture: the old origin fixture is a warning at its line, never an error (report-only, ER-41).
#[test]
fn c176_reports_a_join_onto_the_shared_temp_folder_as_a_warning() {
    let tree = tree_of(&[
        ("tools/paddock-dev/tests/it/origin.rs", SHARED),
        (
            "crates/a/tests/it/parent.rs",
            "fn f(r: &Repo) {\n    let m = r.path().join(\"..\").join(\"msg-final.txt\");\n}\n",
        ),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(
        got,
        vec![("crates/a/tests/it/parent.rs", 2), ("tools/paddock-dev/tests/it/origin.rs", 2)],
        "{:?}",
        r.findings
    );
    assert!(r.findings[0].message.contains("shared temp folder"), "{:?}", r.findings);
    assert_eq!((r.errors(), r.warnings()), (0, 2));
}

/// The same path under a folder the test owns stays green, and so does production code that joins onto the temp folder.
#[test]
fn c176_passes_an_owned_folder_and_production_code() {
    let tree = tree_of(&[
        ("tools/paddock-dev/tests/it/origin.rs", OWNED),
        (
            "crates/a/src/lib.rs",
            "pub fn p() -> std::path::PathBuf {\n    std::env::temp_dir().join(\"x\")\n}\n",
        ),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// A site waived in place with its reason is listed as waived and is no warning; a waiver without a reason stays a warning.
#[test]
fn c176_a_waiver_with_a_reason_lists_the_site_and_one_without_does_not() {
    let waived = "fn f(r: &Repo) {\n    // conformance: shared-temp-parent — a traversal path built to be refused, never written\n    let m = r.path().join(\"..\").join(\"x\");\n}\n";
    let bare = "fn f(r: &Repo) {\n    // conformance: shared-temp-parent\n    let m = r.path().join(\"..\").join(\"x\");\n}\n";
    let tree = tree_of(&[("crates/a/tests/it/waived.rs", waived), ("crates/b/tests/it/bare.rs", bare)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert_eq!((r.errors(), r.warnings()), (0, 1), "{:?}", r.findings);
    assert_eq!(r.waived.len(), 1, "{:?}", r.waived);
    assert!(r.findings.iter().any(|f| f.message.contains("gives no reason")), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the shared joins.
#[test]
fn c176_control_sees_a_builder() {
    let tree = tree_of(&[("crates/a/tests/it/a.rs", "fn a() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C176").expect("a run record").record;
    assert!(record.control, "the control found its planted joins");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the files": a whole-tree run that reads no Rust file did not look at anything, which is never a pass.
#[test]
fn c176_without_a_rust_file_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "temp_dir().join(x)\n")]);
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
fn live_c176_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C176 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
