//! C129 (no new shell): the Rust host against the conformance host as the oracle, on a fixture tree and on this working tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, has_node, node, node_ticking, repo_root, skip};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

const BASELINE: &str = "conformance/shell-baseline.json";
const EXCEPTIONS: &str = "conformance/shell-exceptions.json";

/// A new file of each kind, a baselined file that grew, one that shrank, an excepted path and pattern, a stale baseline entry and
/// exception, shebangs of an extensionless file (`env`, python, node, none), a binary, a caller naming a missing script (and one it
/// resolves from an app folder), a variable and a comment, and inline `run:` blocks.
fn cases() -> Vec<(&'static str, String)> {
    vec![
        (
            BASELINE,
            "{\"files\":[{\"path\":\"grew.sh\",\"lines\":2},{\"path\":\"shrank.sh\",\"lines\":9},{\"path\":\"gone.sh\",\"lines\":3},{\"path\":\"apps/d/scripts/in-app.sh\",\"lines\":1}]}".into(),
        ),
        (
            EXCEPTIONS,
            "{\"exceptions\":[{\"path\":\"dev\",\"id\":\"E1\",\"reason\":\"it must start first\",\"owner\":\"plan\"},{\"path\":\"vm/*.ps1\",\"id\":\"E5\",\"reason\":\"runs in the guest\",\"owner\":\"plan\"},{\"path\":\"gone/\",\"id\":\"E6\",\"reason\":\"names no file\",\"owner\":\"plan\"}]}".into(),
        ),
        ("dev", "#!/bin/sh\necho\n".into()),
        ("vm/a.ps1", "x\n".into()),
        ("vm/sub/b.ps1", "x\n".into()),
        ("new.sh", "a\nb\nc\n".into()),
        ("new.PY", "print(1)".into()),
        ("grew.sh", "1\n2\n3\n".into()),
        ("shrank.sh", "1\n".into()),
        ("bin/env-bash", "#!/usr/bin/env bash\necho\n".into()),
        ("bin/py", "#!/usr/bin/env -S python3 -I\n".into()),
        ("bin/node", "#!/usr/bin/env node\n".into()),
        ("bin/plain", "echo hi\n".into()),
        ("bin/blob", "#!/bin/sh\0\n".into()),
        ("é/ü.bat", "echo é\r\nexit\r\n".into()),
        ("apps/d/scripts/in-app.sh", "x\n".into()),
        ("scripts/keep.txt", "a top-level folder the callers' paths start in\n".into()),
        (
            ".gitea/workflows/x.yml",
            "jobs:\n  a:\n    steps:\n      - run: scripts/gone.sh\n      - run: bash ./new.sh\n      - run: scripts/in-app.sh\n      - run: scripts/$N.sh\n      # scripts/c.sh\n      - run: |\n          echo a\n\n          echo b\n      - name: n\n        run: >-\n          one\n".into(),
        ),
        ("apps/d/paddock.ci.json", "{\"prepare\": [\"scripts/in-app.sh\", \"scripts/nope.ps1\"]}".into()),
        ("lefthook.yml", "pre-push:\n  run: ./new.sh\n".into()),
        ("icons/a.png", "#!/bin/sh bytes".into()),
    ]
}

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C129".into()]),
        files: None,
        today: "2026-11-15".into(),
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
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C129'], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

fn tree_of<T: AsRef<str>>(files: &[(&str, T)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text.as_ref()).unwrap();
    }
    d
}

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
fn c129_flags_new_and_grown_shell_and_names_stale_entries_as_warnings() {
    let tree = tree_of(&cases());
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize, &str)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.line, f.message.split([':', '(']).next().unwrap().trim_end()))
        .collect();
    assert_eq!(
        got,
        [
            ("bin/env-bash", 1, "new shell file"),
            ("bin/py", 1, "new shell file"),
            ("grew.sh", 1, "shell file grew from 2 to 3 lines"),
            ("new.PY", 1, "new shell file"),
            ("new.sh", 1, "new shell file"),
            ("vm/sub/b.ps1", 1, "new shell file"),
            ("é/ü.bat", 1, "new shell file"),
            (BASELINE, 1, "stale baseline entry"),
            (EXCEPTIONS, 1, "stale exception"),
            (BASELINE, 1, "the baseline's shrink-only rule is not enforced here"),
            (
                ".gitea/workflows/x.yml",
                4,
                "names the script `scripts/gone.sh`, which the tree does not have"
            ),
            (
                "apps/d/paddock.ci.json",
                1,
                "names the script `scripts/nope.ps1`, which the tree does not have"
            ),
            (".gitea", 1, "inline shell in .gitea"),
        ]
    );
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
}

