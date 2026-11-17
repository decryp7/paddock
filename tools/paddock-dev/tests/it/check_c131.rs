//! C131 (a golden file holds no value of the machine that captured it): the Rust host against the conformance host as the oracle, on a fixture tree and on this
//! working tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, has_node, node, node_ticking, repo_root, skip};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

/// The planted uid, the invented user an allow entry covers (twice, then a third beyond its count), a placeholder, a golden of
/// another kind, a binary golden, a snapshot, CRLF and non-ASCII text, and a stale entry.
fn cases() -> Vec<(String, String)> {
    let s = |a: &str, b: &str| (a.to_string(), b.to_string());
    vec![
        s(
            "conformance/c131-allow.json",
            r#"{"allow": [{"path": "x/tests/golden/b.json", "site": "/Users/ana", "reason": "an invented user the test passes in", "count": 2},
{"path": "x/tests/golden/b.json", "site": "gone", "reason": "a stale entry here"}]}"#,
        ),
        s("x/tests/golden/a.json", "{\n  \"uid\": 501,\n  \"gid\": 20\n}\n"),
        s("x/tests/golden/b.json", "\"/Users/ana/a\"\n\"/Users/ana/b\"\n\"/Users/ana/c\"\n"),
        s("x/tests/golden/c.json", "{\"uid\": \"<uid>\", \"home\": \"/Users/<name>\"}\n"),
        s(
            "y/tests/goldens/d.txt",
            "uid=1000 /home/runner/x /var/folders/zz/T/a\r\nC:\\Users\\Bo\\x\nC:\\x\\Temp\\y\nclean\n",
        ),
        s("z/e.golden", "euid: 0\n"),
        s("x/tests/golden/f.png", "bytes uid 501"),
        s("x/tests/fixtures/g.json", "{\"uid\": 501}\n"),
        s("x/f.snap", "uid: 7\n"),
        s("x/tests/golden/h.txt", "é😀 uid=1\r\nclean\r\n/home/bï\r\n"),
        // astral characters count as one character, and ſ folds to s in a case-insensitive match, on both hosts
        s("x/tests/golden/i.txt", "uid😀😀😀😀=1\nuſer_id=5\nK uid: 3\n"),
    ]
}

fn fixture_tree() -> tempfile::TempDir {
    let owned = cases();
    let files: Vec<(&str, &str)> = owned.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    tree_of(&files)
}

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C131".into()]),
        files: None,
        today: "2026-11-16".into(),
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
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C131'], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

fn tree_of(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    d
}

/// Both hosts over `root`: the shared part of each report, the Rust one first.
fn both(root: &Path) -> (Value, Value) {
    let js: Value = serde_json::from_str(&node(
        JS_RUN,
        &[("RT5_ROOT", root.display().to_string()), ("RT5_APP", "fixture".into())],
    ))
    .unwrap();
    let r = run_checks(&options(root, "fixture"), &registry()).unwrap();
    (shared_part(&r.json()), shared_part(&js))
}

#[test]
fn c131_flags_the_planted_uid_and_the_other_machine_values_as_warnings() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    let has = |file: &str, line: usize, what: &str| {
        r.findings
            .iter()
            .any(|f| f.file == file && f.line == line && f.message.contains(what) && f.fix.contains("norm()"))
    };
    // ER-33: the planted uid and gid are seen.
    assert!(has("x/tests/golden/a.json", 2, "id `uid\": 501`"), "{got:?}");
    assert!(has("x/tests/golden/a.json", 3, "id `gid\": 20`"), "{got:?}");
    assert!(has("y/tests/goldens/d.txt", 1, "id `uid=1000`") && has("y/tests/goldens/d.txt", 1, "home `/home/runner`"));
    assert!(
        has("y/tests/goldens/d.txt", 1, "temp `/var/folders/`")
            && has("y/tests/goldens/d.txt", 2, "home `C:\\Users\\Bo`")
            && has("y/tests/goldens/d.txt", 3, "temp `\\Temp\\`")
    );
    assert!(has("z/e.golden", 1, "id `euid: 0`"));
    // the third /Users/ana is beyond its entry's count of two; the placeholder, the other folder and the binary file are not read
    assert!(has("x/tests/golden/b.json", 3, "beyond the count"), "{got:?}");
    assert!(
        got.iter()
            .all(|(f, _)| !f.contains("c.json") && !f.contains("fixtures") && !f.ends_with(".png")),
        "{got:?}"
    );
    assert!(
        r.findings
            .iter()
            .any(|f| f.message.starts_with("stale allow entry (x/tests/golden/b.json: gone)")),
        "{got:?}"
    );
    assert!(
        has("x/f.snap", 1, "id `uid: 7`")
            && has("x/tests/golden/h.txt", 1, "id `uid=1`")
            && has("x/tests/golden/h.txt", 3, "home `/home/b`")
    );
    // Exactly these 16, whoever runs it: the check reads no fact of the machine, so a Mac and the CI image as root agree
    // (`./dev golden confirm check_c131` runs this on both).
    assert!(
        has("x/tests/golden/i.txt", 1, "id") && has("x/tests/golden/i.txt", 2, "id") && has("x/tests/golden/i.txt", 3, "id"),
        "{got:?}"
    );
    assert_eq!(got.len(), 16, "{got:?}");
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
}

