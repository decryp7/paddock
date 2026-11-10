//! `./dev check run` (RT-5): the Rust host and its first two checks against the JavaScript ones, the oracle.
//!
//! - `fixture_*`: the recorded answers of the JS runner on `tests/fixtures/check/cases` (`expected.json`, `masked.json`, made by
//!   `generate.mjs`); these need no node, so they outlive the JS host.
//! - `live_*`: the JS host itself as the oracle, on the fixtures and on this working tree (every tracked `.rs` file masked, and the
//!   findings of C39 and C53). When node is missing: a note on a workstation, a failure in CI on Linux (so a missing oracle cannot
//!   let the comparison pass unseen), a `::notice::` on the other CI legs (the Linux leg runs it).
//! - the host: `--only`, `--files`, run records, `not measured`, `check broken`, the report shape and the CLI.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::mask::mask_rust;
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest().ancestors().nth(2).unwrap().to_path_buf()
}

fn fixtures() -> PathBuf {
    manifest().join("tests/fixtures/check")
}

fn skip(reason: &str) {
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "true"));
    if ci && cfg!(target_os = "linux") {
        panic!("the live comparison cannot run in CI: {reason}");
    }
    if ci {
        println!("::notice::the live comparison against the conformance host was skipped on this host: {reason}. The Linux leg runs it.");
    } else {
        eprintln!("skipped: {reason}");
    }
}

fn has_node() -> bool {
    Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success())
}

/// The fixture cases materialized into a scratch tree (`a__b__c.rs.txt` is `a/b/c.rs`).
fn materialize() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for e in std::fs::read_dir(fixtures().join("cases")).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        let rel = name.trim_end_matches(".txt").replace("__", "/");
        let to = d.path().join(&rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures().join("cases").join(&name), to).unwrap();
    }
    d
}

fn options(root: &Path) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        app: "fixture".into(),
        only: Some(vec!["C39".into(), "C53".into()]),
        files: None,
        today: "2026-11-10".into(),
    }
}

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The part of a report the JS host also writes.
fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

#[test]
fn fixture_findings_equal_the_recorded_js_findings_exactly() {
    let tree = materialize();
    let r = run_checks(&options(tree.path()), &registry()).unwrap();
    let got = shared_part(&r.json());
    let want = shared_part(&read_json(&fixtures().join("expected.json")));
    // Same values and, as text, the same key order and finding order.
    assert_eq!(got, want);
    assert_eq!(
        serde_json::to_string_pretty(&got).unwrap(),
        serde_json::to_string_pretty(&want).unwrap()
    );
    assert!(
        r.errors() > 40 && r.waived.len() == 3,
        "the fixture must exercise many findings and the waivers"
    );
}

#[test]
fn fixture_masks_equal_the_recorded_js_masks() {
    let want = read_json(&fixtures().join("masked.json"));
    let tree = materialize();
    let mut n = 0;
    for (rel, masked) in want.as_object().unwrap() {
        let text = String::from_utf8_lossy(&std::fs::read(tree.path().join(rel)).unwrap()).into_owned();
        assert_eq!(mask_rust(&text), masked.as_str().unwrap(), "{rel}");
        n += 1;
    }
    assert!(n >= 14, "{n} cases masked");
}

/// Runs a node program (an ES module given on the command line) in the repository and returns its standard output.
fn node(driver: &str, env: &[(&str, String)]) -> String {
    let mut c = Command::new("node");
    c.args(["--input-type=module", "-e", driver]).current_dir(repo_root());
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().expect("node");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

const JS_RUN: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const now = process.env.RT5_NOW ? new Date(process.env.RT5_NOW) : undefined;
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C39', 'C53'], options: { now } });
process.stdout.write(JSON.stringify(r));
"#;

#[test]
fn live_fixture_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = materialize();
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[
            ("RT5_ROOT", tree.path().display().to_string()),
            ("RT5_APP", "fixture".into()),
            ("RT5_NOW", "2026-11-10T12:00:00Z".into()),
        ],
    ))
    .unwrap();
    let r = run_checks(&options(tree.path()), &registry()).unwrap();
    assert_eq!(shared_part(&r.json()), shared_part(&js));
}

