//! C139 (the exceptions to `paddock/e2e-ready-reads`, an `eslint-suppressions.json`, only shrink): the Rust host against its JavaScript port, on
//! fixture trees (with and without git history) and on this working tree. The check's own test file: nothing in it is read as a spec.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{has_node, node, repo_root, skip};
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

/// The part of a report the JS host also writes.
fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

const R: &str = "paddock/e2e-ready-reads";

fn supp(n: u32) -> String {
    format!("{{\"e2e/a.spec.ts\":{{\"{R}\":{{\"count\":{n}}}}}}}")
}

fn write(root: &Path, files: &[(&str, String)]) {
    for (p, t) in files {
        let to = root.join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, t).unwrap();
    }
}

#[allow(clippy::disallowed_methods, reason = "a test spawns git; it ends it itself")]
fn git(root: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(st.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&st.stderr));
}

/// A tree of every shape: two good lists, a malformed one, one outside the two places, one with a spec snapshot (not a spec).
fn shapes_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        &[
            ("ui/e2e/a.spec.ts", "const a = 1;\n".to_string()),
            ("ui/e2e/x-snapshots/a.ts", "const a = 1;\n".to_string()),
            ("ui/eslint-suppressions.json", supp(2)),
            ("apps/dyno/app/eslint-suppressions.json", supp(1)),
            (
                "apps/debrief/app/eslint-suppressions.json",
                format!("{{\"e2e/a.spec.ts\":{{\"{R}\":{{\"count\":0}}}}}}"),
            ),
            ("apps/pitwall/app/eslint-suppressions.json", "nope".to_string()),
            ("ui/src/eslint-suppressions.json", "nope".to_string()),
        ],
    );
    d
}

/// A repository whose merge base (`main`) has the ui list `base` and whose `topic` commit has `now`.
fn growth_tree(base: u32, now: u32) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        &[
            ("ui/e2e/a.spec.ts", "const a = 1;\n".to_string()),
            ("ui/eslint-suppressions.json", supp(base)),
        ],
    );
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "base"]);
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    write(d.path(), &[("ui/eslint-suppressions.json", supp(now))]);
    git(d.path(), &["commit", "-q", "-a", "--allow-empty", "-m", "topic"]);
    d
}

/// A repository whose merge base (`main`) has no suppressions file and whose `topic` commit adds `apps/pitwall/app/eslint-suppressions.json` with count 5.
fn new_list_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), &[("ui/e2e/a.spec.ts", "const a = 1;\n".to_string())]);
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "base: no list"]);
    git(d.path(), &["checkout", "-q", "-b", "topic"]);
    write(d.path(), &[("apps/pitwall/app/eslint-suppressions.json", supp(5))]);
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "a new exception list"]);
    d
}

const JS_RUN_C139: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const options = process.env.RT5_BASE ? { baseRef: process.env.RT5_BASE } : {};
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C139'], options });
process.stdout.write(JSON.stringify(r));
"#;

fn c139_options(root: &Path, app: &str, base: Option<&str>) -> RunOptions {
    RunOptions {
        only: Some(vec!["C139".into()]),
        config: paddock_dev::check::config::Config::bare(app),
        root: root.to_path_buf(),
        scope: None,
        files: None,
        today: "2026-11-18".into(),
        base_ref: Some(base.unwrap_or("origin/main").into()),
        ci: Some(false),
    }
}

#[test]
fn c139_reads_the_two_places_and_calls_a_malformed_list_check_broken() {
    let tree = shapes_tree();
    let r = run_checks(&c139_options(tree.path(), "fixture", None), &registry()).unwrap();
    let got: Vec<(&str, bool, Option<&str>)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.unwaivable, f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(
        got,
        [
            ("apps/debrief/app/eslint-suppressions.json", true, Some("check-broken")),
            ("apps/pitwall/app/eslint-suppressions.json", true, Some("check-broken")),
        ]
    );
    assert_eq!(r.errors(), 2, "blocking since ci.yml 8869 (ER-41): a list the check cannot read is an error");
}

#[test]
fn c139_a_run_that_reads_no_e2e_spec_is_not_measured() {
    let d = tempfile::tempdir().unwrap();
    write(
        d.path(),
        &[
            ("ui/eslint-suppressions.json", supp(1)),
            ("ui/e2e/x-snapshots/a.ts", "x\n".to_string()),
            ("ui/src/a.ts", "x\n".to_string()),
        ],
    );
    let r = run_checks(&c139_options(d.path(), "fixture", None), &registry()).unwrap();
    let got: Vec<(&str, bool, Option<&str>)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.unwaivable, f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(got, [("ui/e2e", true, Some("not-measured"))]);
}

#[test]
fn c139_a_raised_count_is_growth_and_a_lowered_one_is_not() {
    let grown = growth_tree(1, 2);
    let r = run_checks(&c139_options(grown.path(), "fixture", Some("main")), &registry()).unwrap();
    assert_eq!(r.findings.len(), 1);
    assert!(
        r.findings[0].unwaivable && r.findings[0].message.contains("the exception list grew: 1 entry is"),
        "{:?}",
        r.findings[0].message
    );
    let shrunk = growth_tree(2, 1);
    let r = run_checks(&c139_options(shrunk.path(), "fixture", Some("main")), &registry()).unwrap();
    assert!(r.findings.is_empty());
}

#[test]
fn c139_a_new_exception_list_is_growth_however_many_entries_it_has() {
    let d = new_list_tree();
    let r = run_checks(&c139_options(d.path(), "fixture", Some("main")), &registry()).unwrap();
    assert_eq!(
        r.findings.len(),
        1,
        "{:?}",
        r.findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
    assert_eq!(r.findings[0].file, "apps/pitwall/app/eslint-suppressions.json");
    assert!(
        r.findings[0].unwaivable && r.findings[0].message.contains("grew: 5 entries are"),
        "{}",
        r.findings[0].message
    );
}

#[test]
fn live_c139_findings_equal_the_js_port_on_fixture_trees_and_on_this_tree() {
    if !has_node() {
        return skip("node is not installed");
    }
    let shapes = shapes_tree();
    let grown = growth_tree(1, 3);
    let fresh = new_list_tree();
    let cases: Vec<(std::path::PathBuf, &str, Option<&str>)> = vec![
        (shapes.path().to_path_buf(), "fixture", None),
        (grown.path().to_path_buf(), "fixture", Some("main")),
        (fresh.path().to_path_buf(), "fixture", Some("main")),
        (repo_root(), "paddock", None),
    ];
    for (root, app, base) in cases {
        let mut env = vec![("RT5_ROOT", root.display().to_string()), ("RT5_APP", app.to_string())];
        if let Some(b) = base {
            env.push(("RT5_BASE", b.to_string()));
        }
        let js: Value = serde_json::from_str(&node(JS_RUN_C139, &env)).unwrap();
        let r = run_checks(&c139_options(&root, app, base), &registry()).unwrap();
        let (got, want) = (shared_part(&r.json()), shared_part(&js));
        assert_eq!(got["findings"], want["findings"], "C139 on {app} {base:?}");
        assert_eq!(got, want, "C139 on {app} {base:?}");
    }
}
