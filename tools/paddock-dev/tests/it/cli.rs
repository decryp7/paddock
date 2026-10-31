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
    crate::repo::require_repo();
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
fn graph_affected_prints_the_plan_for_the_repository_it_runs_in_or_is_pointed_at() {
    let root = crate::repo::require_repo().display().to_string();
    for args in [
        vec![
            "--json",
            "graph",
            "affected",
            "--files",
            "crates/paddock-cli/src/lib.rs",
            "--explain",
        ],
        vec![
            "--json",
            "graph",
            "affected",
            "--root",
            &root,
            "--files",
            "crates/paddock-cli/src/lib.rs",
        ],
    ] {
        let (code, out, _) = dev(&args);
        assert_eq!(code, 0);
        let v: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(
            (v["ok"].clone(), v["crates"].clone(), v["all"].clone()),
            (true.into(), true.into(), false.into())
        );
        assert_eq!(v.get("why").is_some(), args.contains(&"--explain"));
    }
    let (code, out, _) = dev(&["graph", "affected", "--all"]);
    assert_eq!(code, 0);
    assert!(out.contains("\"all\":true"));
}

#[test]
fn graph_affected_outside_a_repository_fails_with_exit_1_and_names_the_problem() {
    let d = tempfile::tempdir().unwrap();
    let (code, out, err) = dev(&["--json", "graph", "affected", "--root", d.path().to_str().unwrap(), "--files", "a"]);
    assert_eq!(code, 1);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["ok"], false);
    assert!(err.contains("git repository"), "{err}");
}

#[test]
fn graph_affected_with_nothing_to_plan_is_usage() {
    let (code, _, err) = dev(&["graph", "affected"]);
    assert_eq!(code, 2);
    assert!(err.contains("--files"));
}

#[test]
fn self_hash_outside_a_repository_or_without_an_input_list_names_the_next_step() {
    let d = tempfile::tempdir().unwrap();
    let run = |dir: &std::path::Path| {
        let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .args(["self", "hash"])
            .current_dir(dir)
            .env("GIT_CEILING_DIRECTORIES", d.path().parent().unwrap())
            .output()
            .unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let (code, err) = run(d.path());
    assert_eq!(code, 1);
    assert!(err.contains("Paddock repository"), "{err}");
    // A repository that is not Paddock's: no tools/paddock-dev/inputs.txt.
    let init = Command::new("git").args(["init", "-q"]).current_dir(d.path()).status().unwrap();
    assert!(init.success());
    let (code, err) = run(d.path());
    assert_eq!(code, 1);
    assert!(err.contains("inputs.txt"), "{err}");
}

#[test]
fn graph_affected_outside_a_paddock_repository_fails_at_once_and_names_the_next_step() {
    let d = tempfile::tempdir().unwrap();
    let run = || {
        let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .args(["graph", "affected", "--all"])
            .current_dir(d.path())
            .env("GIT_CEILING_DIRECTORIES", d.path().parent().unwrap())
            .output()
            .unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    // No repository at all, then a repository that is not Paddock's (a copy inside an unrelated one).
    let (code, err) = run();
    assert_eq!(code, 1);
    assert!(err.contains("--root"), "{err}");
    let init = Command::new("git").args(["init", "-q"]).current_dir(d.path()).status().unwrap();
    assert!(init.success());
    let (code, err) = run();
    assert_eq!(code, 1);
    assert!(err.contains("no crates/Cargo.toml") && err.contains("--root"), "{err}");
}
