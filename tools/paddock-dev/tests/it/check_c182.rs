//! C182 (a remote `uses:` is pinned to a full commit SHA): the Rust host over fixture trees, the positive control, a tree with
//! nothing to read and this working tree. Rust only: there is no JavaScript twin.
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
        only: Some(vec!["C182".into()]),
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

const UNPINNED: &str = "jobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n      - uses: ./.gitea/actions/plan\n";
const PINNED: &str = "jobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n      - uses: ./.gitea/actions/plan\n";

/// A moving tag is a warning at its line, never an error (report-only, ER-41), and the fix names the SHA form; a local path is no finding.
#[test]
fn c182_reports_a_moving_tag_as_a_warning() {
    let tree = tree_of(&[(".gitea/workflows/w.yml", UNPINNED), (".gitea/actions/a/action.yml", "runs:\n  using: composite\n  steps:\n    - uses: x/y@main\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, vec![(".gitea/actions/a/action.yml", 4), (".gitea/workflows/w.yml", 5)], "{:?}", r.findings);
    assert!(r.findings[0].fix.contains("40-hex"), "{:?}", r.findings);
    assert_eq!((r.errors(), r.warnings()), (0, 2));
}

/// A SHA with its version comment, local paths and a text that only names `uses:` stay green.
#[test]
fn c182_passes_pins_and_local_paths() {
    let tree = tree_of(&[(".gitea/workflows/w.yml", PINNED), ("docs/x.yml", "steps:\n  - uses: actions/checkout@v4\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the unpinned lines.
#[test]
fn c182_control_sees_a_builder() {
    let tree = tree_of(&[(".gitea/workflows/w.yml", PINNED)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C182").expect("a run record").record;
    assert!(record.control, "the control found its planted unpinned lines");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the check": it is in the registry, reserved, and runs from the conformance contract; removing it fails here.
#[test]
fn c182_is_registered_reserved_and_listed() {
    assert!(registry().iter().any(|c| c.id() == "C182"), "C182 is not in the registry");
    let root = repo_root();
    assert!(std::fs::read_dir(root.join("conformance/reserved")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with("C182-")));
    let ci = std::fs::read_to_string(root.join("conformance/paddock.ci.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&ci).unwrap();
    assert!(v["conformance"]["rust"].to_string().contains("C182"), "C182 is not in conformance.rust");
}

/// "Delete the files": a whole-tree run that reads no workflow or action did not look at anything, which is never a pass.
#[test]
fn c182_without_a_workflow_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "uses: actions/checkout@v4\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")), "{:?}", r.findings);
}

/// A workflow the YAML reader cannot read was not looked at: not measured, never a pass.
#[test]
fn c182_a_workflow_that_does_not_parse_is_not_measured() {
    let tree = tree_of(&[(".gitea/workflows/w.yml", "jobs: [\n  steps: x\n"), (".gitea/workflows/ok.yml", PINNED)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
}

/// This working tree: the check runs over every workflow and action and never fails the run (report-only). After the pinning commit the
/// report is empty, which is the first green run recorded in docs/gates.toml.
#[test]
fn live_c182_the_real_tree_has_no_unpinned_remote_action() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C182 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
    assert!(r.findings.is_empty(), "an unpinned remote `uses:`: {:?}", r.findings);
}
