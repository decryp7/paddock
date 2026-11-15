//! The error paths of every check that reads an allow list through the shared reader (`check::allow_list`,
//! `conformance/src/util/allow-list.mjs`), on both hosts: a broken allow file is one unwaivable check-broken finding and nothing else,
//! and a file the check cannot read is a not-measured error at that file. ER-21 class "the two hosts diverge on an error path" (C125,
//! 865498235; C121 and C124 on train 12): the outcome is owned by the reader, and this test runs for each check that names an allow list,
//! so a new user of the reader is covered without a new test.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{has_node, node, skip};
use paddock_dev::check::{Check, RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;

/// The checks that name an allow list: `(check, its allow file)`.
fn users() -> Vec<(Box<dyn Check>, &'static str)> {
    registry().into_iter().filter_map(|c| c.allow_list().map(|f| (c, f))).collect()
}

fn options(root: &Path, id: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("fixture"),
        scope: None,
        only: Some(vec![id.to_string()]),
        files: None,
        today: "2026-11-15".into(),
        base_ref: Some("origin/main".into()),
    }
}

const JS_RUN: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.RT5_ROOT, app: 'fixture', only: [process.env.RT5_ONLY], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

const JS_USERS: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { checks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/checks/index.mjs')).href);
process.stdout.write(JSON.stringify(checks.filter((c) => c.allowList).map((c) => [c.id, c.allowList])));
"#;

/// The check's control tree, with its allow file replaced by `allow` (left out when `None`).
fn tree(check: &dyn Check, file: &str, allow: Option<&str>) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let mut files: Vec<(&str, &str)> = check.control().files.into_iter().filter(|(p, _)| *p != file).collect();
    if let Some(text) = allow {
        files.push((file, text));
    }
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    d
}

/// The shared part of a report, the JSON parsers' own wording of a parse error cut off (serde_json and V8 word it differently).
fn shared(v: &Value) -> Value {
    let mut findings = v["findings"].clone();
    for f in findings.as_array_mut().unwrap() {
        let m = f["message"].as_str().unwrap().to_string();
        f["message"] = Value::from(m.split(" is not JSON: ").next().unwrap());
    }
    json!({"findings": findings, "errors": v["errors"], "warnings": v["warnings"], "waived": v["waived"]})
}

/// Both hosts over `root` for check `id`: the Rust report's shared part and the JavaScript one's (None without node).
fn both(root: &Path, id: &str) -> (Value, Option<Value>) {
    let r = run_checks(&options(root, id), &registry()).unwrap();
    let js = has_node().then(|| {
        let out = node(JS_RUN, &[("RT5_ROOT", root.display().to_string()), ("RT5_ONLY", id.to_string())]);
        shared(&serde_json::from_str::<Value>(&out).unwrap())
    });
    (shared(&r.json()), js)
}

fn file_and_outcome(v: &Value) -> Vec<(String, Option<String>)> {
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["file"].as_str().unwrap().to_string(), f["outcome"].as_str().map(str::to_string)))
        .collect()
}

#[test]
fn the_checks_that_name_an_allow_list_are_the_same_on_both_hosts() {
    let rust: Vec<(String, String)> = users().iter().map(|(c, f)| (c.id().to_string(), f.to_string())).collect();
    assert!(rust.len() >= 3, "C121, C124 and C125 read an allow list: {rust:?}");
    if !has_node() {
        return skip("node is not installed");
    }
    let js: Vec<(String, String)> = serde_json::from_str(&node(JS_USERS, &[])).unwrap();
    assert_eq!(rust, js);
}

/// "Delete the check": a missing allow file, one that is not JSON, one without its list, or one with a malformed entry is one
/// unwaivable check-broken finding at the allow file and nothing else, on both hosts, for every check that reads one.
#[test]
fn a_broken_allow_file_is_one_check_broken_finding_and_nothing_else_on_both_hosts() {
    for (check, file) in users() {
        for allow in [
            None,
            Some("nope"),
            Some("{}"),
            Some("{\"allow\": [{\"path\": \"a\"}], \"history\": []}"),
        ] {
            let t = tree(check.as_ref(), file, allow);
            let (rust, js) = both(t.path(), check.id());
            assert_eq!(
                file_and_outcome(&rust),
                [(file.to_string(), Some("check-broken".to_string()))],
                "{} with {allow:?}: {rust}",
                check.id()
            );
            if let Some(js) = js {
                assert_eq!(rust, js, "{} with {allow:?}", check.id());
            }
        }
    }
}

/// A file the check reads but cannot is a not-measured error at that file, on both hosts; the other sites are still reported and
/// the run is not called incomplete on top (root reads a 000 file: then nothing is checked).
#[test]
#[cfg(unix)]
fn an_unreadable_file_is_not_measured_at_that_file_on_both_hosts() {
    use std::os::unix::fs::PermissionsExt;
    for (check, file) in users() {
        let control = check.control();
        let t = tree(
            check.as_ref(),
            file,
            control.files.iter().find(|(p, _)| *p == file).map(|(_, text)| *text),
        );
        let (target, _) = control.expect[0];
        let locked = t.path().join(target);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&locked).is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
            return skip("running as root, which reads a 000 file");
        }
        let (rust, js) = both(t.path(), check.id());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        let got = file_and_outcome(&rust);
        assert!(
            got.contains(&(target.to_string(), Some("not-measured".to_string()))),
            "{}: the unreadable file is not measured: {rust}",
            check.id()
        );
        assert!(
            got.iter().all(|(f, _)| !f.is_empty()),
            "{}: one finding at the file, not a run-level 'did not complete' on top: {rust}",
            check.id()
        );
        if let Some(js) = js {
            assert_eq!(rust, js, "{}", check.id());
        }
    }
}
