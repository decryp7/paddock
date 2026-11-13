//! C119 (no workflow job runs `./dev` before its toolchain is set up): the Rust host against the JavaScript check, the oracle, on a
//! tree of every shape the check knows and on this working tree. Node is required: a missing oracle fails the test (ER-33).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::{RunOptions, registry, run_checks};
use paddock_testkit::TestCommand;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

const DRIVER: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.C119_ROOT, app: 'paddock', only: ['C119'] });
process.stdout.write(JSON.stringify({ findings: r.findings, errors: r.errors, warnings: r.warnings }));
"#;

fn js(root: &Path) -> Value {
    let out = TestCommand::new("node")
        .args(["--input-type=module", "-e", DRIVER])
        .current_dir(repo())
        .env("C119_ROOT", root)
        .output()
        .expect("node must be installed: the oracle cannot be skipped");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn rust(root: &Path) -> Value {
    let opts = RunOptions {
        root: root.to_path_buf(),
        app: "paddock".into(),
        only: Some(vec!["C119".into()]),
        files: None,
        today: paddock_dev::check::today_utc(),
        base_ref: None,
    };
    let r = run_checks(&opts, &registry()).unwrap().json();
    json!({"findings": r["findings"], "errors": r["errors"], "warnings": r["warnings"]})
}

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in files {
        let path = d.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, t).unwrap();
    }
    d
}

#[test]
fn every_shape_gives_the_same_findings_in_both_hosts() {
    let d = tree(&[
        (
            ".gitea/actions/kept/action.yml",
            "runs:\n  steps:\n    - run: bash conformance/ci/kit.sh host x\n",
        ),
        (".gitea/actions/plain/action.yml", "runs:\n  steps:\n    - run: echo\n"),
        (
            ".gitea/workflows/a.yml",
            "name: a\njobs:\n  bad:\n    steps:\n      - uses: actions/checkout@v4\n      - run: ./dev x\n      - run: ./dev y\n  kit:\n    steps:\n      - uses: ./.gitea/actions/kept\n      - run: ./dev x\n  plainjob:\n    steps:\n      - uses: ./.gitea/actions/plain\n      - name: n\n        run: |\n          set -e\n          ./dev z\n",
        ),
        (
            ".gitea/workflows/b.yml",
            "jobs:\n  w:\n    steps:\n      # conformance: dev-before-toolchain — the image has the only compiler\n      - run: ./dev x\n  bare:\n    steps:\n      # conformance: dev-before-toolchain\n      - run: ./dev x\n  c:\n    steps:\n      # ./dev x\n      - run: echo ./dev\n      - run: ./devx\n  d:\n    steps:\n      - uses: ./.gitea/actions/setup-rust\n      - run: ./dev ok\n",
        ),
    ]);
    let (a, b) = (js(d.path()), rust(d.path()));
    assert_eq!(a, b);
    assert_eq!(a["warnings"], 4, "bad, plainjob, bare and c: {a}");
}

#[test]
fn a_tree_without_workflows_is_check_broken_in_both_never_a_pass() {
    let d = tree(&[("README.md", "x")]);
    let (a, b) = (js(d.path()), rust(d.path()));
    assert!(a["errors"].as_u64().unwrap() >= 1 && b["errors"].as_u64().unwrap() >= 1, "{a} {b}");
}

#[test]
fn this_working_tree_gives_the_same_findings_in_both_hosts() {
    let root = repo();
    assert_eq!(js(&root)["findings"], rust(&root)["findings"]);
}
