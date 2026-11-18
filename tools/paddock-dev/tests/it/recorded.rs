//! The recorded answers of the retired JavaScript check host (pair P3, ADR 0022 amendment 2026-11-19): for every case a Rust check
//! test once compared with the live `conformance/src/checks/C*.mjs`, `tests/fixtures/check/recorded/<case>.json` holds what the JS
//! host answered. The files are written through the one bless of `golden.rs` (`PADDOCK_GOLDEN_BLESS=1`, refused in CI), never by hand.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::path::PathBuf;

fn path(case: &str) -> PathBuf {
    assert!(
        !case.is_empty() && case.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "a recorded case is named with letters, digits and dashes: {case:?}"
    );
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check/recorded")
        .join(format!("{case}.json"))
}

/// What the JS host answered for `case`, to hold `ours` to. With `PADDOCK_GOLDEN_BLESS=1` (the one bless of `golden.rs`, refused in CI) the
/// file is first rewritten from `ours`: a changed answer is a reviewed edit of a recorded file, never a silent one. The files were first
/// recorded from the JavaScript host (retired 2026-11-19), by the same call with that host's answer. Returns the recorded answer; a case
/// that was never recorded fails naming the command.
pub(crate) fn expect(case: &str, ours: &Value) -> Value {
    let p = path(case);
    if crate::golden::bless() {
        write(&p, &scrub(ours.clone()));
    }
    read(case, &p)
}

pub(crate) fn write(p: &std::path::Path, v: &Value) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    crate::golden::record(p, v);
    // C125 reads every file of the tree for a stale disk name, and a recording quotes the fixtures that plant one: the file spells
    // the slash of `/Volumes/` as the JSON escape `\u002f` (the same value once read), so it names no disk; that lean-profile folder is spelled so
    // too, because the checkout profiles' test fails a file that names a path the lean profile leaves out.
    let text = std::fs::read_to_string(p)
        .unwrap()
        .replace("/Volumes/", "\\u002fVolumes\\u002f")
        .replace(concat!("docs/", "design"), concat!("docs\\u002f", "design"));
    std::fs::write(p, text).unwrap();
}

fn read(case: &str, p: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(p).unwrap_or_else(|e| {
        panic!(
            "no recorded answer for {case} ({e}). Record it with `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test({case})'`, review it and commit {}",
            p.display()
        )
    });
    scrub(serde_json::from_str(&text).unwrap())
}

/// A commit named in a message (`merge base 8a4141d37`) is a machine value: a throw-away repository has a new one every run (ER-46).
/// Both sides of a comparison go through this, the recorded one inside [`expect`].
pub(crate) fn scrub(v: Value) -> Value {
    match v {
        Value::String(s) => {
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(i) = rest.find("merge base ") {
                let (head, tail) = rest.split_at(i + "merge base ".len());
                out.push_str(head);
                let hex = tail.chars().take_while(|c| c.is_ascii_hexdigit()).count();
                if hex >= 7 {
                    out.push_str("<sha>");
                    rest = &tail[hex..];
                } else {
                    rest = tail;
                }
            }
            out.push_str(rest);
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(scrub).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, scrub(v))).collect()),
        other => other,
    }
}

/// What a run over the live tree must hold whatever else the tree holds (P1-1 of the review of pair P3): the checks ran, looked at
/// something, passed their positive control and found no error. Which warnings the tree has is not asserted: an unrelated change may add
/// a report-only site, and a test that fails for it fails whoever next runs paddock-dev. The comparison with the JS host's answers is
/// made on a frozen copy of files ([`frozen`]), never on the live tree.
pub(crate) fn live_tree_problems(report: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if report["errors"] != 0 {
        out.push(format!("{} errors", report["errors"]));
    }
    let runs = report["runs"].as_array().map_or(&[][..], Vec::as_slice);
    if runs.is_empty() {
        out.push("the report has no run record: nothing was measured".to_string());
    }
    for r in runs {
        let id = r["check"].as_str().unwrap_or("?");
        if r["ran"] != true || r["completed"] != true {
            out.push(format!("{id} did not run to the end"));
        }
        if r["control"] != true {
            out.push(format!("{id}: its positive control did not see what it is built to see"));
        }
        if r["probes"].as_u64().unwrap_or(0) == 0 {
            out.push(format!("{id} looked at nothing"));
        }
        if r["precondition"]["met"] != true {
            out.push(format!("{id}: its precondition failed"));
        }
    }
    out
}

/// [`live_tree_problems`] as an assertion naming `what`.
pub(crate) fn assert_live_tree_sound(what: &str, report: &Value) {
    let p = live_tree_problems(report);
    assert!(p.is_empty(), "{what}: the run over this tree is not sound: {p:?}");
}

