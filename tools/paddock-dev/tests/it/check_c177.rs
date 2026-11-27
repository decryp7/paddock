//! C177 (a test does not read a register a tool writes as the live file): the Rust host over fixture trees, the positive control, a tree
//! with nothing to read and this working tree. Rust only: there is no JavaScript twin.
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
        only: Some(vec!["C177".into()]),
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

const INCLUDE: &str = "const CFG: &str = include_str!(\"../../../../docs/test-cache.toml\");\n";
const READ: &str = "fn t() {\n    let p = repo_root().join(\"docs/escape-register.toml\");\n    let _ = std::fs::read_to_string(p);\n}\n";
const TEMP: &str = "fn t() {\n    let d = tempfile::tempdir().unwrap();\n    let _ = std::fs::read_to_string(d.path().join(\"docs/escape-register.toml\"));\n}\n";

/// Positive control for the fixture: both live reads are warnings at their line, never errors (report-only, ER-41).
#[test]
fn c177_reports_an_include_and_a_live_read_as_warnings() {
    let tree = tree_of(&[("tools/x/src/a/tests.rs", INCLUDE), ("tools/x/tests/it/b.rs", READ)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(
        got,
        vec![("tools/x/src/a/tests.rs", 1), ("tools/x/tests/it/b.rs", 2)],
        "{:?}",
        r.findings
    );
    assert!(
        r.findings[0].message.contains("shadow-run --write"),
        "the writer is named: {:?}",
        r.findings
    );
    assert_eq!((r.errors(), r.warnings()), (0, 2));
}

/// The same read of a copy under the test's own temporary folder stays green, and so does the accessor.
#[test]
fn c177_passes_a_fixture_copy_and_the_accessor() {
    let tree = tree_of(&[
        ("tools/x/tests/it/temp.rs", TEMP),
        (
            "tools/x/tests/it/accessor.rs",
            "fn t() {\n    let c = Config::read(&repo_root()).unwrap();\n}\n",
        ),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the live reads.
#[test]
fn c177_control_sees_a_builder() {
    let tree = tree_of(&[("tools/x/src/a.rs", "fn a() {}\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C177").expect("a run record").record;
    assert!(record.control, "the control found its planted reads");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the files": a whole-tree run that reads no Rust file did not look at anything, which is never a pass.
#[test]
fn c177_without_a_rust_file_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "docs/test-cache.toml\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// This working tree: the check runs over every Rust file and never fails the run (report-only), and it finds no live read of a
/// register (the count the register records).
#[test]
fn live_c177_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C177 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