#[test]
fn live_tree_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[("RT5_ROOT", root.display().to_string()), ("RT5_APP", "paddock".into())],
    ))
    .unwrap();
    let opts = RunOptions {
        app: "paddock".into(),
        today: paddock_dev::check::today_utc(),
        ..options(&root)
    };
    let r = run_checks(&opts, &registry()).unwrap();
    let (got, want) = (shared_part(&r.json()), shared_part(&js));
    assert_eq!(
        got["findings"], want["findings"],
        "findings of C39, C53 and the waiver engine on this tree"
    );
    assert_eq!(got["waived"], want["waived"]);
    assert_eq!(got, want);
    assert!(r.waived.len() > 5, "the tree's own waivers must be exercised");
}

#[test]
fn live_mask_equals_the_js_mask_on_every_tracked_rust_file() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let listed = Command::new("git")
        .args(["ls-files", "-z", "--", "*.rs"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(listed.status.success());
    let files: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    assert!(files.len() > 1000, "the golden needs every tracked .rs file, found {}", files.len());
    let out = tempfile::tempdir().unwrap();
    const DRIVER: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
import { readFileSync, writeFileSync } from 'node:fs';
const { maskRust } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/checks/C39-freed-port.mjs')).href);
const files = JSON.parse(readFileSync(process.env.RT5_LIST, 'utf8'));
files.forEach((f, i) => writeFileSync(join(process.env.RT5_OUT, `${i}.txt`), maskRust(readFileSync(f, 'utf8')), 'utf8'));
"#;
    let list = out.path().join("list.json");
    std::fs::write(&list, serde_json::to_string(&files).unwrap()).unwrap();
    node(
        DRIVER,
        &[
            ("RT5_LIST", list.display().to_string()),
            ("RT5_OUT", out.path().display().to_string()),
        ],
    );
    let mut differ = Vec::new();
    for (i, f) in files.iter().enumerate() {
        let text = String::from_utf8_lossy(&std::fs::read(root.join(f)).unwrap()).into_owned();
        let want = std::fs::read(out.path().join(format!("{i}.txt"))).unwrap();
        if mask_rust(&text).as_bytes() != want.as_slice() {
            differ.push(f.clone());
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} files are masked differently from the JS maskRust: {:?}",
        differ.len(),
        files.len(),
        &differ[..differ.len().min(5)]
    );
}

#[test]
fn only_selects_checks_and_an_unknown_id_is_refused() {
    let tree = materialize();
    let mut o = options(tree.path());
    o.only = Some(vec!["C53".into()]);
    let r = run_checks(&o, &registry()).unwrap();
    assert_eq!(r.checks, ["C53"]);
    assert!(r.findings.iter().all(|f| f.check != "C39"));
    o.only = Some(vec!["C39".into(), "C999".into()]);
    assert_eq!(run_checks(&o, &registry()).err().unwrap().0, ["C999"]);
}

#[test]
fn files_scopes_the_run_and_the_record_says_so() {
    let tree = materialize();
    let mut o = options(tree.path());
    o.files = Some(vec![
        "crates/a/src/c53_bad.rs".into(),
        "./crates/a/tests/c39_good.rs".into(),
        "crates/a/src/gone.rs".into(),
    ]);
    let r = run_checks(&o, &registry()).unwrap();
    let files: std::collections::BTreeSet<&str> = r
        .findings
        .iter()
        .filter(|f| f.check.starts_with('C'))
        .map(|f| f.file.as_str())
        .collect();
    assert_eq!(files, ["crates/a/src/c53_bad.rs"].into_iter().collect());
    let (c39, found39) = &r.runs[0];
    assert_eq!((c39.scope.name(), c39.probes, *found39), ("files", 2, false));
    assert!(c39.measured(), "the two .rs files in scope were read");
    // No waiver warning for waivers of files outside the scope.
    assert!(
        r.findings.iter().all(|f| f.check != "W003"),
        "{:?}",
        r.findings.iter().filter(|f| f.check == "W003").collect::<Vec<_>>()
    );
}

#[test]
fn a_scoped_run_that_reads_no_rust_file_is_not_measured_and_not_an_error() {
    let tree = materialize();
    let mut o = options(tree.path());
    o.files = Some(vec!["paddock.waivers.toml".into()]);
    let r = run_checks(&o, &registry()).unwrap();
    for (rec, found) in &r.runs {
        assert_eq!(rec.status(*found).name(), "not measured", "{}", rec.check);
    }
    assert!(
        r.findings.iter().all(|f| !f.check.starts_with('C')),
        "a push gate on a docs change must not fail"
    );
}

#[test]
fn a_whole_tree_run_that_looks_at_nothing_is_an_error_no_waiver_covers() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("readme.md"), "x").unwrap();
    std::fs::write(
        d.path().join("paddock.waivers.toml"),
        "[[waiver]]\ncheck = \"C39\"\npath = \"*\"\nreason = \"try\"\nexpires = \"2099-01-01\"\n",
    )
    .unwrap();
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    let broken: Vec<_> = r.findings.iter().filter(|f| f.unwaivable).collect();
    assert_eq!(broken.len(), 2, "{:?}", r.findings);
    assert!(broken[0].message.starts_with("not measured"));
    assert!(r.waived.is_empty());
    assert_eq!(r.runs[0].0.status(true).name(), "not measured");
}

