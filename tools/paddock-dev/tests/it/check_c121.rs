//! C121 (declared data is read through its accessor): the Rust host against its JavaScript port, on a fixture tree and on this
//! working tree. The check's own test file: C121 does not read it (its `IMPLEMENTATION`), so the invented lines below need no allow
//! entry.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{has_node, node, repo_root, skip};
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;

/// The part of a report the JS host also writes.
fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

/// A tree with every shape C121 reads: flagged and clean registries and tests, an allowed folder, a fixture folder, comments.
fn c121_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let files: &[(&str, &str)] = &[
        (
            "conformance/c121-allow.json",
            "{\"allow\":[{\"path\":\"conformance/test/x.test.mjs\",\"site\":\"a.match(/x/)\",\"reason\":\"tests of checks of workflows\"},{\"path\":\"scripts/ok.test.mjs\",\"site\":\"sed .gitea/workflows/ci.yml\",\"reason\":\"reads a workflow as its subject\",\"count\":2},{\"path\":\"scripts/gone.test.mjs\",\"site\":\"grep\",\"reason\":\"a stale entry\"}]}",
        ),
        (
            "docs/a.toml",
            "[[g]]\ncheck = [\"sh\", \"-c\", \"sed -n p .gitea/workflows/ci.yml\"]\n",
        ),
        (
            "docs/b.toml",
            "# grep .gitea/workflows/ci.yml in a comment\ncheck = [\"./dev\", \"ci\", \"conformance-args\"]\n",
        ),
        (
            "docs/c.toml",
            "check = [\"sh\", \"-c\", \"rg -n x workflows/ci.yml | awk 1\"]\nwhat = \"the sedate .gitea/workflows/ci.yml\"\n",
        ),
        (
            "tools/t/tests/it/a.rs",
            "fn a() {\n    let _ = Regex::new(\"x\"); // .gitea/workflows/ci.yml\n}\n",
        ),
        (
            "tools/t/tests/it/b.rs",
            "fn b() {\n    let _ = read(\".gitea/workflows/ci.yml\");\n}\n",
        ),
        (
            "tools/t/src/tests.rs",
            "fn c() { Command::new(\"grep\").arg(\".gitea/workflows/x.yml\"); }\n",
        ),
        (
            "conformance/test/x.test.mjs",
            "const t = a.match(/x/); // .gitea/workflows/ci.yml\n",
        ),
        // two identical sites, one entry with `count: 2`; a third, other site in the same file is still reported
        (
            "scripts/ok.test.mjs",
            "sed .gitea/workflows/ci.yml\nsed .gitea/workflows/ci.yml\ngrep x .gitea/workflows/ci.yml\n",
        ),
        (
            "scripts/bad.test.mjs",
            "const r = new RegExp(\"x\").test(readFileSync('.gitea/workflows/ci.yml'));\nconst m = t.matchAll(/x/g); // workflows/ci.yml\n",
        ),
        (
            "tools/t/tests/fixtures/f.rs",
            "Command::new(\"grep\"); // .gitea/workflows/ci.yml\n",
        ),
        ("a/b_test.rs", "perl -ne print .gitea/workflows/ci.yml\r\nx\r\n"),
    ];
    for (p, t) in files {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, t).unwrap();
    }
    d
}

const JS_RUN_C121: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C121'], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

fn c121_options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        only: Some(vec!["C121".into()]),
        config: paddock_dev::check::config::Config::bare(app),
        root: root.to_path_buf(),
        scope: None,
        files: None,
        today: "2026-11-10".into(),
        base_ref: Some("origin/main".into()),
    }
}

#[test]
fn c121_flags_the_shapes_it_names_and_only_as_warnings() {
    let tree = c121_tree();
    let r = run_checks(&c121_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(String, usize)> = r.findings.iter().map(|f| (f.file.clone(), f.line)).collect();
    assert_eq!(
        got,
        [
            ("a/b_test.rs".to_string(), 1),
            ("docs/a.toml".to_string(), 2),
            ("docs/c.toml".to_string(), 1),
            ("scripts/bad.test.mjs".to_string(), 1),
            ("scripts/bad.test.mjs".to_string(), 2),
            ("scripts/ok.test.mjs".to_string(), 3),
            ("tools/t/src/tests.rs".to_string(), 1),
            ("tools/t/tests/it/a.rs".to_string(), 2),
            // the entry for a file that is gone is stale
            ("conformance/c121-allow.json".to_string(), 1),
        ]
    );
    assert!(r.findings.iter().any(|f| {
        f.message
            .contains("stale allow entry (scripts/gone.test.mjs: grep) matches no site")
    }));
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
    assert_eq!(r.warnings(), 9);
}

#[test]
fn c121_without_its_allow_list_cannot_look() {
    let tree = c121_tree();
    std::fs::remove_file(tree.path().join("conformance/c121-allow.json")).unwrap();
    let r = run_checks(&c121_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, bool, Option<&str>)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.unwaivable, f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(
        got,
        [("conformance/c121-allow.json", true, Some("check-broken"))],
        "a check that could not read its allow list is broken, one finding no waiver covers and nothing else"
    );
}

#[test]
fn live_c121_findings_equal_the_js_port_on_a_fixture_tree_and_on_this_tree() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = c121_tree();
    for (root, app) in [(tree.path().to_path_buf(), "fixture"), (repo_root(), "paddock")] {
        let js: Value = serde_json::from_str(&node(
            JS_RUN_C121,
            &[("RT5_ROOT", root.display().to_string()), ("RT5_APP", app.into())],
        ))
        .unwrap();
        let r = run_checks(&c121_options(&root, app), &registry()).unwrap();
        let (got, want) = (shared_part(&r.json()), shared_part(&js));
        assert_eq!(got["findings"], want["findings"], "C121 on {app}");
        assert_eq!(got, want, "C121 on {app}");
    }
}

/// The check's own files are not read: their invented lines need no allow entry; another test file still is.
#[test]
fn c121_does_not_read_its_own_files() {
    let d = tempfile::tempdir().unwrap();
    let line = "const t = text.match(/x/); // .gitea/workflows/ci.yml\n";
    let mut files: Vec<&str> = paddock_dev::check::c121::IMPLEMENTATION.to_vec();
    assert!(files.contains(&"tools/paddock-dev/tests/it/check_c121.rs") && files.contains(&"conformance/test/c121-declared-data.test.mjs"));
    files.push("tools/paddock-dev/tests/it/check.rs");
    for p in files {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, line).unwrap();
    }
    std::fs::write(d.path().join("conformance/c121-allow.json"), "{\"allow\": []}").unwrap();
    let r = run_checks(&c121_options(d.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, [("tools/paddock-dev/tests/it/check.rs", 1)], "another test file is still read");
}
