//! `dev check --pre-push` as a real process on this repository's tree, with fake `cargo` and no `cargo-xwin` on PATH (ER-14: no
//! real lint runs): the same gates as CI for a change to `crates/`, the exit codes, and one line per failure with its next step.
// windows-twin: none: the fake tools are shell scripts; the planner and gate table are unit-tested on every OS in src/pre_push
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

const CHANGE: &str = "crates/paddock-proc/src/lib.rs";

/// A folder with a `cargo` that lets `metadata` through to the real one (the planner reads the graph with it) and answers every
/// other call from `script`; the PATH of the run is that folder plus the system's, which has no cargo-xwin and no docker.
fn fake_bin(dir: &Path, script: &str) -> String {
    let real = std::env::var("CARGO").unwrap();
    let real_dir = Path::new(&real).parent().unwrap().display().to_string();
    let cargo = format!("#!/bin/sh\ncase \"$1\" in metadata) PATH=\"{real_dir}:$PATH\" exec \"{real}\" \"$@\";; esac\n{script}\n");
    let path = dir.join("cargo");
    paddock_testkit::exe::write_program(&path, cargo).unwrap();
    format!("{}:/usr/bin:/bin", dir.display())
}

/// The fake cargo runs without the machine-wide slot: `PADDOCK_HEAVY=off`, as a machine only its user uses would set it.
fn dev(path: &str, args: &[&str]) -> (i32, String, String) {
    dev_heavy(path, args, Some("off"), true)
}

#[allow(clippy::disallowed_methods)]
fn dev_heavy(path: &str, args: &[&str], heavy: Option<&str>, test_off: bool) -> (i32, String, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = crate::dev_bin::command()
        .args(["check", "--pre-push", "--base", "HEAD", "--files", CHANGE])
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env_remove("PADDOCK_HEAVY")
        .envs(heavy.map(|h| ("PADDOCK_HEAVY", h)))
        .envs(test_off.then_some(("PADDOCK_TEST_HEAVY_OFF", "1")))
        // An empty cache: the heavy tool is built (by the fake cargo, which builds nothing) and never taken from the user's.
        .env(
            "PADDOCK_DEV_HOME",
            path.split(':').next().unwrap_or("/nonexistent").to_string() + "/home",
        )
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_dry_run_lists_the_gates_ci_runs_for_a_crates_change_and_why_others_are_left_out() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "exit 0");
    let (code, out, err) = dev(&path, &["--dry-run"]);
    assert_eq!(code, 0, "{err}{out}");
    for want in [
        "run   fmt crates",
        "run   lockfile crates",
        "run   clippy crates",
        "run   clippy-windows crates",
        "skip  clippy-linux crates",
        "skip  commit-lint",
        // Conformance is in every plan (fix/lint-no-fixed-test-waits, b013e1142).
        "run   conformance",
        "run   ipc-endpoints",
    ] {
        assert!(out.contains(want), "{want} missing:\n{out}");
    }
    assert!(
        out.contains("cargo clippy --workspace --all-targets --all-features --locked -- -D warnings"),
        "{out}"
    );
}

/// The gates-register row runs on every push (ER-41), including one that changes only a file C100 reads through its own globs.
#[test]
fn the_gates_check_is_planned_for_a_crates_change_and_for_an_app_contract_change() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "exit 0");
    for files in [CHANGE, "apps/dyno/paddock.ci.json"] {
        let (code, out, err) = dev(&path, &["--dry-run", "--files", files]);
        assert_eq!(code, 0, "{err}{out}");
        assert!(out.contains("run   gates-register"), "{files}: {out}");
    }
}

#[test]
fn a_failing_fmt_is_exit_5_with_one_error_line_and_the_rerun_command() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "echo 'Diff in lib.rs' >&2; exit 1");
    let (code, out, err) = dev(&path, &["--only", "fmt"]);
    assert_eq!(code, 5, "{err}{out}");
    let line = out
        .lines()
        .find(|l| l.contains("::error::fmt crates"))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        line.contains("(exit 1)") && line.contains("Next step:") && line.contains("Re-run alone: cd crates && cargo fmt --all --check"),
        "{line}"
    );
    assert!(out.contains("Diff in lib.rs") && out.contains("do not push"), "{out}");
}

#[test]
fn without_xwin_the_windows_lint_is_not_measured_exit_6_never_a_pass() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "exit 0");
    let (code, out, err) = dev(&path, &["--only", "clippy-windows"]);
    assert_eq!(code, 6, "{err}{out}");
    assert!(
        out.contains("NOT MEASURED  clippy-windows crates") && out.contains("cargo install cargo-xwin"),
        "{out}"
    );
    assert!(!out.contains("ok   "), "nothing passed: {out}");
}

#[test]
fn everything_that_runs_green_is_exit_0() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "exit 0");
    let (code, out, err) = dev(&path, &["--only", "fmt,clippy"]);
    assert_eq!(code, 0, "{err}{out}");
    assert!(
        out.contains("ok            fmt crates") && out.contains("ok            clippy crates"),
        "{out}"
    );
}

