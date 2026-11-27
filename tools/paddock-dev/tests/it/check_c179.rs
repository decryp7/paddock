//! C179 (a crate whose tests read files the graph cannot resolve declares them): the Rust host over fixture trees, the positive control,
//! a tree with nothing to read and this working tree. Rust only: there is no JavaScript twin.
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
        only: Some(vec!["C179".into()]),
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

const WS: &str = "[workspace]\nmembers = [\"a\", \"b\"]\n";
const BARE: &str = "[package]\nname = \"a\"\nversion = \"0.1.0\"\n";
const DECLARED: &str = "[package]\nname = \"b\"\nversion = \"0.1.0\"\n[package.metadata.paddock]\ntest-reads = [\"data/**\"]\n";
const SITE: &str = "pub fn f() {\n    let _ = std::env::current_dir();\n}\n";

/// A blind site with no declaration is a warning on the crate's manifest, never an error (report-only, ER-41); the same site with a
/// declaration is not reported.
#[test]
fn c179_reports_a_blind_crate_without_a_declaration_as_a_warning() {
    let tree = tree_of(&[
        ("Cargo.toml", WS),
        ("a/Cargo.toml", BARE),
        ("a/src/lib.rs", SITE),
        ("b/Cargo.toml", DECLARED),
        ("b/src/lib.rs", SITE),
        ("data/x.txt", "x\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, vec![("a/Cargo.toml", 1)], "{:?}", r.findings);
    assert!(
        r.findings[0].message.contains("a/src/lib.rs"),
        "the site is named: {:?}",
        r.findings
    );
    assert_eq!((r.errors(), r.warnings()), (0, 1));
}

/// A declared glob that matches no tracked file is a finding too.
#[test]
fn c179_reports_a_glob_that_matches_nothing() {
    let tree = tree_of(&[
        ("Cargo.toml", WS),
        ("a/Cargo.toml", BARE),
        ("a/src/lib.rs", "pub fn f() {}\n"),
        ("b/Cargo.toml", DECLARED),
        ("b/src/lib.rs", SITE),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<&str> = r.findings.iter().map(|f| f.file.as_str()).collect();
    assert_eq!(got, vec!["b/Cargo.toml"], "{:?}", r.findings);
    assert!(r.findings[0].message.contains("data/**"), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the gaps.
#[test]
fn c179_control_sees_a_gap() {
    let tree = tree_of(&[("Cargo.toml", WS), ("a/Cargo.toml", BARE), ("a/src/lib.rs", "pub fn f() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C179").expect("a run record").record;
    assert!(record.control, "the control found its planted gaps");
}

/// "Delete the files": a whole-tree run that reads no Rust file with a read the graph may not resolve did not look at anything, which is
/// never a pass.
#[test]
fn c179_without_a_candidate_file_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "x\n")]);
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
fn live_c179_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C179 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
