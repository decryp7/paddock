//! C125 (no hard-coded machine disk paths): the Rust host against the conformance host as the oracle, on a fixture tree and on this
//! working tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, has_node, node, node_ticking, repo_root, skip};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

/// A stale guard default, an allowed invented volume next to an unlisted one, a longer name and a literal beyond the count, skipped
/// Markdown history beside a script that is read, a binary file, a worktree's `.git` file, placeholders, non-ASCII text before a
/// site, and stale allow and history entries.
const CASES: &[(&str, &str)] = &[
    (
        "conformance/c125-allow.json",
        r#"{"history": [{"path": "docs/adr/", "reason": "ADRs record history"}, {"path": "docs/gone/", "reason": "names no file"},
{"path": "docs/quiet/", "reason": "names no disk"}],
"allow": [{"path": "apps/d/src/vol.rs", "site": "/Volumes/Data", "reason": "invented test data", "count": 2},
{"path": "apps/d/src/vol.rs", "site": "/Volumes/Gone", "reason": "a stale entry"},
{"path": "scripts/x.sh", "site": "/Volumes/USB", "reason": "one site of two"},
{"path": "scripts/y.sh", "site": "/Volumes/M", "reason": "not a whole component", "count": 3.0}]}"#,
    ),
    (
        ".claude/hooks/worktree-guard.py",
        "# é😀\nMAIN = os.environ.get(\"M\", \"/Volumes/d8sst7/dev.decryptology.net/paddock\")\nx = '/Volumes/<disk>/a /Volumes/... /System/Volumes/Data'\n",
    ),
    (
        "apps/d/src/vol.rs",
        "let a = \"/Volumes/Data\";\nlet b = [\"/Volumes/Data/x\", \"/Volumes/d7sst7\"];\nlet c = \"/Volumes/Database\";\nlet d = \"/Volumes/Data\";\n",
    ),
    ("scripts/x.sh", "A=/Volumes/USB\nB=/Volumes/USB/b\n"),
    ("scripts/y.sh", "A=/Volumes/Media\n"),
    ("docs/adr/0001-x.md", "built in /Volumes/d7sst7/dev-targets/x\n"),
    ("docs/adr/run.sh", "cd /Volumes/d7sst7/x\n"),
    ("docs/quiet/a.md", "no disk is named here\n"),
    ("icons/a.png", "bytes /Volumes/d7sst7"),
    (".git", "gitdir: /Volumes/d7sst7/paddock/.git/worktrees/x\n"),
];

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C125".into()]),
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
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C125'], options: {} });
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

fn fixture_tree() -> tempfile::TempDir {
    tree_of(CASES)
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
fn c125_flags_unlisted_disk_paths_and_names_stale_entries_as_warnings() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize, &str)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.line, f.message.split(':').next().unwrap()))
        .collect();
    let path = "hard-codes the machine disk path";
    assert_eq!(
        got,
        [
            (
                ".claude/hooks/worktree-guard.py",
                2,
                &*format!("{path} `/Volumes/d8sst7/dev.decryptology.net/paddock`")
            ),
            // an allowed name hides neither another literal, nor a longer name, nor a literal beyond its count
            ("apps/d/src/vol.rs", 2, &format!("{path} `/Volumes/d7sst7`")),
            ("apps/d/src/vol.rs", 3, &format!("{path} `/Volumes/Database`")),
            (
                "apps/d/src/vol.rs",
                4,
                &format!("{path} `/Volumes/Data` beyond the count of its allow entry")
            ),
            // history covers Markdown only
            ("docs/adr/run.sh", 1, &format!("{path} `/Volumes/d7sst7/x`")),
            (
                "scripts/x.sh",
                2,
                &format!("{path} `/Volumes/USB/b` beyond the count of its allow entry")
            ),
            ("scripts/y.sh", 1, &format!("{path} `/Volumes/Media`")),
            (
                "conformance/c125-allow.json",
                1,
                "stale history entry (docs/gone/) skipped no disk path"
            ),
            (
                "conformance/c125-allow.json",
                1,
                "stale history entry (docs/quiet/) skipped no disk path"
            ),
            ("conformance/c125-allow.json", 1, "stale allow entry (apps/d/src/vol.rs"),
            ("conformance/c125-allow.json", 1, "stale allow entry (scripts/y.sh"),
        ]
    );
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
}