#[test]
fn when_heavy_cannot_be_built_the_compiles_are_not_measured_exit_6_never_a_pass() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "exit 0");
    let (code, out, err) = dev_heavy(&path, &["--only", "clippy"], None, false);
    assert_eq!(code, 6, "{err}{out}");
    assert!(
        out.contains("NOT MEASURED  clippy crates") && out.contains("no heavy slot tool"),
        "{out}"
    );
    assert!(!out.contains("ok            clippy"), "{out}");
}

#[test]
fn heavy_off_without_the_test_variable_is_an_override_and_not_measured() {
    let d = tempfile::tempdir().unwrap();
    let path = fake_bin(d.path(), "exit 0");
    let (code, out, err) = dev_heavy(&path, &["--only", "clippy"], Some("off"), false);
    assert_eq!(code, 6, "{err}{out}");
    assert!(out.starts_with("OVERRIDE SET") || out.contains("\nOVERRIDE SET"), "{out}");
    assert!(out.contains("NOT MEASURED  clippy crates"), "{out}");
}

/// The hook path: git runs `pre-push` with `GIT_DIR` set to the pushing worktree's git folder. The run plans the pushed commit in a
/// throw-away worktree of it, so every git call there must read that worktree, not the pushing one (Paddock train 14, 2026-11-17: a
/// push of a commit that was not HEAD failed with "ui/projects/paddock-ui/scroll/ng-package.json could not be read", a file only
/// HEAD of the pushing worktree had, because `git ls-files` answered from `GIT_DIR`'s index).
#[test]
fn a_push_of_a_commit_that_is_not_head_plans_that_commit_even_with_git_dir_set_as_the_hook_sets_it() {
    let d = tempfile::tempdir().unwrap();
    let git = |dir: &Path, args: &[&str]| -> String {
        let out = paddock_proc::command("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let main = d.path().join("main");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "base"]);
    std::fs::write(main.join("a.txt"), "1\n").unwrap();
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-q", "-m", "base"]);
    let base = git(&main, &["rev-parse", "HEAD"]);
    // The pushed commit: one more plain file.
    git(&main, &["checkout", "-q", "-b", "pushed"]);
    std::fs::write(main.join("b.txt"), "2\n").unwrap();
    git(&main, &["add", "b.txt"]);
    git(&main, &["commit", "-q", "-m", "pushed"]);
    let pushed = git(&main, &["rev-parse", "HEAD"]);
    // The pushing worktree: HEAD is another commit, with a package.json that the pushed commit lacks.
    let wt = d.path().join("wt");
    git(&main, &["worktree", "add", "-q", "-b", "other", wt.to_str().unwrap(), &base]);
    std::fs::create_dir_all(wt.join("ui/x")).unwrap();
    std::fs::write(wt.join("ui/x/ng-package.json"), "{}\n").unwrap();
    git(&wt, &["add", "ui/x/ng-package.json"]);
    git(&wt, &["commit", "-q", "-m", "other"]);
    let gitdir = git(&wt, &["rev-parse", "--absolute-git-dir"]);

    let mut child = crate::dev_bin::command()
        .args([
            "check",
            "--pre-push",
            "--refs-from-stdin",
            "--dry-run",
            "--base",
            &base,
            "--remote",
            "origin",
        ])
        .args(["--root", wt.to_str().unwrap()])
        .current_dir(&wt)
        .env("GIT_DIR", &gitdir)
        .env("PADDOCK_PREPUSH_WORKTREES", d.path().join("work"))
        .env("PADDOCK_DEV_HOME", d.path().join("home"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let line = format!("refs/heads/pushed {pushed} refs/heads/x {}\n", "0".repeat(40));
        child.stdin.take().unwrap().write_all(line.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    assert!(!text.contains("could not be read"), "{text}");
}

/// A site that is not the planner: `worktree list` reads the worktrees of the repository it runs in. With the hook's `GIT_DIR`
/// naming another repository it must still list this one's.
#[test]
fn worktree_list_reads_its_own_repository_when_git_dir_names_another_one() {
    let d = tempfile::tempdir().unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let out = paddock_proc::command("git")
            .args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let (a, b) = (d.path().join("a"), d.path().join("b"));
    for (dir, branch) in [(&a, "only-in-a"), (&b, "only-in-b")] {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", branch]);
        std::fs::write(dir.join("f.txt"), "x\n").unwrap();
        git(dir, &["add", "--all"]);
        git(dir, &["commit", "-q", "-m", "one"]);
    }
    let run = |git_dir: Option<&Path>| {
        let mut cmd = crate::dev_bin::command();
        cmd.args(["worktree", "list", "--json"]).current_dir(&b);
        if let Some(g) = git_dir {
            cmd.env("GIT_DIR", g);
        }
        let out = cmd.output().unwrap();
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    };
    let plain = run(None);
    assert!(
        plain.contains("only-in-b"),
        "positive control: the plain run lists this repository: {plain}"
    );
    let hooked = run(Some(&a.join(".git")));
    assert!(
        hooked.contains("only-in-b") && !hooked.contains("only-in-a"),
        "GIT_DIR changed what is listed: {hooked}"
    );
}
