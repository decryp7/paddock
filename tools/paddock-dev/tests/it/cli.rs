//! The command line contract: one JSON object with --json, stable exit codes, help and errors that name the next step.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::process::Command;

fn dev(args: &[&str]) -> (i32, String, String) {
    dev_in(None, args)
}

/// Runs `dev` in `dir` (the process's own folder when `None`): a test that reads a repository gives it one of its own.
fn dev_in(dir: Option<&std::path::Path>, args: &[&str]) -> (i32, String, String) {
    let mut cmd = crate::dev_bin::command();
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
    let root = paddock_env::winpath::real(d.path()).unwrap();
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
fn self_hash_outside_a_repository_or_without_an_input_list_names_the_next_step() {
    let d = tempfile::tempdir().unwrap();
    let run = |dir: &std::path::Path| {
        let out = crate::dev_bin::command()
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
fn plan_refuses_json_mentions_since_run_in_its_usage_and_annotates_errors_in_a_workflow() {
    let (code, _, err) = dev(&["--json", "plan", "--all"]);
    assert_eq!(code, 2);
    assert!(err.contains("no --json mode"), "{err}");
    let out = crate::dev_bin::command()
        .args(["plan", "--nope"])
        .env("GITHUB_ACTIONS", "true")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        err.contains("::error::dev plan: unknown option --nope") && err.contains("--since-run"),
        "{err}"
    );
}

/// ADR 0098 WP-4: everything `--trial` can refuse is refused before the scratch clone is made and before `trial/<branch>` is
/// pushed (the scratch folder stays empty, so no git ran at all).
#[test]
fn a_trial_with_bad_arguments_is_refused_before_anything_is_cloned_or_pushed() {
    let scratch = tempfile::tempdir().unwrap();
    let run = |extra: &[&str]| {
        let mut cmd = crate::dev_bin::command();
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        cmd.args(["ci", "dispatch", "ci.yml", "--trial", "feat/x", "--root"])
            .arg(&repo)
            .args(extra)
            .env("GITEA_TOKEN", "not-a-real-token")
            .env("PADDOCK_TRIAL_DIR", scratch.path())
            // A machine with no derivable train reservation (CI's runners, run 8208): bad arguments are refused before the
            // reservation is read, so the answer does not depend on where the tool runs.
            .env_remove("PADDOCK_TRAIN_CI_LOCK_ROOT")
            .current_dir(scratch.path());
        let out = cmd.output().unwrap();
        (out.status.code().unwrap(), String::from_utf8_lossy(&out.stderr).into_owned())
    };
    for (extra, word) in [
        (&["--max", "soon"][..], "duration"),
        (&["--max", "30m", "--input", "novalue"], "key=value"),
        (&["--max", "0s"], "out of range"),
    ] {
        let (code, err) = run(extra);
        assert_eq!(code, 2, "{extra:?}: {err}");
        assert!(err.contains(word), "{extra:?}: {err}");
    }
    let mut cmd = crate::dev_bin::command();
    cmd.args(["ci", "dispatch", "ci.yml", "--trial", "bad branch;x", "--max", "30m"])
        .env("GITEA_TOKEN", "not-a-real-token")
        .env("PADDOCK_TRIAL_DIR", scratch.path());
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        std::fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "a clone was made before the arguments were checked"
    );
}
