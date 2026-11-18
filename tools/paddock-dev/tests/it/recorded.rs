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

fn write(p: &std::path::Path, v: &Value) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    crate::golden::record(p, v);
    // C125 reads every file of the tree for a stale disk name, and a recording quotes the fixtures that plant one: the file spells
    // the slash of `/Volumes/` as the JSON escape `\u002f` (the same value once read), so it names no disk.
    let text = std::fs::read_to_string(p).unwrap().replace("/Volumes/", "\\u002fVolumes\\u002f");
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

/// The part of a report of the whole tree that is stable under unrelated edits: the number of errors and, per check, the files that
/// have a finding (no line numbers, so a moved line is no change). The tree changes under the tests, so a whole-tree answer is a
/// ceiling (ER-46: lists only shrink), not an exact copy.
pub(crate) fn tree_summary(report: &Value) -> Value {
    let mut sites: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> = Default::default();
    for f in report["findings"].as_array().unwrap() {
        sites
            .entry(f["check"].as_str().unwrap().to_string())
            .or_default()
            .insert(f["file"].as_str().unwrap_or("").to_string());
    }
    serde_json::json!({ "errors": report["errors"], "sites": sites })
}

/// What `got` (a [`tree_summary`]) has that `want` does not: errors that differ and files with a finding the recording did not have.
fn tree_excess(want: &Value, got: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if got["errors"] != want["errors"] {
        out.push(format!("errors: {} against {}", got["errors"], want["errors"]));
    }
    for (check, files) in got["sites"].as_object().unwrap() {
        for f in files.as_array().unwrap() {
            if !want["sites"][check].as_array().is_some_and(|w| w.contains(f)) {
                out.push(format!("{check} {}", f.as_str().unwrap()));
            }
        }
    }
    out
}

/// Holds `ours`, a report of this working tree, to the answer the JS host gave on the whole tree (`case`): the same errors and no
/// file with a finding that the JS host did not have. A bless rewrites the answer from `ours`, which can only shrink it in review.
pub(crate) fn assert_tree_within(case: &str, ours: &Value) {
    let want = expect(case, &tree_summary(ours));
    let excess = tree_excess(&want, &tree_summary(ours));
    assert!(
        excess.is_empty(),
        "{case}: the JS host reported none of these on this tree (a new site is a fix or a reviewed bless): {excess:?}"
    );
}

#[test]
fn a_commit_in_a_message_is_scrubbed_and_nothing_else() {
    let v = serde_json::json!({"m": ["at the merge base 8a4141d37; it may only shrink", "merge base main", "base 8a4141d37"]});
    assert_eq!(
        scrub(v),
        serde_json::json!({"m": ["at the merge base <sha>; it may only shrink", "merge base main", "base 8a4141d37"]})
    );
}

/// The positive control of [`assert_tree_within`]: a site the JS host did not report, an error it did not have and a check it did not
/// run each fail it, and a site that went away does not.
#[test]
fn a_tree_report_with_a_new_site_or_an_error_the_js_host_did_not_have_fails_the_ceiling() {
    let finding = |check: &str, file: &str| serde_json::json!({"check": check, "file": file, "line": 1});
    let report = |errors: u64, f: Vec<Value>| tree_summary(&serde_json::json!({"errors": errors, "findings": f}));
    let want = report(0, vec![finding("C1", "a.rs"), finding("C1", "b.rs")]);
    assert!(
        tree_excess(&want, &report(0, vec![finding("C1", "a.rs")])).is_empty(),
        "a site that went away is fine"
    );
    assert_eq!(
        tree_excess(&want, &report(0, vec![finding("C1", "c.rs")])),
        ["C1 c.rs"],
        "a new file"
    );
    assert_eq!(
        tree_excess(&want, &report(0, vec![finding("C2", "a.rs")])),
        ["C2 a.rs"],
        "a new check on a known file"
    );
    assert_eq!(tree_excess(&want, &report(1, vec![])).len(), 1, "an error");
}