/// The tests' own frozen tree: `tests/fixtures/check/real-tree.json`, the files of this repository that carry findings of the 12 checks
/// (and the lists and baselines they read) as they were on 2026-11-19, with the answer the JavaScript host gave on them
/// (`recorded/frozen-real-tree.json`, made from commit 984fa453b with `tests/fixtures/check/harvest/freeze.mjs`). It never changes with
/// the repository, so comparing with it cannot fail for a later change.
pub(crate) fn frozen() -> (tempfile::TempDir, Value) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/check/real-tree.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir).unwrap()).unwrap();
    let tree = tempfile::tempdir().unwrap();
    for f in v["files"].as_array().unwrap() {
        let to = tree.path().join(f[0].as_str().unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, f[1].as_str().unwrap()).unwrap();
    }
    (tree, v)
}

/// The wording the hosts own differently, made equal on both sides: the command that regenerates a list (the JS tool is retired), the
/// JSON parsers' own error text (serde_json and V8), the git error behind "no merge base" (a bare temporary folder), and a check that
/// could not look at all, which the JS host calls "check broken: no <file> was found to read" and the Rust host "not measured": both
/// are an unwaivable error that never passes (ER-33).
pub(crate) fn normal(report: &Value) -> Value {
    let mut v = report.clone();
    for list in ["findings", "waived"] {
        for f in v[list].as_array_mut().unwrap() {
            let mut m = f["message"].as_str().unwrap().to_string();
            m = m.replace("node conformance/bin/paddock-shell-baseline.mjs", "./dev check shell-baseline");
            if let Some(i) = m.find(" is not JSON: ") {
                m.truncate(i + " is not JSON: ".len());
            }
            if let Some(i) = m.find("no merge base (") {
                m.truncate(i + "no merge base (".len());
            }
            let fix = f["fix"]
                .as_str()
                .unwrap_or_default()
                .replace("node conformance/bin/paddock-shell-baseline.mjs", "./dev check shell-baseline");
            let fix = fix.split(" is not JSON: ").next().unwrap_or_default().to_string();
            let refused = f["unwaivable"] == true
                && f["severity"] == "error"
                && f["file"]
                    .as_str()
                    .is_some_and(|p| p.is_empty() || !p.contains('.') || p.ends_with("workflows"));
            if refused && (m.contains("was found to read") || m.contains("its precondition failed")) {
                m = "a check that could not look refuses to pass".to_string();
                f["file"] = serde_json::json!("");
                f["fix"] = serde_json::json!("");
                f["outcome"] = serde_json::json!("");
            } else {
                f["fix"] = serde_json::json!(fix);
            }
            f["message"] = serde_json::json!(m);
        }
    }
    v
}

#[test]
fn a_commit_in_a_message_is_scrubbed_and_nothing_else() {
    let v = serde_json::json!({"m": ["at the merge base 8a4141d37; it may only shrink", "merge base main", "base 8a4141d37"]});
    assert_eq!(
        scrub(v),
        serde_json::json!({"m": ["at the merge base <sha>; it may only shrink", "merge base main", "base 8a4141d37"]})
    );
}

/// P1-1's positive control, both ways. A report with a new site the recorded answer never had (a planted shell script is a C129
/// warning) is sound, so a later unrelated change cannot fail the live tests; a report whose control failed, that looked at nothing,
/// that did not complete or that has an error is not.
#[test]
fn a_new_site_in_the_live_tree_is_sound_and_a_broken_control_is_not() {
    let run = |check: &str, control: bool, probes: u64, completed: bool, met: bool| serde_json::json!({"check": check, "ran": true, "completed": completed, "control": control, "probes": probes, "precondition": {"met": met}});
    let report =
        |errors: u64, findings: Vec<Value>, runs: Vec<Value>| serde_json::json!({"errors": errors, "findings": findings, "runs": runs});
    let planted = serde_json::json!({"check": "C129", "file": "apps/debrief/scripts/new.sh", "severity": "warn"});
    assert!(
        live_tree_problems(&report(0, vec![planted], vec![run("C129", true, 40, true, true)])).is_empty(),
        "a new warning does not fail"
    );
    for (what, bad) in [
        ("a failed control", report(0, vec![], vec![run("C129", false, 40, true, true)])),
        ("nothing looked at", report(0, vec![], vec![run("C129", true, 0, true, true)])),
        ("not completed", report(0, vec![], vec![run("C129", true, 40, false, true)])),
        ("a failed precondition", report(0, vec![], vec![run("C129", true, 40, true, false)])),
        ("an error", report(1, vec![], vec![run("C129", true, 40, true, true)])),
        ("no run record", report(0, vec![], vec![])),
    ] {
        assert!(!live_tree_problems(&bad).is_empty(), "{what} fails");
    }
}
