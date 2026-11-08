//! The command line contract: one JSON object with --json, stable exit codes, help and errors that name the next step.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::process::Command;

fn dev(args: &[&str]) -> (i32, String, String) {
    dev_in(None, args)
}

/// Runs `dev` in `dir` (the process's own folder when `None`): a test that reads a repository gives it one of its own.
fn dev_in(dir: Option<&std::path::Path>, args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.args(args);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.output().unwrap();
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
    // A repository of its own (ER-14): the checkout may be a worktree whose .git names a path this machine lacks.
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("tools/paddock-dev")).unwrap();
    std::fs::write(root.join("tools/paddock-dev/inputs.txt"), "# x\ntools/paddock-dev\n").unwrap();
    crate::golden::git(&root, &["init", "-q", "-b", "main"]);
    crate::golden::git(&root, &["add", "-A"]);
    crate::golden::git(&root, &["commit", "-q", "-m", "base"]);
    let (code, out, err) = dev_in(Some(&root), &["--json", "self", "hash"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v["hash"].as_str().is_some_and(|h| h.len() >= 40));
    assert!(v["compiler"].as_array().is_some_and(|c| c.len() == 2));
    assert!(v["inputs"].as_array().is_some_and(|c| c.iter().any(|p| p == "tools/paddock-dev")));
    let (code, out, _) = dev_in(Some(&root), &["self", "hash"]);
    assert_eq!(code, 0);
    assert!(out.starts_with("hash "));
}

#[test]
fn graph_affected_prints_the_plan_for_the_repository_it_runs_in_or_is_pointed_at() {
    // The golden fixture repository, built in a scratch folder: no dependence on the checkout this test runs from.
    let (_keep, fixture) = crate::golden::fixture_repo();
    let root = fixture.display().to_string();
    let file = "crates/a/src/lib.rs";
    for (dir, args) in [
        (
            Some(fixture.as_path()),
            vec!["--json", "graph", "affected", "--files", file, "--explain"],
        ),
        (None, vec!["--json", "graph", "affected", "--root", &root, "--files", file]),
    ] {
        let (code, out, err) = dev_in(dir, &args);
        assert_eq!(code, 0, "{err}");
        let v: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(
            (v["ok"].clone(), v["crates"].clone(), v["all"].clone()),
            (true.into(), true.into(), false.into())
        );
        assert_eq!(v.get("why").is_some(), args.contains(&"--explain"));
    }
    let (code, out, _) = dev_in(Some(&fixture), &["graph", "affected", "--all"]);
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
