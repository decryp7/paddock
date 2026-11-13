//! C121 (tests compare paths as paths, ADR 0099): the Rust host against the conformance host as the oracle, on a fixture tree and on
//! this working tree, and the tree's own state (no rule A or B site is left; the check starts with an empty baseline).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, has_node, node, node_ticking, repo_root, skip};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

/// Files with sites of every rule, a waiver, non-test code, a skipped folder and non-ASCII text before a site.
const CASES: [(&str, &str); 6] = [
    (
        "crates/a/tests/it/bad.rs",
        "// é😀\nfn t() {\n    let a = format!(\"{}/x\", p.display());\n    let b = s.replace('\\\\', \"/\");\n    assert_eq!(p.display().to_string(), \"x\");\n    let c = format!(\"{}\\\\{}\", p.display(), n);\n}\n",
    ),
    (
        "crates/a/tests/it/waived.rs",
        "fn t() {\n    // conformance: path-text — a URL path, not a file\n    let a = format!(\"{}/x\", u.display());\n    // conformance: path-text\n    let b = format!(\"{}/y\", u.display());\n}\n",
    ),
    (
        "crates/a/tests/it/good.rs",
        "fn t() {\n    let a = format!(\"{}/x\", name);\n    let b = s.replace(\"a\", \"/\");\n}\n",
    ),
    (
        "crates/a/src/lib.rs",
        "fn f() { let a = format!(\"{}/x\", p.display()); }\n#[cfg(test)]\nmod tests {\n    fn t() { let a = format!(\"{}/x\", p.display()); }\n}\n",
    ),
    ("crates/a/src/x_tests.rs", "fn t() { let b = s.replace(r\"\\\", \"/\"); }\n"),
    ("apps/d/vendor/s/tests/v.rs", "fn t() { let a = format!(\"{}/x\", p.display()); }\n"),
];

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        app: app.into(),
        only: Some(vec!["C121".into()]),
        files: None,
        today: "2026-11-13".into(),
        base_ref: Some("origin/main".into()),
    }
}

fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

const JS_RUN: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C121'] });
process.stdout.write(JSON.stringify(r));
"#;

fn fixture_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in CASES {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    d
}

#[test]
fn the_fixture_tree_reports_the_sites_of_every_rule_and_only_in_test_code() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(String, usize)> = r.findings.iter().map(|f| (f.file.clone(), f.line)).collect();
    assert_eq!(
        got,
        [
            ("crates/a/src/lib.rs".to_string(), 4),
            ("crates/a/src/x_tests.rs".to_string(), 1),
            ("crates/a/tests/it/bad.rs".to_string(), 3),
            ("crates/a/tests/it/bad.rs".to_string(), 4),
            ("crates/a/tests/it/bad.rs".to_string(), 5),
            ("crates/a/tests/it/bad.rs".to_string(), 6),
            ("crates/a/tests/it/waived.rs".to_string(), 5),
        ]
    );
    assert_eq!(r.errors(), 0, "report-only: every finding is a warning (ER-41)");
    assert_eq!(r.warnings(), 7);
}

#[test]
fn live_fixture_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = fixture_tree();
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[("RT5_ROOT", tree.path().display().to_string()), ("RT5_APP", "fixture".into())],
    ))
    .unwrap();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert_eq!(shared_part(&r.json()), shared_part(&js));
}

#[test]
fn live_tree_findings_equal_the_js_host_and_no_rule_a_or_b_site_is_left() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let js_root = root.clone();
    let js = paddock_testkit::guarded("the JS C121 over the live tree", LIVE_STALL, move |progress| {
        let driver = format!(
            "import fs from 'node:fs';\nimport {{ syncBuiltinESMExports }} from 'node:module';\nconst read = fs.readFileSync;\nfs.readFileSync = (...a) => {{ process.stdout.write('.\\n'); return read(...a); }};\nsyncBuiltinESMExports();\n{JS_RUN}"
        );
        let env = [("RT5_ROOT", js_root.display().to_string()), ("RT5_APP", "paddock".into())];
        serde_json::from_str::<Value>(node_ticking(&driver, &env, &progress).trim()).unwrap()
    });
    let r = paddock_testkit::guarded("the Rust C121 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    assert_eq!(shared_part(&r.json()), shared_part(&js));
    // The ADR's WP-3b: all sites were moved onto paddock_testkit::path, so the check starts with an empty baseline. Rule C
    // warnings are a heuristic and stay.
    let left: Vec<String> = r
        .findings
        .iter()
        .filter(|f| f.message.contains("(rule A:") || f.message.contains("(rule B:"))
        .map(|f| format!("{}:{}", f.file, f.line))
        .collect();
    assert!(left.is_empty(), "rule A or B sites are back in test code: {left:?}");
}
