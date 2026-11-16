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
            "{\"allow\":[{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"baseline: planted before StandIn\",\"count\":2},{\"path\":\"tools/t/src/d_test.rs\",\"site\":\"conformance: stand-in\",\"reason\":\"the hook under test is a script\"},{\"path\":\"tools/t/tests/it/gone.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"a stale entry\"}]}".to_string(),
        ),
        ("tools/t/tests/it/ok.rs", format!("{SH}{SH}{SH}")),
        ("tools/t/tests/it/bad.rs", format!("fn b() {{}}\n{SH}")),
        ("tools/t/src/lib.rs", "const SHIM: &str = \"#!/bin/sh\\n\";\n".to_string()),
        ("tools/t/src/a.rs", format!("const SHIM: &str = \"#!/bin/sh\\n\";\n#[cfg(test)]\nmod tests {{\n{SH}}}\n")),
        ("tools/t/src/b.rs", format!("#[cfg(all(test, unix))]\nmod t {{\n{SH}}}\n")),
        ("tools/t/src/c.rs", format!("#[cfg(any(test, windows))]\nmod t {{\n{SH}}}\n")),
        ("tools/t/src/tests.rs", "let a = \"#!/usr/bin/env bash\\n\";\nlet b = \"#!/bin/zsh\\n\";\nlet c = \"#!/usr/bin/env node\\n\";\n".to_string()),
        ("tools/t/src/d_test.rs", "// writes #!/bin/sh in the old test\n/// doc #!/bin/sh\nlet w = \"#!/bin/sh\\n\"; // conformance: stand-in — the hook under test is a script\n// conformance: stand-in — a script by design\nlet v = \"#!/bin/sh\\n\";\n\nlet u = \"#!/bin/sh\\n\";\n".to_string()),
        (
            "tools/t/src/e_test.rs",
            "let a = \"#!/bin/sh\\n\"; // conformance: stand-in \u{2014} ab\r\nlet b = \"#!/bin/sh\\n\"; // conformance: stand-in \u{2014} \u{1F600}a\r\nlet c = \"#!/bin/sh\\n\"; // conformance: stand-in \u{2014} \u{1F600}\r\n".to_string(),
        ),
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
            ("tools/t/src/d_test.rs".to_string(), 5),
            ("tools/t/src/d_test.rs".to_string(), 7),
            ("tools/t/src/e_test.rs".to_string(), 1),
            ("tools/t/src/e_test.rs".to_string(), 2),
            ("tools/t/src/e_test.rs".to_string(), 3),
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
    assert_eq!(r.warnings(), 14);
    assert!(
        r.findings.iter().any(|f| f.file == "tools/t/src/d_test.rs"
            && f.line == 5
            && f.message == "a stand-in waiver beyond the count of its allow entry")
    );
    assert!(
        r.findings
            .iter()
            .any(|f| f.file == "tools/t/src/e_test.rs" && f.line == 2 && f.message == "a stand-in waiver the allow list does not name")
    );
    assert!(
        !r.findings.iter().any(|f| f.file == "tools/t/src/d_test.rs" && f.line == 3),
        "the first waiver is named by the list"
    );
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

#[allow(clippy::disallowed_methods, reason = "a test spawns git; it ends it itself")]
fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
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
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn allow_of(count: usize, extra: &str) -> String {
    format!(
        "{{\"allow\":[{{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/sh\",\"reason\":\"baseline: planted before StandIn\",\"count\":{count}}}{extra}]}}"
    )
}

/// A repository whose merge base (`origin/main`) holds the allow list `base` and whose `topic` commit holds `now`.
fn growth_repo(base: &str, now: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::create_dir_all(root.join("conformance")).unwrap();
    std::fs::create_dir_all(root.join("tools/t/tests/it")).unwrap();
    std::fs::write(root.join("conformance/c130-allow.json"), base).unwrap();
    std::fs::write(root.join("tools/t/tests/it/ok.rs"), SH.repeat(3)).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(root, &["checkout", "-q", "-b", "topic"]);
    std::fs::write(root.join("conformance/c130-allow.json"), now).unwrap();
    git(root, &["commit", "-q", "-a", "--allow-empty", "-m", "topic"]);
    d
}

fn grew(root: &Path) -> Vec<(String, usize, bool, String)> {
    let r = run_checks(&c130_options(root, "fixture"), &registry()).unwrap();
    r.findings
        .iter()
        .filter(|f| f.message.contains("allow list grew") || f.message.contains("shrink-only rule"))
        .map(|f| (f.file.clone(), f.line, f.unwaivable, f.message.clone()))
        .collect()
}

#[test]
fn a_raised_count_or_a_new_entry_is_one_unwaivable_finding_and_shrinking_is_silent() {
    assert!(grew(growth_repo(&allow_of(3, ""), &allow_of(3, "")).path()).is_empty());
    assert!(
        grew(growth_repo(&allow_of(3, ""), &allow_of(2, "")).path()).is_empty(),
        "a lowered count is shrinking"
    );
    let raised = grew(growth_repo(&allow_of(2, ""), &allow_of(3, "")).path());
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert_eq!(
        (raised[0].0.as_str(), raised[0].1, raised[0].2),
        ("conformance/c130-allow.json", 1, true)
    );
    assert!(
        raised[0]
            .3
            .starts_with("the allow list grew: 1 entry is (counting each `count`) not in conformance/c130-allow.json at the merge base ")
    );
    let extra =
        ",{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/bash\",\"reason\":\"baseline: planted before StandIn\",\"count\":2}";
    let added = grew(growth_repo(&allow_of(1, ""), &allow_of(1, extra)).path());
    assert!(added[0].3.contains("grew: 2 entries are"), "{added:?}");
}

