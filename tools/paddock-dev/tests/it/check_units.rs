//! Every input the JavaScript tests of the 12 retired check modules fed their detection functions or the runner (pair P3, ADR 0022
//! amendment 2026-11-19), recorded with the JS host's answer: `tests/fixtures/check/units/c<n>.json`. Each case is a small tree; the
//! Rust check must answer it as the JS one did (findings, waived, errors, warnings). The inputs were harvested by running the JS
//! test files with their exported functions and `runChecks` wrapped; a case that needs a git history or an unreadable file is not here
//! (the Rust tests of those, `check_c129.rs`, `check_c130.rs` and `check.rs`, build them).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::recorded::normal;
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::PathBuf;

fn units(id: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check/units")
        .join(format!("{}.json", id.to_lowercase()));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

/// The cases of `id` the Rust host answers differently, each with what differs (the first difference of the findings).
fn differences(id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cases = units(id);
    let bless = crate::golden::bless();
    assert!(!cases.is_empty(), "{id}: no recorded case (a loop over nothing passes, ER-12)");
    for case in &mut cases {
        let dir = tempfile::tempdir().unwrap();
        for f in case["files"].as_array().unwrap() {
            let (path, text) = (f[0].as_str().unwrap(), f[1].as_str().unwrap());
            let to = dir.path().join(path);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(to, text).unwrap();
        }
        // The Rust host calls a whole-tree run that looked at nothing "not measured" (ER-33); the JS host answered `[]` for an input its
        // check skips by path. A file every check reads (C124 reads only `tools/paddock-dev/src`) and flags nothing in gives the Rust run something to have looked at.
        for rel in ["crates/filler/src/filler.rs", "tools/paddock-dev/src/filler.rs"] {
            let filler = dir.path().join(rel);
            if !filler.exists() {
                std::fs::create_dir_all(filler.parent().unwrap()).unwrap();
                std::fs::write(filler, "fn filler() {}\n").unwrap();
            }
        }
        let today = case["now"]
            .as_str()
            .map_or_else(paddock_dev::check::today_utc, |n| n[..10].to_string());
        let opts = RunOptions {
            root: dir.path().to_path_buf(),
            config: paddock_dev::check::config::Config::bare(case["app"].as_str().unwrap()),
            scope: None,
            only: Some(vec![id.to_string()]),
            files: None,
            today,
            base_ref: Some("origin/main".into()),
            ci: Some(false),
        };
        let raw = run_checks(&opts, &registry()).unwrap().json();
        if bless {
            // A reviewed bless (refused in CI by `golden::bless`): the recorded answer becomes what the Rust host answers now.
            case["result"] =
                json!({"findings": raw["findings"], "waived": raw["waived"], "errors": raw["errors"], "warnings": raw["warnings"]});
        }
        let got = normal(&raw);
        let want = normal(&case["result"]);
        for k in ["findings", "waived", "errors", "warnings"] {
            if got[k] != want[k] {
                out.push(format!("{id} {} `{k}`: Rust {} against JS {}", case["name"], got[k], want[k]));
                break;
            }
        }
    }
    if bless {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/check/units")
            .join(format!("{}.json", id.to_lowercase()));
        crate::recorded::write(&path, &Value::Array(cases));
    }
    out
}

macro_rules! unit_parity {
    ($($name:ident: $id:literal,)+) => {$(
        #[test]
        fn $name() {
            let d = differences($id);
            assert!(d.is_empty(), "the Rust host answers {} differently from the JS host on {} recorded inputs:\n{}", $id, d.len(), d.join("\n"));
        }
    )+};
}

unit_parity! {
    c39_answers_every_input_of_the_js_tests_as_the_js_host_did: "C39",
    c53_answers_every_input_of_the_js_tests_as_the_js_host_did: "C53",
    c119_answers_every_input_of_the_js_tests_as_the_js_host_did: "C119",
    c120_answers_every_input_of_the_js_tests_as_the_js_host_did: "C120",
    c121_answers_every_input_of_the_js_tests_as_the_js_host_did: "C121",
    c122_answers_every_input_of_the_js_tests_as_the_js_host_did: "C122",
    c123_answers_every_input_of_the_js_tests_as_the_js_host_did: "C123",
    c124_answers_every_input_of_the_js_tests_as_the_js_host_did: "C124",
    c125_answers_every_input_of_the_js_tests_as_the_js_host_did: "C125",
    c129_answers_every_input_of_the_js_tests_as_the_js_host_did: "C129",
    c130_answers_every_input_of_the_js_tests_as_the_js_host_did: "C130",
    c131_answers_every_input_of_the_js_tests_as_the_js_host_did: "C131",
}

/// The positive control: a recorded answer that is changed is seen (a comparison that cannot fail proves nothing, ER-33).
#[test]
fn a_changed_recorded_answer_is_a_difference() {
    let mut case = units("C39").remove(0);
    case["result"]["warnings"] = json!(case["result"]["warnings"].as_u64().unwrap() + 7);
    let dir = tempfile::tempdir().unwrap();
    for f in case["files"].as_array().unwrap() {
        let to = dir.path().join(f[0].as_str().unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, f[1].as_str().unwrap()).unwrap();
    }
    let opts = RunOptions {
        root: dir.path().to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(vec!["C39".into()]),
        files: None,
        today: "2026-11-19".into(),
        base_ref: None,
        ci: Some(false),
    };
    let got = run_checks(&opts, &registry()).unwrap().json();
    assert_ne!(got["warnings"], case["result"]["warnings"]);
}

/// The real-tree comparison, on a frozen copy (P1-1): the files of this repository that carry findings of the 12 checks, as they were
/// when the JS host was retired, answer as the JS host answered. The live tree is only held to be sound (`recorded::live_tree_problems`).
#[test]
fn the_frozen_real_tree_answers_as_the_js_host_did() {
    let (tree, frozen) = crate::recorded::frozen();
    assert!(frozen["files"].as_array().unwrap().len() > 100, "the frozen tree has its files");
    let ids: Vec<String> = [
        "C39", "C53", "C119", "C120", "C121", "C122", "C123", "C124", "C125", "C129", "C130", "C131",
    ]
    .map(String::from)
    .to_vec();
    let opts = RunOptions {
        root: tree.path().to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(ids),
        files: None,
        today: "2026-11-19".into(),
        base_ref: Some("origin/main".into()),
        ci: Some(false),
    };
    let raw = run_checks(&opts, &registry()).unwrap().json();
    // C122's rules D and E (2026-11-21) are newer than the JavaScript host this file records: the comparison covers the rules it had.
    let is_new_rule = |f: &Value| {
        f["message"]
            .as_str()
            .is_some_and(|m| m.contains("(rule D:") || m.contains("(rule E:"))
    };
    let findings: Vec<Value> = raw["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| !is_new_rule(f))
        .cloned()
        .collect();
    let dropped = raw["findings"].as_array().unwrap().len() - findings.len();
    let warnings = raw["warnings"].as_u64().unwrap() - dropped as u64;
    let shared = json!({"app": raw["app"], "checks": raw["checks"], "findings": findings, "waived": raw["waived"], "errors": raw["errors"], "warnings": warnings});
    let want = crate::recorded::expect("frozen-real-tree", &shared);
    assert_eq!(normal(&shared), normal(&want));
}
