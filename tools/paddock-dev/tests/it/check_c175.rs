//! C175 (a PADDOCK_ variable is read through paddock-ci-config's settings, not by a string literal): the Rust host over fixture trees, the
//! positive control, a tree with nothing to read and this working tree. Rust only: there is no JavaScript twin.
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
        only: Some(vec!["C175".into()]),
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

const DIRECT: &str = "pub fn home() -> Option<String> {\n    std::env::var(\"PADDOCK_CI_HOME\").ok()\n}\n";
const THROUGH_SETTINGS: &str = "pub fn home(vars: &dyn Vars) -> Option<String> {\n    Settings::new(vars).get(PADDOCK_CI_HOME)\n}\n";

/// Positive control for the fixture: a direct read is a warning at its line, never an error (report-only, ER-41).
#[test]
fn c175_reports_a_direct_read_as_a_warning_and_names_an_undeclared_one() {
    let tree = tree_of(&[
        ("tools/paddock-dev/src/direct.rs", DIRECT),
        (
            "tools/paddock-dev/src/new.rs",
            "fn f() { let _ = std::env::var(\"PADDOCK_NOT_DECLARED_ANYWHERE\"); }\n",
        ),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(
        got,
        vec![("tools/paddock-dev/src/direct.rs", 2), ("tools/paddock-dev/src/new.rs", 1)],
        "{:?}",
        r.findings
    );
    assert!(r.findings[0].message.contains("read it through the settings"), "{:?}", r.findings);
    assert!(r.findings[1].message.contains("is not declared"), "{:?}", r.findings);
    assert_eq!((r.errors(), r.warnings()), (0, 2));
}

/// The same read through the settings stays green.
#[test]
fn c175_passes_the_same_read_through_the_settings() {
    let tree = tree_of(&[("tools/paddock-dev/src/ok.rs", THROUGH_SETTINGS)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the direct reads.
#[test]
fn c175_control_sees_a_builder() {
    let tree = tree_of(&[("tools/paddock-dev/src/a.rs", "fn a() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C175").expect("a run record").record;
    assert!(record.control, "the control found its planted reads");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the files": a whole-tree run that reads no Rust file did not look at anything, which is never a pass.
#[test]
fn c175_without_a_rust_file_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "PADDOCK_X\n")]);
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
fn live_c175_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C175 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
