//! C120 (nothing guesses cargo's build-folder layout): the Rust host against the JavaScript check, the oracle, on the fixture cases of
//! `tests/fixtures/layout-guess` (`a__b__c.rs.txt` is `a/b/c.rs`) and on this working tree. Node is required: a missing oracle fails the
//! test (ER-33).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

const DRIVER: &str = r#"
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const { runChecks } = await import(pathToFileURL(join(process.cwd(), 'conformance/src/runner.mjs')).href);
const r = await runChecks({ root: process.env.C120_ROOT, app: 'paddock', only: ['C120'] });
process.stdout.write(JSON.stringify({ findings: r.findings, waived: r.waived, errors: r.errors, warnings: r.warnings }));
"#;

fn js(root: &Path) -> Value {
    let out = paddock_proc::command("node")
        .args(["--input-type=module", "-e", DRIVER])
        .current_dir(repo())
        .env("C120_ROOT", root)
        .output()
        .expect("node must be installed: the oracle cannot be skipped");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn rust(root: &Path) -> Value {
    let opts = RunOptions {
        root: root.to_path_buf(),
        app: "paddock".into(),
        only: Some(vec!["C120".into()]),
        files: None,
        today: paddock_dev::check::today_utc(),
        base_ref: None,
    };
    let r = run_checks(&opts, &registry()).unwrap().json();
    json!({"findings": r["findings"], "waived": r["waived"], "errors": r["errors"], "warnings": r["warnings"]})
}

/// The fixture cases as a tree.
fn cases() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layout-guess");
    for entry in std::fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        let rel = name.strip_suffix(".txt").unwrap().replace("__", "/");
        let to = d.path().join(&rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/layout-guess")
                .join(&name),
            to,
        )
        .unwrap();
    }
    d
}

#[test]
fn every_shape_gives_the_same_findings_in_both_hosts() {
    let d = cases();
    let (a, b) = (js(d.path()), rust(d.path()));
    assert_eq!(a, b);
    let at = |file: &str| -> Vec<u64> {
        a["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["file"] == file)
            .map(|f| f["line"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(
        at("crates/a/src/c120_bad.rs"),
        [2, 6, 14, 15, 19, 20, 21, 23, 27, 28, 29, 30],
        "parent hops, dot-dot hops, ancestors().nth(n >= 2), literal deps folders and path ends"
    );
    assert!(at("crates/a/src/c120_good.rs").is_empty() && at("crates/a/src/c120_not_a_layout.rs").is_empty());
    assert!(
        at("crates/paddock-env/src/cargo_layout.rs").is_empty(),
        "the implementation is skipped"
    );
    assert_eq!(at("scripts/c120_bad.sh"), [3, 4, 5, 8, 9]);
    assert!(at("scripts/c120_good.sh").is_empty());
    assert_eq!(a["errors"], 0, "report-only: warnings never fail");
    // A waived site is counted as waived (with its reason), not as a finding: the site's own reason beats the file's, the file's
    // reason covers `#[cfg(test)]` modules and files under tests/, and production code of the same file is still reported.
    let waived = |file: &str| -> Vec<(u64, String)> {
        a["waived"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["file"] == file)
            .map(|w| (w["line"].as_u64().unwrap(), w["waiver"]["reason"].as_str().unwrap().to_string()))
            .collect()
    };
    let w = waived("crates/a/src/c120_file_waiver.rs");
    assert_eq!(w.iter().map(|x| x.0).collect::<Vec<_>>(), [10, 12, 13], "{w:?}");
    assert!(
        w[0].1.contains("filler data") && w[1].1.contains("its own reason") && w[2].1.contains("filler data"),
        "{w:?}"
    );
    assert_eq!(
        at("crates/a/src/c120_file_waiver.rs"),
        [3, 18],
        "production code in a file with a file waiver is still reported"
    );
    assert_eq!(waived("crates/a/tests/c120_tests_dir.rs").len(), 1);
    assert!(at("crates/a/tests/c120_tests_dir.rs").is_empty());
    assert!(a["waived"].as_array().unwrap().iter().all(|w| w["waiver"]["expires"] == "inline"));
    assert_eq!(
        at("crates/a/src/c120_bare_file_waiver.rs"),
        [3],
        "a file marker without a reason waives nothing"
    );
}

#[test]
fn this_working_tree_gives_the_same_findings_in_both_hosts() {
    let root = repo();
    assert_eq!(js(&root)["findings"], rust(&root)["findings"]);
    assert_eq!(rust(&root)["warnings"], 0, "every site carries a waiver with a reason, or is fixed");
    assert!(
        rust(&root)["waived"].as_array().unwrap().len() >= 20,
        "the waived sites stay visible in the report"
    );
}

/// A check that marks one finding inline-waived and one unwaivable-and-inline-waived (as a buggy check might).
struct Fake;

impl paddock_dev::check::Check for Fake {
    fn id(&self) -> &'static str {
        "X1"
    }
    fn rule(&self) -> u32 {
        1
    }
    fn title(&self) -> &'static str {
        "fake"
    }
    fn run(&self, _ctx: &paddock_dev::check::Ctx) -> paddock_dev::check::CheckOutput {
        use paddock_dev::check::finding::Finding;
        paddock_dev::check::CheckOutput {
            findings: vec![
                Finding::warn("X1", 1, "x.rs", 1, "m", "f").inline_waived("a reason here"),
                Finding::error("X1", 1, "x.rs", 2, "it could not look", "fix")
                    .unwaivable()
                    .inline_waived("a comment cannot cover this"),
            ],
            probes: 1,
            completed: true,
            precondition: paddock_dev::check::record::Precondition {
                met: true,
                detail: String::new(),
            },
        }
    }
    fn control(&self) -> paddock_dev::check::Control {
        paddock_dev::check::Control {
            files: vec![("x.rs", "fn a() {}\n")],
            expect: vec![("x.rs", 1), ("x.rs", 2)],
        }
    }
}

#[test]
fn an_unwaivable_finding_is_never_waived_by_a_comment_and_an_inline_waived_one_counts_as_waived() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("x.rs"), "fn a() {}\n").unwrap();
    let opts = RunOptions {
        root: d.path().to_path_buf(),
        app: "x".into(),
        only: None,
        files: None,
        today: "2026-11-13".into(),
        base_ref: None,
    };
    let r = run_checks(&opts, &[Box::new(Fake)]).unwrap();
    assert_eq!(r.waived.len(), 1);
    assert_eq!(
        (r.waived[0].reason.as_str(), r.waived[0].expires.as_str()),
        ("a reason here", "inline")
    );
    assert_eq!(r.findings.len(), 1);
    assert_eq!(r.findings[0].line, 2, "the unwaivable finding stays a finding");
    assert_eq!(r.errors(), 1);
}
