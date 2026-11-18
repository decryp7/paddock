//! C139 (an e2e spec that disables `paddock/e2e-ready-reads` says why, and the exceptions only shrink): the Rust host against its JavaScript
//! port, on a fixture tree and on this working tree. The check's own test file: C139 does not read it (its `IMPLEMENTATION`).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{has_node, node, repo_root, skip};
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;

/// The part of a report the JS host also writes.
fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

const D: &str = "// eslint-disable-next-line paddock/e2e-ready-reads -- immediate by design\n";

/// A tree with every shape C139 reads: a listed directive and one beyond its count, an unlisted one, a block, a bare one, one without a
/// description, another rule's directive, a file outside the e2e folders and a stale entry.
fn c139_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let files: Vec<(&str, String)> = vec![
        (
            "conformance/c139-allow.json",
            "{\"allow\":[{\"path\":\"ui/e2e/ok.spec.ts\",\"site\":\"paddock/e2e-ready-reads\",\"reason\":\"the read is immediate by design\"},{\"path\":\"ui/e2e/gone.spec.ts\",\"site\":\"paddock/e2e-ready-reads\",\"reason\":\"a stale entry\"}]}".to_string(),
        ),
        ("ui/e2e/ok.spec.ts", format!("{D}a();\n{D}b();\n")),
        ("ui/e2e/new.spec.ts", format!("{D}a();\n")),
        ("ui/e2e/block.spec.ts", "/* eslint-disable paddock/e2e-ready-reads */\n".to_string()),
        ("ui/e2e/bare.spec.ts", "/* eslint-disable */\n// eslint-disable-next-line\n".to_string()),
        ("ui/e2e/why.spec.ts", "// eslint-disable-next-line paddock/e2e-ready-reads\n// eslint-disable-next-line paddock/e2e-ready-reads -- ab\n// eslint-disable-line paddock/e2e-ready-reads -- \u{1F600}\n// eslint-disable-line paddock/e2e-ready-reads -- \u{1F600}a\n".to_string()),
        ("ui/e2e/other.spec.ts", "// eslint-disable-next-line no-console -- another rule\n// eslint-disable-next-line no-console, paddock/e2e-ready-reads -- two rules\r\n".to_string()),
        ("apps/dyno/app/e2e/crlf.spec.ts", "x(); // eslint-disable-line paddock/e2e-ready-reads -- crlf line\r\n".to_string()),
        ("ui/src/not-e2e.ts", D.to_string()),
        ("ui/e2e/x-snapshots/a.ts", D.to_string()),
    ];
    for (p, t) in files {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, t).unwrap();
    }
    d
}

const JS_RUN_C139: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C139'], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

fn c139_options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        only: Some(vec!["C139".into()]),
        config: paddock_dev::check::config::Config::bare(app),
        root: root.to_path_buf(),
        scope: None,
        files: None,
        today: "2026-11-18".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    }
}

#[test]
fn c139_flags_the_shapes_it_names_only_as_warnings() {
    let tree = c139_tree();
    let r = run_checks(&c139_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(String, usize)> = r.findings.iter().map(|f| (f.file.clone(), f.line)).collect();
    assert_eq!(
        got,
        [
            ("apps/dyno/app/e2e/crlf.spec.ts".to_string(), 1),
            ("ui/e2e/bare.spec.ts".to_string(), 1),
            ("ui/e2e/bare.spec.ts".to_string(), 2),
            ("ui/e2e/block.spec.ts".to_string(), 1),
            ("ui/e2e/new.spec.ts".to_string(), 1),
            ("ui/e2e/ok.spec.ts".to_string(), 3),
            ("ui/e2e/other.spec.ts".to_string(), 2),
            ("ui/e2e/why.spec.ts".to_string(), 1),
            ("ui/e2e/why.spec.ts".to_string(), 2),
            ("ui/e2e/why.spec.ts".to_string(), 3),
            ("ui/e2e/why.spec.ts".to_string(), 4),
            // the entry for a file that is gone is stale
            ("conformance/c139-allow.json".to_string(), 1),
        ]
    );
    assert!(r.findings.iter().any(|f| f.message.contains("beyond the count of its allow entry")));
    assert_eq!(r.errors(), 0, "report-only until its first green CI run (ER-41)");
    assert_eq!(r.warnings(), 12);
}

#[test]
fn c139_without_its_allow_list_cannot_look() {
    let tree = c139_tree();
    std::fs::remove_file(tree.path().join("conformance/c139-allow.json")).unwrap();
    let r = run_checks(&c139_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, bool, Option<&str>)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.unwaivable, f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(got, [("conformance/c139-allow.json", true, Some("check-broken"))]);
}

#[test]
fn live_c139_findings_equal_the_js_port_on_a_fixture_tree_and_on_this_tree() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = c139_tree();
    for (root, app) in [(tree.path().to_path_buf(), "fixture"), (repo_root(), "paddock")] {
        let js: Value = serde_json::from_str(&node(
            JS_RUN_C139,
            &[("RT5_ROOT", root.display().to_string()), ("RT5_APP", app.into())],
        ))
        .unwrap();
        let r = run_checks(&c139_options(&root, app), &registry()).unwrap();
        let (got, want) = (shared_part(&r.json()), shared_part(&js));
        assert_eq!(got["findings"], want["findings"], "C139 on {app}");
        assert_eq!(got, want, "C139 on {app}");
    }
}