/// "Delete the check": an allow file that is missing, not JSON or without its lists makes the check broken, one finding and
/// nothing else, on both hosts, never a pass.
#[test]
fn c125_with_a_broken_allow_file_is_one_check_broken_finding_on_both_hosts() {
    for allow in [
        None,
        Some("nope"),
        Some("{\"allow\": []}"),
        Some("{\"history\": [], \"allow\": [{\"path\": \"a\"}]}"),
    ] {
        let mut files = vec![("scripts/a.sh", "x=/Volumes/d7sst7\n")];
        if let Some(text) = allow {
            files.push(("conformance/c125-allow.json", text));
        }
        let tree = tree_of(&files);
        let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
        let got: Vec<(&str, Option<&str>)> = r.findings.iter().map(|f| (f.file.as_str(), f.outcome.map(|o| o.name()))).collect();
        assert_eq!(got, [("conformance/c125-allow.json", Some("check-broken"))], "{allow:?}");
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

/// A file the run cannot read is not measured, at that file, on both hosts (root reads a 000 file: then nothing is checked).
#[test]
#[cfg(unix)]
fn c125_an_unreadable_file_is_not_measured_on_both_hosts() {
    use std::os::unix::fs::PermissionsExt;
    let tree = tree_of(&[
        ("conformance/c125-allow.json", "{\"history\": [], \"allow\": []}"),
        ("scripts/a.sh", "x=/Volumes/d7sst7\n"),
        ("scripts/b.sh", "y=/Volumes/d8sst7\n"),
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
    let got: Vec<(&str, Option<&str>)> = r.findings.iter().map(|f| (f.file.as_str(), f.outcome.map(|o| o.name()))).collect();
    assert_eq!(got, [("scripts/a.sh", Some("not-measured")), ("scripts/b.sh", None)]);
    if let Some((rust, js)) = parity {
        assert_eq!(rust, js);
    }
}

#[test]
fn live_c125_fixture_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = fixture_tree();
    let (got, want) = both(tree.path());
    assert_eq!(got["findings"], want["findings"]);
    assert_eq!(got, want);
}

#[test]
fn live_c125_tree_findings_equal_the_js_host() {
    if !has_node() {
        return skip("node is not installed");
    }
    let root = repo_root();
    let js_root = root.clone();
    let js = paddock_testkit::guarded("the JS C125 over the live tree", LIVE_STALL, move |progress| {
        let driver = format!(
            "import fs from 'node:fs';\nimport {{ syncBuiltinESMExports }} from 'node:module';\nconst read = fs.readFileSync;\nfs.readFileSync = (...a) => {{ process.stdout.write('.\\n'); return read(...a); }};\nsyncBuiltinESMExports();\n{JS_RUN}"
        );
        let env = [("RT5_ROOT", js_root.display().to_string()), ("RT5_APP", "paddock".into())];
        serde_json::from_str::<Value>(node_ticking(&driver, &env, &progress).trim()).unwrap()
    });
    let r = paddock_testkit::guarded("the Rust C125 over the live tree", LIVE_STALL, {
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

/// A file the check reads and then passes over (a NUL byte) is not read: an entry for it says so, and it is no probe.
#[test]
fn c125_an_entry_for_a_file_it_passes_over_names_a_file_it_does_not_read() {
    let tree = tree_of(&[
        (
            "conformance/c125-allow.json",
            "{\"history\": [], \"allow\": [{\"path\": \"scripts/blob.sh\", \"site\": \"/Volumes/Data\", \"reason\": \"invented test data\"}]}",
        ),
        ("scripts/blob.sh", "x=/Volumes/Data\0\n"),
        // one text file, so the run looked at something
        ("scripts/a.sh", "echo hi\n"),
    ]);
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, &str)> = r.findings.iter().map(|f| (f.file.as_str(), f.message.as_str())).collect();
    assert_eq!(
        got,
        [(
            "conformance/c125-allow.json",
            "allow entry (scripts/blob.sh: /Volumes/Data) names a file this check does not read"
        )]
    );
    if has_node() {
        let (rust, js) = both(tree.path());
        assert_eq!(rust, js);
    }
}

/// A run over other files (`--files`, the push gate) says nothing about a history entry whose files it did not look at.
#[test]
fn c125_a_history_entry_outside_a_files_run_is_not_stale() {
    let tree = tree_of(&[
        (
            "conformance/c125-allow.json",
            "{\"history\": [{\"path\": \"docs/adr/\", \"reason\": \"ADRs record history\"}], \"allow\": []}",
        ),
        ("docs/adr/0001.md", "built in /Volumes/d7sst7/x\n"),
        ("scripts/a.sh", "echo hi\n"),
    ]);
    let o = RunOptions {
        files: Some(vec!["scripts/a.sh".into(), "conformance/c125-allow.json".into()]),
        ..options(tree.path(), "fixture")
    };
    let r = run_checks(&o, &registry()).unwrap();
    assert!(r.findings.is_empty(), "{:?}", r.findings);
}
