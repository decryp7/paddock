//! C123 (a Rust test starts ./dev, cargo or node only through TestCommand): the Rust host against the JavaScript check, the oracle, on a
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
const r = await runChecks({ root: process.env.C123_ROOT, app: 'paddock', only: ['C123'] });
process.stdout.write(JSON.stringify({ findings: r.findings, errors: r.errors, warnings: r.warnings }));
"#;

fn js(root: &Path) -> Value {
    let out = TestCommand::new("node")
        .args(["--input-type=module", "-e", DRIVER])
        .current_dir(repo())
        .env("C123_ROOT", root)
        .output()
        .expect("node must be installed: the oracle cannot be skipped");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn rust(root: &Path) -> Value {
    let opts = RunOptions {
        root: root.to_path_buf(),
        app: "paddock".into(),
        only: Some(vec!["C123".into()]),
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
            "crates/a/tests/it/bad.rs",
            "fn t() {\n    Command::new(\"node\").output();\n    paddock_proc::command(\n        env!(\"CARGO\"),\n    );\n    quiet_command(std::env::var(\"CARGO\").unwrap());\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    run(bin);\n}\n",
        ),
        (
            "crates/a/tests/it/good.rs",
            "fn t() {\n    TestCommand::new(\"node\").output();\n    Command::new(\"git\").output();\n    // Command::new(\"cargo\")\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    TestCommand::new(\"sh\").arg(bin);\n}\n",
        ),
        (
            "crates/a/src/lib.rs",
            "fn run() {\n    Command::new(\"cargo\");\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        Command::new(\"cargo\");\n    }\n}\n",
        ),
        ("crates/a/src/plain.rs", "fn run() {\n    Command::new(\"cargo\");\n}\n"),
        (
            "crates/a/tests/it/waived.rs",
            "fn t() {\n    // conformance: test-env \u{2014} a probe of the shell\n    Command::new(\"cargo\");\n}\nfn u() {\n    // conformance: test-env\n    Command::new(\"cargo\");\n}\n",
        ),
    ]);
    let (a, b) = (js(d.path()), rust(d.path()));
    assert_eq!(a, b);
    assert_eq!(a["warnings"], 6, "bad.rs 4, lib.rs 1, the bare waiver 1: {a}");
}

#[test]
fn a_tree_without_rust_tests_is_check_broken_in_both_never_a_pass() {
    let d = tree(&[("README.md", "x")]);
    let (a, b) = (js(d.path()), rust(d.path()));
    assert!(a["errors"].as_u64().unwrap() >= 1 && b["errors"].as_u64().unwrap() >= 1, "{a} {b}");
}

#[test]
fn this_working_tree_gives_the_same_findings_in_both_hosts() {
    let root = repo();
    assert_eq!(js(&root)["findings"], rust(&root)["findings"]);
}
