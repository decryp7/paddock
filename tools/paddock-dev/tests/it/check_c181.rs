//! C181 (a test reads a lock's state through `paddock_testkit::lock::await_free` or `await_held`, never once in an assertion): the Rust
//! host over fixture trees, the positive control, a tree with nothing to read, the registry and this working tree. Rust only: there is
//! no JavaScript twin.
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
        only: Some(vec!["C181".into()]),
        files: None,
        today: "2026-11-28".into(),
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

/// The shape of the throttle `filelock` test: one read of the slot after the handle was dropped.
const DIRECT: &str = "#[test]\nfn freed() {\n    drop(held);\n    assert!(!paddock_throttle::slotfile::is_held(&lock).unwrap());\n}\n";

/// A direct read in an assertion is a warning at the read, never an error (report-only, ER-41), and names the helper.
#[test]
fn c181_reports_a_direct_read_as_a_warning() {
    let tree = tree_of(&[("crates/x/tests/it/lock.rs", DIRECT), ("crates/x/src/lib.rs", "pub fn f() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, vec![("crates/x/tests/it/lock.rs", 4)], "{:?}", r.findings);
    assert!(r.findings[0].fix.contains("await_free"), "the helper is named: {:?}", r.findings);
    assert_eq!((r.errors(), r.warnings()), (0, 1));
}

/// The same expectation through the helper is clean.
#[test]
fn c181_passes_the_read_through_the_helper() {
    let text = "#[test]\nfn freed() {\n    drop(held);\n    paddock_testkit::lock::await_free(\"the slot is free\", || paddock_throttle::slotfile::is_held(&lock).unwrap());\n}\n";
    let tree = tree_of(&[("crates/x/tests/it/lock.rs", text)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the direct reads.
#[test]
fn c181_control_sees_a_direct_read() {
    let tree = tree_of(&[("crates/x/tests/it/lock.rs", "#[test]\nfn a() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C181").expect("a run record").record;
    assert!(record.control, "the control found its planted reads");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the check": it is registered, so the host runs it, and a tree with nothing to read is not a pass.
#[test]
fn c181_is_registered_and_a_tree_with_no_rust_file_is_not_measured() {
    assert!(
        registry().iter().any(|c| c.id() == "C181"),
        "C181 is registered in register_checks!"
    );
    let tree = tree_of(&[("docs/other.md", "assert!(is_held(&p));\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// This working tree: the check runs over every Rust file and never fails the run (report-only).
#[test]
fn live_c181_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C181 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
