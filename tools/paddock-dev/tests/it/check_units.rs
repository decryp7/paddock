//! Every input the JavaScript tests of the 12 retired check modules fed their detection functions or the runner (pair P3, ADR 0022
//! amendment 2026-11-19), recorded with the JS host's answer: `tests/fixtures/check/units/c<n>.json`. Each case is a small tree; the
//! Rust check must answer it as the JS one did (findings, waived, errors, warnings). The inputs were harvested by running the JS
//! test files with their exported functions and `runChecks` wrapped; a case that needs a git history or an unreadable file is not here
//! (the Rust tests of those, `check_c129.rs`, `check_c130.rs` and `check.rs`, build them).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::PathBuf;

fn units(id: &str) -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check/units")
        .join(format!("{}.json", id.to_lowercase()));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

/// The wording the hosts own differently, made equal on both sides: the command that regenerates a list (the JS tool is retired), the
/// JSON parsers' own error text (serde_json and V8), the git error behind "no merge base" (a bare temporary folder), and a check that
/// could not look at all, which the JS host calls "check broken: no <file> was found to read" and the Rust host "not measured": both
/// are an unwaivable error that never passes (ER-33).
fn normal(report: &Value) -> Value {
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
                f["file"] = json!("");
                f["fix"] = json!("");
                f["outcome"] = json!("");
            } else {
                f["fix"] = json!(fix);
            }
            f["message"] = json!(m);
        }
    }
    v
}

/// The cases of `id` the Rust host answers differently, each with what differs (the first difference of the findings).
fn differences(id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let cases = units(id);
    assert!(!cases.is_empty(), "{id}: no recorded case (a loop over nothing passes, ER-12)");
    for case in &cases {
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
        let got = normal(&run_checks(&opts, &registry()).unwrap().json());
        let want = normal(&case["result"]);
        for k in ["findings", "waived", "errors", "warnings"] {
            if got[k] != want[k] {
                out.push(format!("{id} {} `{k}`: Rust {} against JS {}", case["name"], got[k], want[k]));
                break;
            }
        }
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
