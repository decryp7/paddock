//! C130 (Rust tests install the testkit stand-in, not a `#!/bin/sh` fixture): the Rust host against its JavaScript port, on a fixture
//! tree and on this working tree. The check's own test file: C130 does not read it (its `IMPLEMENTATION`), so the invented lines below
//! need no allow entry.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{has_node, node, repo_root, skip};
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::Path;

/// The part of a report the JS host also writes.
fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

const SH: &str = "std::fs::write(&p, \"#!/bin/sh\\nexit 3\\n\").unwrap();\n";

/// A tree with every shape C130 reads: an allowed site, one beyond its count, an unlisted one, a stale entry, every cfg(test) form,
/// production shell text, comments, waivers, a fixture folder and each shebang spelling.
fn c130_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let files: Vec<(&str, String)> = vec![
        (
            "conformance/c130-allow.json",
            "{\"allow\":[{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"baseline: planted before StandIn\",\"count\":2},{\"path\":\"tools/t/tests/it/gone.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"a stale entry\"}]}".to_string(),
        ),
        ("tools/t/tests/it/ok.rs", format!("{SH}{SH}{SH}")),
        ("tools/t/tests/it/bad.rs", format!("fn b() {{}}\n{SH}")),
        ("tools/t/src/lib.rs", "const SHIM: &str = \"#!/bin/sh\\n\";\n".to_string()),
        ("tools/t/src/a.rs", format!("const SHIM: &str = \"#!/bin/sh\\n\";\n#[cfg(test)]\nmod tests {{\n{SH}}}\n")),
        ("tools/t/src/b.rs", format!("#[cfg(all(test, unix))]\nmod t {{\n{SH}}}\n")),
        ("tools/t/src/c.rs", format!("#[cfg(any(test, windows))]\nmod t {{\n{SH}}}\n")),
        ("tools/t/src/tests.rs", "let a = \"#!/usr/bin/env bash\\n\";\nlet b = \"#!/bin/zsh\\n\";\nlet c = \"#!/usr/bin/env node\\n\";\n".to_string()),
        ("tools/t/src/d_test.rs", "// writes #!/bin/sh in the old test\n/// doc #!/bin/sh\nlet w = \"#!/bin/sh\\n\"; // conformance: stand-in — the hook under test is a script\n// conformance: stand-in — a script by design\nlet v = \"#!/bin/sh\\n\";\n\nlet u = \"#!/bin/sh\\n\";\n".to_string()),
        ("tools/t/tests/fixtures/f.rs", SH.to_string()),
        ("tools/t/tests/it/script.sh", "#!/bin/sh\necho hi\n".to_string()),
        ("a/tests/crlf.rs", "let a = \"#!/bin/sh\\n\";\r\nlet b = 1;\r\n".to_string()),
    ];
    for (p, t) in files {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, t).unwrap();
    }
    d
}

const JS_RUN_C130: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.RT5_ROOT, app: process.env.RT5_APP, only: ['C130'], options: {} });
process.stdout.write(JSON.stringify(r));
"#;

fn c130_options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        only: Some(vec!["C130".into()]),
        config: paddock_dev::check::config::Config::bare(app),
        root: root.to_path_buf(),
        scope: None,
        files: None,
        today: "2026-11-16".into(),
        base_ref: Some("origin/main".into()),
    }
}

#[test]
fn c130_flags_the_shapes_it_names_and_only_as_warnings() {
    let tree = c130_tree();
    let r = run_checks(&c130_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(String, usize)> = r.findings.iter().map(|f| (f.file.clone(), f.line)).collect();
    assert_eq!(
        got,
        [
            ("a/tests/crlf.rs".to_string(), 1),
            ("tools/t/src/a.rs".to_string(), 4),
            ("tools/t/src/b.rs".to_string(), 3),
            ("tools/t/src/c.rs".to_string(), 3),
            ("tools/t/src/d_test.rs".to_string(), 7),
            ("tools/t/src/tests.rs".to_string(), 1),
            ("tools/t/src/tests.rs".to_string(), 2),
            ("tools/t/tests/it/bad.rs".to_string(), 2),
            ("tools/t/tests/it/ok.rs".to_string(), 3),
            // the entry for a file that is gone is stale
            ("conformance/c130-allow.json".to_string(), 1),
        ]
    );
    assert!(r.findings.iter().any(|f| f.message.contains("beyond the count of its allow entry")));
    assert_eq!(r.errors(), 0, "report-only until its first green run (ER-41)");
    assert_eq!(r.warnings(), 10);
}

#[test]
fn c130_without_its_allow_list_cannot_look() {
    let tree = c130_tree();
    std::fs::remove_file(tree.path().join("conformance/c130-allow.json")).unwrap();
    let r = run_checks(&c130_options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, bool, Option<&str>)> = r
        .findings
        .iter()
        .map(|f| (f.file.as_str(), f.unwaivable, f.outcome.map(|o| o.name())))
        .collect();
    assert_eq!(got, [("conformance/c130-allow.json", true, Some("check-broken"))]);
}

#[test]
fn live_c130_findings_equal_the_js_port_on_a_fixture_tree_and_on_this_tree() {
    if !has_node() {
        return skip("node is not installed");
    }
    let tree = c130_tree();
    for (root, app) in [(tree.path().to_path_buf(), "fixture"), (repo_root(), "paddock")] {
        let js: Value = serde_json::from_str(&node(
            JS_RUN_C130,
            &[("RT5_ROOT", root.display().to_string()), ("RT5_APP", app.into())],
        ))
        .unwrap();
        let r = run_checks(&c130_options(&root, app), &registry()).unwrap();
        let (got, want) = (shared_part(&r.json()), shared_part(&js));
        assert_eq!(got["findings"], want["findings"], "C130 on {app}");
        assert_eq!(got, want, "C130 on {app}");
    }
}

/// The check's own files are not read: their invented lines need no allow entry; another test file still is.
#[test]
fn c130_does_not_read_its_own_files() {
    let d = tempfile::tempdir().unwrap();
    let mut files: Vec<&str> = paddock_dev::check::c130::IMPLEMENTATION.to_vec();
    assert!(files.contains(&"tools/paddock-dev/tests/it/check_c130.rs") && files.contains(&"tools/paddock-dev/src/check/c130.rs"));
    files.push("tools/paddock-dev/tests/it/check.rs");
    for p in files {
        let to = d.path().join(p);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, SH).unwrap();
    }
    std::fs::write(d.path().join("conformance/c130-allow.json"), "{\"allow\": []}").unwrap();
    let r = run_checks(&c130_options(d.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(&str, usize)> = r.findings.iter().map(|f| (f.file.as_str(), f.line)).collect();
    assert_eq!(got, [("tools/paddock-dev/tests/it/check.rs", 1)], "another test file is still read");
}
