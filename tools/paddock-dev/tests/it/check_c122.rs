//! C122 (tests compare paths as paths, ADR 0099): the Rust host against the answers the JavaScript check gave (recorded in `tests/fixtures/check/recorded`; the JS module is retired, pair P3), on a fixture tree and on
//! this working tree, and the tree's own state (no rule A or B site is left; the check starts with an empty baseline).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::{LIVE_STALL, repo_root};
use paddock_dev::check::tree::Tree;
use paddock_dev::check::{RunOptions, registry, run_checks, run_checks_in};
use serde_json::{Value, json};
use std::path::Path;

/// Files with sites of every rule, a waiver, non-test code, a skipped folder and non-ASCII text before a site.
const CASES: [(&str, &str); 6] = [
    (
        "crates/a/tests/it/bad.rs",
        "// é😀\nfn t() {\n    let a = format!(\"{}/x\", p.display());\n    let b = s.replace('\\\\', \"/\");\n    assert_eq!(p.display().to_string(), \"x\");\n    let c = format!(\"{}\\\\{}\", p.display(), n);\n}\n",
    ),
    (
        "crates/a/tests/it/waived.rs",
        "fn t() {\n    // conformance: path-text — a URL path, not a file\n    let a = format!(\"{}/x\", u.display());\n    // conformance: path-text\n    let b = format!(\"{}/y\", u.display());\n}\n",
    ),
    (
        "crates/a/tests/it/good.rs",
        "fn t() {\n    let a = format!(\"{}/x\", name);\n    let b = s.replace(\"a\", \"/\");\n}\n",
    ),
    (
        "crates/a/src/lib.rs",
        "fn f() { let a = format!(\"{}/x\", p.display()); }\n#[cfg(test)]\nmod tests {\n    fn t() { let a = format!(\"{}/x\", p.display()); }\n}\n",
    ),
    ("crates/a/src/x_tests.rs", "fn t() { let b = s.replace(r\"\\\", \"/\"); }\n"),
    ("apps/d/vendor/s/tests/v.rs", "fn t() { let a = format!(\"{}/x\", p.display()); }\n"),
];

fn options(root: &Path, app: &str) -> RunOptions {
    RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare(app),
        scope: None,
        only: Some(vec!["C122".into()]),
        files: None,
        today: "2026-11-13".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    }
}

fn shared_part(v: &Value) -> Value {
    json!({"app": v["app"], "checks": v["checks"], "findings": v["findings"], "waived": v["waived"], "errors": v["errors"], "warnings": v["warnings"]})
}

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
fn the_fixture_tree_reports_the_sites_of_every_rule_and_only_in_test_code() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got: Vec<(String, usize)> = r.findings.iter().map(|f| (f.file.clone(), f.line)).collect();
    assert_eq!(
        got,
        [
            ("crates/a/src/lib.rs".to_string(), 4),
            ("crates/a/src/x_tests.rs".to_string(), 1),
            ("crates/a/tests/it/bad.rs".to_string(), 3),
            ("crates/a/tests/it/bad.rs".to_string(), 4),
            ("crates/a/tests/it/bad.rs".to_string(), 5),
            ("crates/a/tests/it/bad.rs".to_string(), 6),
            ("crates/a/tests/it/waived.rs".to_string(), 5),
        ]
    );
    assert_eq!(r.errors(), 0, "report-only: every finding is a warning (ER-41)");
    assert_eq!(r.warnings(), 7);
}

#[test]
fn live_fixture_findings_equal_the_recorded_js_answer() {
    let tree = fixture_tree();
    let r = run_checks(&options(tree.path(), "fixture"), &registry()).unwrap();
    let got = shared_part(&r.json());
    assert_eq!(got, crate::recorded::expect("c122-fixture", &got));
}

#[test]
fn live_tree_run_is_sound_and_no_rule_a_or_b_site_is_left() {
    let root = repo_root();
    let r = paddock_testkit::guarded("the Rust C122 over the live tree", LIVE_STALL, {
        let root = root.clone();
        move |progress| {
            let tree = Tree::open(&root).with_progress(move || progress.tick());
            run_checks_in(&tree, &options(&root, "paddock"), &registry()).unwrap()
        }
    });
    crate::recorded::assert_live_tree_sound("c122", &r.json());
    // The ADR's WP-3b: all sites were moved onto paddock_testkit::path, so the check starts with an empty baseline. Rule C
    // warnings are a heuristic and stay.
    let left: Vec<String> = r
        .findings
        .iter()
        .filter(|f| f.message.contains("(rule A:") || f.message.contains("(rule B:"))
        .map(|f| format!("{}:{}", f.file, f.line))
        .collect();
    assert!(left.is_empty(), "rule A or B sites are back in test code: {left:?}");
}

