//! C180 (under pipefail, no `| grep -q`, `| grep -m` or `| head`): the Rust host over fixture trees, the positive control, a tree
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
        only: Some(vec!["C180".into()]),
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

/// The shape of run 9247: a step with no `shell:` (act runs bash -e -o pipefail) that pipes a large output into `grep -q`.
const WORKFLOW: &str = "jobs:\n  quick:\n    runs-on: x\n    steps:\n      - name: probe\n        run: |\n          tree=$(cargo tree)\n          echo \"$tree\" | grep -q '^paddock-proc ' || exit 1\n";
const SCRIPT: &str = "#!/usr/bin/env bash\nset -euo pipefail\nfoo | grep -q a\nfirst=$(foo | head -n1)\n";

/// A workflow step and a script with the bad shape are warnings at their line, never errors (report-only, ER-41), and name the safe form.
#[test]
fn c180_reports_a_workflow_step_and_a_script_as_warnings() {
    let tree = tree_of(&[(".gitea/workflows/w.yml", WORKFLOW), ("scripts/x.sh", SCRIPT)]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(
        got,
        vec![(".gitea/workflows/w.yml", 8), ("scripts/x.sh", 3), ("scripts/x.sh", 4)],
        "{:?}",
        r.findings
    );
    assert!(r.findings[0].fix.contains("<<<"), "the safe form is named: {:?}", r.findings);
    assert_eq!((r.errors(), r.warnings()), (0, 3));
}

/// The safe forms, a shell without pipefail and a Windows shell stay green.
#[test]
fn c180_passes_the_safe_forms() {
    let tree = tree_of(&[
        (
            ".gitea/workflows/w.yml",
            "jobs:\n  a:\n    runs-on: x\n    steps:\n      - run: |\n          out=$(cargo tree)\n          grep -q '^paddock-proc ' <<<\"$out\"\n      - shell: cmd\n        run: dir | grep -q a\n",
        ),
        ("scripts/x.sh", "#!/bin/sh\nset -e\nfoo | grep -q a\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the pipelines.
#[test]
fn c180_control_sees_a_builder() {
    let tree = tree_of(&[("scripts/x.sh", "#!/usr/bin/env bash\nset -eu\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C180").expect("a run record").record;
    assert!(record.control, "the control found its planted pipelines");
    assert!(record.probes >= 1, "the file was read");
}

/// "Delete the files": a whole-tree run that reads no script or workflow did not look at anything, which is never a pass.
#[test]
fn c180_without_a_script_is_not_measured() {
    let tree = tree_of(&[("docs/other.md", "foo | grep -q a\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// A workflow the YAML reader cannot read was not looked at: not measured, never a pass.
#[test]
fn c180_a_workflow_that_does_not_parse_is_not_measured() {
    let tree = tree_of(&[(".gitea/workflows/w.yml", "jobs: [\n  steps: x\n"), ("scripts/x.sh", "set -e\n")]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(r.errors() >= 1, "{:?}", r.findings);
    assert!(
        r.findings.iter().all(|f| f.outcome.is_some_and(|o| o.name() == "not-measured")),
        "{:?}",
        r.findings
    );
}

/// This working tree: the check runs over every script and workflow and never fails the run (report-only).
#[test]
fn live_c180_the_real_tree_runs_and_blocks_nothing() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C180 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(r.errors(), 0, "report-only: {:?}", r.findings);
}