/// The positive control (ER-33): the host ran the check over its own planted tree and it saw the planted uid.
#[test]
fn c131_control_sees_a_planted_uid() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let record = &r.runs.iter().find(|x| x.record.check == "C131").expect("a run record").record;
    assert!(record.control, "the control found its planted uid");
}

/// "Delete the check": an allow file that is missing, not JSON or without its list makes the check broken, one finding and nothing
/// else, on both hosts, never a pass.
#[test]
fn c131_with_a_broken_allow_file_is_one_check_broken_finding_on_both_hosts() {
    for allow in [None, Some("nope"), Some("{}"), Some("{\"allow\": [{\"path\": \"a\"}]}")] {
        let mut files = vec![("x/tests/golden/a.json", "{\"uid\": 501}\n")];
        if let Some(text) = allow {
            files.push(("conformance/c131-allow.json", text));
        }
        let tree = tree_of(&files);
        let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
        let got: Vec<(&str, Option<&str>)> = r.findings.iter().map(|f| (f.file.as_str(), f.outcome.map(|o| o.name()))).collect();
        assert_eq!(got, [("conformance/c131-allow.json", Some("check-broken"))], "{allow:?}");
        if has_node() {
            // The JSON parsers word their own error differently (serde_json and V8): the message is compared up to that text.
            let cut = |mut v: Value| {
                for f in v["findings"].as_array_mut().unwrap() {
                    let m = f["message"].as_str().unwrap().to_string();
                    f["message"] = Value::from(m.split(" is not JSON: ").next().unwrap());
                }
                v
            };
            let (rust, js) = both(tree.path());
            assert_eq!(cut(rust), cut(js), "{allow:?}");
        }
    }
}

#[test]
fn live_c131_fixture_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = fixture_tree();
    let (got, want) = both(tree.path());
    assert_eq!(got["findings"], want["findings"]);
    assert_eq!(got, want);
}

#[test]
fn live_c131_tree_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let js_root = root.clone();
    let js = paddock_testkit::guarded("the JS C131 over the live tree", LIVE_STALL, move |progress| {
        let driver = format!(
            "import fs from 'node:fs';\nimport {{ syncBuiltinESMExports }} from 'node:module';\nconst read = fs.readFileSync;\nfs.readFileSync = (...a) => {{ process.stdout.write('.\\n'); return read(...a); }};\nsyncBuiltinESMExports();\n{JS_RUN}"
        );
        let env = [("RT5_ROOT", js_root.display().to_string()), ("RT5_APP", "paddock".into())];
        serde_json::from_str::<Value>(node_ticking(&driver, &env, &progress).trim()).unwrap()
    });
    let r = paddock_testkit::guarded("the Rust C131 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    let (got, want) = (shared_part(&r.json()), shared_part(&js));
    assert_eq!(got["findings"], want["findings"]);
    assert_eq!(got, want);
}

/// The shared parity corpus (a byte order mark, CRLF, astral text, case-folding letters, no-break spaces, long lines).
#[test]
fn c131_hosts_agree_on_the_shared_parity_corpus() {
    super::parity_corpus::assert_no_divergence("C131", &[("x/tests/golden/first.json", "uid=1\n")]);
}
