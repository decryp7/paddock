//! C119 (no workflow job runs `./dev` before its toolchain is set up): the Rust host against the answers the JavaScript check gave
//! (recorded, `tests/fixtures/check/recorded`; the JS module is retired, pair P3) on a tree of every shape the check knows and on this tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn full(root: &Path) -> Value {
    let opts = RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(vec!["C119".into()]),
        files: None,
        today: paddock_dev::check::today_utc(),
        base_ref: None,
        ci: Some(false),
    };
    run_checks(&opts, &registry()).unwrap().json()
}

fn rust(root: &Path) -> Value {
    let r = full(root);
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
    let b = rust(d.path());
    let a = crate::recorded::expect("c119-every-shape", &b);
    assert_eq!(a, b);
    assert_eq!(a["warnings"], 4, "bad, plainjob, bare and c: {a}");
}

#[test]
fn a_tree_without_workflows_is_check_broken_in_both_never_a_pass() {
    let d = tree(&[("README.md", "x")]);
    let b = rust(d.path());
    let a = crate::recorded::expect("c119-no-workflows", &b);
    assert!(a["errors"].as_u64().unwrap() >= 1 && b["errors"].as_u64().unwrap() >= 1, "{a} {b}");
}

#[test]
fn this_working_tree_run_is_sound() {
    let root = repo();
    crate::recorded::assert_live_tree_sound("c119", &full(&root));
}
