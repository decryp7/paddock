//! `dev check --pre-push` as a real process on this repository's tree, with fake `cargo` and no `cargo-xwin` on PATH (ER-14: no
//! real lint runs): the same gates as CI for a change to `crates/`, the exit codes, and one line per failure with its next step.
// windows-twin: none: the fake tools are shell scripts; the planner and gate table are unit-tested on every OS in src/pre_push
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

const CHANGE: &str = "crates/paddock-proc/src/lib.rs";

/// A folder with a `cargo` that lets `metadata` through to the real one (the planner reads the graph with it) and answers every
/// other call from `script`; the PATH of the run is that folder plus the system's, which has no cargo-xwin and no docker.
fn fake_bin(dir: &Path, script: &str) -> String {
    let real = std::env::var("CARGO").unwrap();
    let real_dir = Path::new(&real).parent().unwrap().display().to_string();
    let cargo = format!("#!/bin/sh\ncase \"$1\" in metadata) PATH=\"{real_dir}:$PATH\" exec \"{real}\" \"$@\";; esac\n{script}\n");
    let path = dir.join("cargo");
    std::fs::write(&path, cargo).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:/usr/bin:/bin", dir.display())
}

#[allow(clippy::disallowed_methods)]
fn dev(path: &str, args: &[&str]) -> (i32, String, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["check", "--pre-push", "--base", "HEAD", "--files", CHANGE])
        .args(args)
        .current_dir(root)
        .env("PATH", path)
        .env_remove("PADDOCK_HEAVY")
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
