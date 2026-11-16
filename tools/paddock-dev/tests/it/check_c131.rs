//! C131 (a golden file holds no value of the machine that captured it): the Rust host against the conformance host as the oracle, on a fixture tree and on this
//! working tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, has_node, node, node_ticking, repo_root, skip};
use paddock_dev::check::c131::machine_facts;
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

/// The planted uid, the invented user an allow entry covers (twice, then a third beyond its count), a placeholder, a golden of
/// another kind, a binary golden, a stale entry, and (made from this machine's own facts at run time, never written down) the user,
/// the host, the home folder, the RAM and the CPU count of the machine that runs the test.
fn cases() -> Vec<(String, String)> {
    let env: HashMap<String, String> = std::env::vars().collect();
    let f = machine_facts(&env);
    let mut own = format!("user {}\nhost {}\nhome {}/x\n", f.user, f.host, f.home);
    own.push_str(&format!(
        "ram {:.0}\nram_kb {:.0}\ncpus: {:.0}\n",
        f.mem_bytes,
        (f.mem_bytes / 1024.0).floor(),
        f.cpus
    ));
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
        s("x/tests/golden/h.txt", &own),
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

/// This machine's facts as the Rust host reads them, handed to the JavaScript host (the `node` helper starts it with a bare
/// environment, so it would not see the user or home folder): both hosts then judge the same machine.
fn facts_json() -> String {
    let env: HashMap<String, String> = std::env::vars().collect();
    let f = machine_facts(&env);
    json!({"user": f.user, "home": f.home, "host": f.host, "memBytes": f.mem_bytes, "cpus": f.cpus}).to_string()
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
        &[
            ("RT5_ROOT", root.display().to_string()),
            ("RT5_APP", "fixture".into()),
            ("PADDOCK_GOLDEN_FACTS", facts_json()),
        ],
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
        let env = [
            ("RT5_ROOT", js_root.display().to_string()),
            ("RT5_APP", "paddock".into()),
            ("PADDOCK_GOLDEN_FACTS", facts_json()),
        ];
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