/// "Delete the check": a baseline or exception list that is missing, not JSON or incomplete makes the check broken, one finding and
/// nothing else, on both hosts, never a pass.
#[test]
fn c129_with_a_broken_list_is_one_check_broken_finding_on_both_hosts() {
    let good_b = "{\"files\":[]}";
    let good_e = "{\"exceptions\":[]}";
    for (which, text) in [
        (BASELINE, None),
        (BASELINE, Some("nope")),
        (BASELINE, Some("{}")),
        (BASELINE, Some("{\"files\":[{\"path\":\"a.sh\"}]}")),
        (EXCEPTIONS, None),
        (EXCEPTIONS, Some("nope")),
        (
            EXCEPTIONS,
            Some("{\"exceptions\":[{\"path\":\"a\",\"id\":\"E9\",\"reason\":\"long enough\",\"owner\":\"x\"}]}"),
        ),
        (
            EXCEPTIONS,
            Some("{\"exceptions\":[{\"path\":\"a\",\"id\":\"E1\",\"reason\":\"short\",\"owner\":\"x\"}]}"),
        ),
    ] {
        let mut files = vec![("a.sh", "x\n".to_string())];
        for (path, good) in [(BASELINE, good_b), (EXCEPTIONS, good_e)] {
            if path != which {
                files.push((path, good.to_string()));
            } else if let Some(t) = text {
                files.push((path, t.to_string()));
            }
        }
        let tree = tree_of(&files);
        let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
        let got: Vec<(&str, Option<&str>)> = r.findings.iter().map(|f| (f.file.as_str(), f.outcome.map(|o| o.name()))).collect();
        assert_eq!(got, [(which, Some("check-broken"))], "{which} {text:?}");
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
            assert_eq!(cut(rust), cut(js), "{which} {text:?}");
        }
    }
}

/// A file the run cannot read is not measured, at that file, on both hosts (root reads a 000 file: then nothing is checked).
#[test]
#[cfg(unix)]
fn c129_an_unreadable_file_is_not_measured_on_both_hosts() {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree_of(&[
        (
            BASELINE,
            "{\"files\":[{\"path\":\"scripts/a.sh\",\"lines\":1},{\"path\":\"scripts/b.sh\",\"lines\":1}]}".to_string(),
        ),
        (EXCEPTIONS, "{\"exceptions\":[]}".to_string()),
        ("scripts/a.sh", "x\n".to_string()),
        ("scripts/b.sh", "y\n".to_string()),
    ]);
    let locked = tree.path().join("scripts/a.sh");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&locked).is_ok() {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        eprintln!("not measured: running as root, which reads a 000 file");
        return;
    }
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let parity = has_node().then(|| both(tree.path()));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    let got: Vec<(&str, Option<&str>)> = r
        .findings
        .iter()
        .filter(|f| !f.message.contains("shrink-only rule is not enforced"))
        .map(|f| (f.file.as_str(), f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(got, [("scripts/a.sh", Some("not-measured"))]);
    if let Some((rust, js)) = parity {
        assert_eq!(rust, js);
    }
}

/// A run over other files (`--files`, the push gate) says nothing about baseline entries and exceptions whose files it did not look at.
#[test]
fn c129_a_files_run_judges_only_the_files_it_looked_at() {
    let tree = tree_of(&[
        (BASELINE, "{\"files\":[{\"path\":\"old.sh\",\"lines\":1}]}".to_string()),
        (
            EXCEPTIONS,
            "{\"exceptions\":[{\"path\":\"fx/\",\"id\":\"E6\",\"reason\":\"fixtures\",\"owner\":\"plan\"}]}".to_string(),
        ),
        ("old.sh", "x\n".to_string()),
        ("fx/a.sh", "x\n".to_string()),
        ("new.sh", "x\n".to_string()),
        ("a.txt", "x\n".to_string()),
    ]);
    let o = RunOptions {
        files: Some(vec!["new.sh".into(), "a.txt".into()]),
        ..options(tree.path(), "fixture")
    };
    let r = run_checks(&o, &registry()).unwrap();
    let got: Vec<(&str, &str)> = r
        .findings
        .iter()
        .filter(|f| !f.message.contains("shrink-only rule is not enforced"))
        .map(|f| (f.file.as_str(), f.message.split(" (").next().unwrap()))
        .collect();
    assert_eq!(got, [("new.sh", "new shell file")]);
}

#[test]
fn live_c129_fixture_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = tree_of(&cases());
    let (got, want) = both(tree.path());
    assert_eq!(got["findings"], want["findings"]);
    assert_eq!(got, want);
}

#[test]
fn live_c129_tree_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let js_root = root.clone();
    let js = paddock_testkit::guarded("the JS C129 over the live tree", LIVE_STALL, move |progress| {
        let driver = format!(
            "import fs from 'node:fs';\nimport {{ syncBuiltinESMExports }} from 'node:module';\nconst read = fs.readFileSync;\nfs.readFileSync = (...a) => {{ process.stdout.write('.\\n'); return read(...a); }};\nsyncBuiltinESMExports();\n{JS_RUN}"
        );
        let env = [("RT5_ROOT", js_root.display().to_string()), ("RT5_APP", "paddock".into())];
        serde_json::from_str::<Value>(node_ticking(&driver, &env, &progress).trim()).unwrap()
    });
    let r = paddock_testkit::guarded("the Rust C129 over the live tree", LIVE_STALL, {
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