#[test]
fn growth_is_not_measured_on_a_fixture_folder_and_a_missing_base_is_said() {
    let tree = c130_tree();
    assert!(grew(tree.path()).is_empty(), "no .git: nothing to compare, nothing said");
    let repo = growth_repo(&allow_of(1, ""), &allow_of(1, ""));
    let mut o = c130_options(repo.path(), "fixture");
    o.base_ref = Some("no-such-ref".into());
    let r = run_checks(&o, &registry()).unwrap();
    let said: Vec<_> = r
        .findings
        .iter()
        .filter(|f| f.message.contains("shrink-only rule is not enforced"))
        .collect();
    assert_eq!(said.len(), 1, "a checkout where git cannot give a base says so, never silence");
}

#[test]
fn growth_findings_equal_the_js_host_in_a_git_repository() {
    if !has_node() {
        return skip("node is not installed");
    }
    let extra = ",{\"path\":\"tools/t/tests/it/ok.rs\",\"site\":\"#!/bin/bash\",\"reason\":\"baseline: planted before StandIn\"}";
    for (name, repo) in [
        ("raised", growth_repo(&allow_of(2, ""), &allow_of(3, ""))),
        ("added", growth_repo(&allow_of(3, ""), &allow_of(3, extra))),
        ("same", growth_repo(&allow_of(3, ""), &allow_of(3, ""))),
    ] {
        let js: Value = serde_json::from_str(&node(
            JS_RUN_C130,
            &[("RT5_ROOT", repo.path().display().to_string()), ("RT5_APP", "fixture".into())],
        ))
        .unwrap();
        let r = run_checks(&c130_options(repo.path(), "fixture"), &registry()).unwrap();
        let (got, want) = (shared_part(&r.json()), shared_part(&js));
        assert_eq!(got["findings"], want["findings"], "{name}");
        assert_eq!(got, want, "{name}");
    }
}

/// `main` has the list `base`; `train` (off main) has `train_list`; `member` (off train) has `member_list`; both host refs are in
/// `refs/remotes/origin/*` so a run names them as `origin/main` and `origin/train`.
fn train_repo(base: &str, train_list: &str, member_list: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::create_dir_all(root.join("conformance")).unwrap();
    std::fs::create_dir_all(root.join("tools/t/tests/it")).unwrap();
    std::fs::write(root.join("conformance/c130-allow.json"), base).unwrap();
    std::fs::write(root.join("tools/t/tests/it/ok.rs"), SH.repeat(4)).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "main"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(root, &["checkout", "-q", "-b", "train"]);
    std::fs::write(root.join("conformance/c130-allow.json"), train_list).unwrap();
    git(root, &["commit", "-q", "-a", "-m", "train member 1"]);
    git(root, &["update-ref", "refs/remotes/origin/train", "HEAD"]);
    git(root, &["checkout", "-q", "-b", "member"]);
    std::fs::write(root.join("conformance/c130-allow.json"), member_list).unwrap();
    git(root, &["commit", "-q", "-a", "--allow-empty", "-m", "member"]);
    d
}

fn grew_against(root: &Path, base_ref: &str) -> Vec<String> {
    let mut o = c130_options(root, "fixture");
    o.base_ref = Some(base_ref.into());
    run_checks(&o, &registry())
        .unwrap()
        .findings
        .iter()
        .filter(|f| f.message.contains("allow list grew"))
        .map(|f| f.message.clone())
        .collect()
}

#[test]
fn the_trains_own_entries_are_not_growth_of_a_member_measured_against_the_train() {
    let repo = train_repo(&allow_of(1, ""), &allow_of(3, ""), &allow_of(3, ""));
    assert_eq!(
        grew_against(repo.path(), "origin/main").len(),
        1,
        "against main the train's raise shows as the member's growth"
    );
    assert!(
        grew_against(repo.path(), "origin/train").is_empty(),
        "against the train the member added nothing"
    );
    let own = train_repo(&allow_of(1, ""), &allow_of(3, ""), &allow_of(4, ""));
    let msgs = grew_against(own.path(), "origin/train");
    assert_eq!(msgs.len(), 1, "the member's own raise counts");
    assert!(msgs[0].starts_with("the allow list grew: 1 entry is"), "{msgs:?}");
    // The JavaScript host agrees on the same repositories with the same ref.
    if has_node() {
        for (name, repo) in [("clean", &repo), ("own", &own)] {
            let js: Value = serde_json::from_str(&node(
                JS_RUN_C130,
                &[
                    ("RT5_ROOT", repo.path().display().to_string()),
                    ("RT5_APP", "fixture".into()),
                    ("PADDOCK_BASE_REF", "origin/train".into()),
                ],
            ))
            .unwrap();
            let mut o = c130_options(repo.path(), "fixture");
            o.base_ref = Some("origin/train".into());
            let r = run_checks(&o, &registry()).unwrap();
            assert_eq!(shared_part(&r.json())["findings"], shared_part(&js)["findings"], "{name}");
        }
    }
}

#[test]
fn a_base_in_the_older_format_says_so_with_its_own_fix_on_both_hosts() {
    let repo = growth_repo(
        "{\"allow\":[{\"path\":\"tools/t/tests/it/ok.rs\",\"reason\":\"an older entry without a site\"}]}",
        &allow_of(1, ""),
    );
    let r = run_checks(&c130_options(repo.path(), "fixture"), &registry()).unwrap();
    let f: Vec<_> = r
        .findings
        .iter()
        .filter(|f| f.message.contains("shrink-only rule is not enforced"))
        .collect();
    assert_eq!(f.len(), 1);
    assert!(
        f[0].message.contains("cannot be parsed") && f[0].fix.contains("older format"),
        "{:?}",
        f[0]
    );
}
