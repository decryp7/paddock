//! `dev check --pre-push` as a real process on this repository's tree, with fake `cargo` and no `cargo-xwin` on PATH (ER-14: no
//! real lint runs): the same gates as CI for a change to `crates/`, the exit codes, and one line per failure with its next step.
// windows-twin: none: the fake tools are shell scripts; the planner and gate table are unit-tested on every OS in src/pre_push
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::TestCommand;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const CHANGE: &str = "crates/paddock-proc/src/lib.rs";

/// A folder with a `cargo` that lets `metadata` through to the real one (the planner reads the graph with it) and answers every
/// other call from `script`; the PATH of the run is that folder plus the system's, which has no cargo-xwin and no docker.
fn fake_bin(dir: &Path, script: &str) -> String {
    // conformance: test-env — only the path of the real cargo is written into the stand-in's script; no process starts here
    let real = std::env::var("CARGO").unwrap();
    let real_dir = Path::new(&real).parent().unwrap().display().to_string();
    let cargo = format!("#!/bin/sh\ncase \"$1\" in metadata) PATH=\"{real_dir}:$PATH\" exec \"{real}\" \"$@\";; esac\n{script}\n");
    let path = dir.join("cargo");
    std::fs::write(&path, cargo).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:/usr/bin:/bin", dir.display())
}

/// The fake cargo runs without the machine-wide slot: `PADDOCK_HEAVY=off`, as a machine only its user uses would set it.
fn dev(path: &str, args: &[&str]) -> (i32, String, String) {
    dev_heavy(path, args, Some("off"), true)
}

#[allow(clippy::disallowed_methods)]
fn dev_heavy(path: &str, args: &[&str], heavy: Option<&str>, test_off: bool) -> (i32, String, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"))
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
        // A named engineering-rules folder with no heavy.sh is the only place `dev` looks: the machine's own checkout is not found.
        .env(
            "ENGINEERING_RULES",
            path.split(':').next().unwrap_or("/nonexistent").to_string() + "/no-engineering-rules",
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

/// Every name a run of `./dev` started by a test may carry (ADR 0100): what the process builder lists, plus this file's own
/// named additions (`PADDOCK_DEV_HOME`, `PADDOCK_HEAVY`, `PADDOCK_TEST_HEAVY_OFF`, `ENGINEERING_RULES`) and what `dev` itself sets for its gates (`CARGO_BUILD_JOBS`).
fn listed(name: &str) -> bool {
    const NAMED: &[&str] = &[
        "PADDOCK_DEV_HOME",
        "PADDOCK_HEAVY",
        "PADDOCK_TEST_HEAVY_OFF",
        "ENGINEERING_RULES",
        "CARGO_BUILD_JOBS",
        "PWD",
        "OLDPWD",
        "SHLVL",
        "_",
    ];
    paddock_testkit::process::allowed_names().contains(&name) || NAMED.contains(&name)
}

/// ER-14, ADR 0100: a test run never reaches the machine through the environment. The machine here has the real slot tool
/// (`ENGINEERING_RULES`, `HEAVY_LOCK_DIR`) and an ambient variable; the child must not see any of them, so no slot is taken
/// and the fake cargo's environment holds only listed names.
#[test]
fn a_run_never_sees_the_machines_environment_or_takes_a_real_slot() {
    let mut env = paddock_testkit::env_lock();
    let d = tempfile::tempdir().unwrap();
    let rules = d.path().join("rules");
    std::fs::create_dir_all(rules.join("scripts")).unwrap();
    let marker = d.path().join("real-heavy-ran");
    let script = rules.join("scripts/heavy.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\nexec \"$@\"\n", marker.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let slots = d.path().join("slots");
    env.set("ENGINEERING_RULES", &rules);
    env.set("HEAVY_LOCK_DIR", &slots);
    env.set("PADDOCK_ENV_CANARY", "1");
    let envlog = d.path().join("envlog");
    let path = fake_bin(d.path(), &format!("env >> {}; exit 0", envlog.display()));
    let (code, out, err) = dev_heavy(&path, &["--only", "clippy"], None, false);
    let seen = std::fs::read_to_string(&envlog).unwrap_or_default();
    assert!(!marker.exists(), "the machine's heavy.sh ran (exit {code}): {err}{out}");
    assert!(!slots.exists(), "a machine-wide slot folder was made (ER-14)");
    let unlisted: Vec<&str> = seen
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, _)| k)
        .filter(|k| !listed(k))
        .collect();
    assert!(unlisted.is_empty(), "the child saw variables nobody listed: {unlisted:?}");
}