#[test]
fn an_empty_tree_fails_the_precondition() {
    let d = tempfile::tempdir().unwrap();
    let r = run_checks(&options(d.path()), &registry()).unwrap();
    assert!(r.runs.iter().all(|(rec, _)| !rec.precondition.met));
    assert!(r.errors() >= 2);
}

#[test]
fn the_report_has_the_js_shape_and_the_run_records() {
    let tree = materialize();
    let v = run_checks(&options(tree.path()), &registry()).unwrap().json();
    for k in ["app", "checks", "findings", "waived", "errors", "warnings", "runs"] {
        assert!(v.get(k).is_some(), "{k}");
    }
    let run = &v["runs"][0];
    for k in [
        "check",
        "ran",
        "completed",
        "precondition",
        "probes",
        "scope",
        "os",
        "control",
        "status",
    ] {
        assert!(run.get(k).is_some(), "{k}");
    }
    assert_eq!(run["scope"], "root");
    assert_eq!(run["os"], std::env::consts::OS);
    assert!(run["probes"].as_u64().unwrap() >= 10);
}

fn dev(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_paddock-dev")).args(args).output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

#[test]
fn the_command_exits_5_on_findings_and_prints_one_json_object() {
    let tree = materialize();
    let root = tree.path().display().to_string();
    let (code, out, _) = dev(&["--json", "check", "run", "--root", &root, "--only", "c53", "--app", "fixture"]);
    assert_eq!(code, 5, "findings exit with 5");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["checks"], json!(["C53"]));
    let clean = tempfile::tempdir().unwrap();
    std::fs::write(clean.path().join("ok.rs"), "fn a() {}\n").unwrap();
    let (code, out, _) = dev(&[
        "check",
        "run",
        "--root",
        &clean.path().display().to_string(),
        "--files",
        "ok.rs",
        "--only",
        "C39",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("run C39: clean"), "{out}");
    let (code, _, err) = dev(&["check", "run", "--root", &root, "--only", "C1"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown check: C1"), "{err}");
}

#[test]
fn the_command_writes_the_report_to_out() {
    let tree = materialize();
    let out = tree.path().join("report.json");
    let (_, _, _) = dev(&[
        "check",
        "run",
        "--root",
        &tree.path().display().to_string(),
        "--out",
        &out.display().to_string(),
    ]);
    let v = read_json(&out);
    assert!(v["findings"].as_array().unwrap().len() > 40);
}