/// Two rule D sites and one rule E site, in test code.
const DE_SOURCE: &str = "fn t() {\n    cmd.arg(a.display().to_string());\n    cmd.arg(b.display().to_string());\n    assert!(out.contains(&c.display().to_string()));\n}\n";

fn baseline_tree(counts: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, text) in [
        ("crates/a/tests/it/de.rs", DE_SOURCE.to_string()),
        (paddock_dev::check::c122_baseline::FILE, format!("{{\"counts\": {counts}}}")),
    ] {
        let to = d.path().join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, text).unwrap();
    }
    d
}

fn at_baseline(r: &paddock_dev::check::RunResult) -> Vec<String> {
    r.findings
        .iter()
        .filter(|f| f.file == paddock_dev::check::c122_baseline::FILE)
        .map(|f| f.message.clone())
        .collect()
}

/// ER-46: the count of rules D and E may only shrink. At the baseline there is nothing to say (the positive control: the tree has
/// sites, so a check that never counted would pass the same way, which the next two tests exclude).
#[test]
fn the_de_baseline_holds_when_the_counts_are_equal() {
    let r = run_checks(&options(baseline_tree(r#"{"D": 2, "E": 1}"#).path(), "fixture"), &registry()).unwrap();
    assert_eq!(
        r.findings.iter().filter(|f| f.file.ends_with("de.rs")).count(),
        3,
        "the three sites are reported as warnings"
    );
    assert!(at_baseline(&r).is_empty(), "{:?}", at_baseline(&r));
}

#[test]
fn a_count_above_the_baseline_is_one_unwaivable_finding_and_below_asks_to_lower_it() {
    let above = run_checks(&options(baseline_tree(r#"{"D": 1, "E": 1}"#).path(), "fixture"), &registry()).unwrap();
    assert_eq!(
        at_baseline(&above),
        ["rule D has 2 sites and the baseline allows 1: the count may only shrink"]
    );
    assert!(
        above
            .findings
            .iter()
            .any(|f| f.file == paddock_dev::check::c122_baseline::FILE && f.unwaivable)
    );
    let below = run_checks(&options(baseline_tree(r#"{"D": 2, "E": 4}"#).path(), "fixture"), &registry()).unwrap();
    assert_eq!(at_baseline(&below), ["rule E has 1 sites and the baseline still allows 4"]);
    assert_eq!(above.errors() + below.errors(), 0, "report-only");
}

#[test]
fn an_unreadable_baseline_is_a_check_broken_finding() {
    let r = run_checks(&options(baseline_tree("5").path(), "fixture"), &registry()).unwrap();
    assert_eq!(at_baseline(&r).len(), 1, "{:?}", at_baseline(&r));
    assert!(at_baseline(&r)[0].starts_with("the check cannot look:"));
}

/// The file against the merge base: a raised count in a commit is growth, one finding that needs the user's approval.
#[test]
fn a_baseline_raised_against_the_merge_base_is_growth() {
    let d = baseline_tree(r#"{"D": 2, "E": 1}"#);
    #[allow(clippy::disallowed_methods, reason = "a test spawns git; it ends it itself")]
    let git = |args: &[&str]| {
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
            .current_dir(d.path())
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(
        d.path().join(paddock_dev::check::c122_baseline::FILE),
        r#"{"counts": {"D": 1, "E": 1}}"#,
    )
    .unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "base"]);
    std::fs::write(
        d.path().join(paddock_dev::check::c122_baseline::FILE),
        r#"{"counts": {"D": 2, "E": 1}}"#,
    )
    .unwrap();
    git(&["commit", "-q", "-am", "raise"]);
    let mut o = options(d.path(), "fixture");
    o.base_ref = Some("HEAD".into());
    let r = run_checks(&o, &registry()).unwrap();
    let said = at_baseline(&r);
    assert!(said.iter().any(|m| m.starts_with("the count baseline grew by 1 site")), "{said:?}");
}
