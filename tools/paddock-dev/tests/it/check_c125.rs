//! C125 (no hard-coded machine disk paths): the Rust host against the conformance host as the oracle, on a fixture tree and on this
//! working tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, has_node, node, node_ticking, repo_root, skip};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

/// A stale guard default, an allowed invented volume next to an unlisted one, a skipped history document, a binary file, a
/// worktree's `.git` file, placeholders, non-ASCII text before a site, and stale, short and ambiguous allow entries.
const CASES: [(&str, &str); 8] = [
    (
        "conformance/c125-allow.json",
        r#"{"history": [{"path": "docs/adr/", "reason": "ADRs record history"}, {"path": "docs/gone/", "reason": "names no file"}],
"allow": [{"path": "apps/d/src/vol.rs", "site": "/Volumes/Data", "reason": "invented test data", "count": 2},
{"path": "apps/d/src/vol.rs", "site": "/Volumes/Gone", "reason": "a stale entry"},
{"path": "scripts/x.sh", "site": "/Volumes/USB", "reason": "an ambiguous entry"},
{"path": "scripts/y.sh", "site": "/Volumes/M", "reason": "a short count", "count": 3}]}"#,
    ),
    (
        ".claude/hooks/worktree-guard.py",
        "# é😀\nMAIN = os.environ.get(\"M\", \"/Volumes/d8sst7/dev.decryptology.net/paddock\")\nx = '/Volumes/<disk>/a /Volumes/... /System/Volumes/Data'\n",
    ),
    (
        "apps/d/src/vol.rs",
        "let a = \"/Volumes/Data\";\nlet b = [\"/Volumes/Data/x\", \"/Volumes/d7sst7\"];\n",
    ),
    ("scripts/x.sh", "A=/Volumes/USB\nB=/Volumes/USB/b\n"),
    ("scripts/y.sh", "A=/Volumes/Media\n"),
    ("docs/adr/0001-x.md", "built in /Volumes/d7sst7/dev-targets/x\n"),
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
fn c125_flags_unlisted_disk_paths_and_names_stale_and_ambiguous_entries_as_warnings() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize, &str)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.line, f.message.split(':').next().unwrap()))
        .collect();
    assert_eq!(
        got,
        [
            (
                ".claude/hooks/worktree-guard.py",
                2,
                "hard-codes the machine disk path `/Volumes/d8sst7/dev.decryptology.net/paddock`"
            ),
            // an allowed name does not hide another literal in the same file
            ("apps/d/src/vol.rs", 2, "hard-codes the machine disk path `/Volumes/d7sst7`"),
            ("conformance/c125-allow.json", 1, "stale history entry (docs/gone/) names no file"),
            ("conformance/c125-allow.json", 1, "stale allow entry (apps/d/src/vol.rs"),
            ("conformance/c125-allow.json", 1, "ambiguous allow entry (scripts/x.sh"),
            ("conformance/c125-allow.json", 1, "stale allow entry (scripts/y.sh"),
        ]
    );
    let said = |needle: &str| r.findings.iter().any(|f| f.message.contains(needle));
    assert!(
        said("(scripts/y.sh: /Volumes/M) matches only 1 of its 3 sites"),
        "a count above the matches is stale"
    );
    assert!(
        said("(scripts/x.sh: /Volumes/USB) matches 2 sites"),
        "two matches of a count of 1 are ambiguous"
    );
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
}

/// "Delete the check": with its allow file gone the check cannot look, and that is an error no waiver covers, never a pass.
#[test]
fn c125_without_its_allow_file_cannot_look() {
    let tree = fixture_tree();
    std::fs::remove_file(tree.path().join("conformance/c125-allow.json")).unwrap();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    assert!(
        r.errors() > 0,
        "a check that could not read its allow file is an error no waiver covers"
    );
}

#[test]
fn live_c125_fixture_findings_equal_the_js_host() {
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
    let (got, want) = (shared_part(&r.json()), shared_part(&js));
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
