//! The command line contract: one JSON object with --json, stable exit codes, help and errors that name the next step.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::process::Command;

fn dev(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev")).args(args).output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn help_and_version_exit_zero() {
    for args in [&["--help"][..], &["ci", "slot", "--help"], &["--version"]] {
        let (code, out, _) = dev(args);
        assert_eq!(code, 0, "{args:?}");
        assert!(!out.is_empty());
    }
}

#[test]
fn a_wrong_command_line_is_usage_exit_2_and_json_mode_gets_one_error_object() {
    let (code, out, err) = dev(&["nonsense"]);
    assert_eq!(code, 2);
    assert!(out.is_empty() && err.contains("./dev --help"));
    let (code, out, _) = dev(&["--json", "ci", "slot", "acquire"]);
    assert_eq!(code, 2);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        (v["ok"].clone(), v["exit"].clone(), v["kind"].clone()),
        (false.into(), 2.into(), "usage".into())
    );
    assert!(v["hint"].as_str().unwrap().contains("--help"));
}

#[test]
fn self_hash_prints_one_object_with_the_hash_and_the_compiler() {
    let (code, out, _) = dev(&["--json", "self", "hash"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v["hash"].as_str().is_some_and(|h| h.len() >= 40));
    assert!(v["compiler"].as_array().is_some_and(|c| c.len() == 2));
    assert!(v["inputs"].as_array().is_some_and(|c| c.iter().any(|p| p == "tools/paddock-dev")));
    let (code, out, _) = dev(&["self", "hash"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("hash "));
}

#[test]
fn graph_affected_with_nothing_to_plan_is_usage() {
    let (code, _, err) = dev(&["graph", "affected"]);
    assert_eq!(code, 2);
    assert!(err.contains("--files"));
}
