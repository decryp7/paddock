//! Detector 5: is the workspace-hack of the merge result up to date? Several merges of one train can each be right and
//! still leave `crates/paddock-workspace-hack/Cargo.toml` stale, because `cargo hakari generate` computes it from every
//! member's dependencies together (trains 2026-11-10-4 and 2026-11-11-5 went red on exactly that). The check writes the
//! result tree's `crates/` and `.cargo/` to a scratch folder and runs `cargo hakari generate --diff` there: exit 0 is clean,
//! exit 1 with a diff is drift, anything else (no `cargo-hakari`, no registry) is not measured, never a pass (ADR 0089).
//! cargo-hakari 0.9.39 prints the diff on STANDARD ERROR (standard output is empty), so both streams are read. The check compares
//! `paddock-workspace-hack/Cargo.toml` only; refreshing the lockfiles that list a changed crate is the author's next step and is not
//! verified here.
//! A tree without `crates/.config/hakari.toml` has nothing to check.

use super::git::Git;
use super::{Finding, Kind};
use paddock_proc::{Limits, command, run_bounded};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CONFIG: &str = "crates/.config/hakari.toml";
const TIMEOUT: Duration = Duration::from_secs(600);
/// Lines of the diff shown in a finding.
const EXCERPT_LINES: usize = 24;

/// Reads the exit code and output of `cargo hakari generate --diff` (pure, so every shape is tested): `Ok(None)` when the
/// generated file is what is committed (exit 0), `Ok(Some(lines))` for drift (exit 1 and a diff), `Err(why)` for anything else (the
/// tool could not answer). This is the probe only; the pass is made by `measure` around the whole merge check (ADR 0089).
pub fn interpret(code: Option<i32>, stdout: &str, stderr: &str) -> Result<Option<Vec<String>>, String> {
    let both = format!("{stdout}\n{stderr}");
    match code {
        Some(0) => Ok(None),
        Some(1) if both.contains("--- original") => Ok(Some(
            both.lines()
                .filter(|l| !l.trim().is_empty())
                .take(EXCERPT_LINES)
                .map(str::to_string)
                .collect(),
        )),
        other => {
            let tail: String = stderr
                .trim()
                .chars()
                .rev()
                .take(300)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Err(format!("`cargo hakari generate --diff` ended with {other:?} and no diff: {tail}"))
        }
    }
}

/// A scratch folder that is removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    /// In the system's temp folder, on purpose: the copy must sit outside every checkout, or cargo and rustup would also read the
    /// `.cargo/config.toml` and toolchain file of whichever checkout (the main one, another worktree) is above it.
    #[allow(clippy::disallowed_methods)] // conformance: scratch outside every checkout, see above
    fn new() -> Result<Scratch, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("paddock-merge-check-hakari-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make the scratch folder {}: {e}", dir.display()))?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the detector on `tree` (`files`: its blob listing); `cargo` is the program to start (a test passes a stand-in).
pub fn check(git: &Git, tree: &str, files: &HashMap<String, String>, cargo: &str) -> Result<Vec<Finding>, String> {
    if !files.contains_key(CONFIG) {
        return Ok(Vec::new());
    }
    let scratch = Scratch::new()?;
    let tarball = scratch.0.join("tree.tar");
    let tarball_arg = tarball.to_string_lossy().into_owned();
    // `.cargo/` holds the registry the lockfile names; a tree without it (a small fixture) is archived without.
    let mut args = vec!["archive", "--format=tar", "-o", &tarball_arg, tree, "crates"];
    if files.keys().any(|p| p.starts_with(".cargo/")) {
        args.push(".cargo");
    }
    // The tree's own toolchain pin, when it has one (so the result is judged with the tree's rustc, not the machine's default).
    for pin in ["rust-toolchain", "rust-toolchain.toml"] {
        if files.contains_key(pin) {
            args.push(pin);
        }
    }
    git.bytes(&args)?;
    extract(&tarball, &scratch.0)?;
    let mut cmd = command(cargo);
    cmd.args(["hakari", "generate", "--diff"]).current_dir(scratch.0.join("crates"));
    let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("`cargo hakari generate --diff` could not run: {e}"))?;
    let code = match out.outcome {
        paddock_proc::Outcome::Exited(st) => st.code(),
        _ => None,
    };
    match interpret(code, &String::from_utf8_lossy(&out.stdout), &out.stderr_text())? {
        None => Ok(Vec::new()),
        Some(excerpts) => Ok(vec![Finding {
            kind: Kind::WorkspaceHack,
            subject: "crates/paddock-workspace-hack/Cargo.toml".into(),
            detail: "does not match `cargo hakari generate` for the merged tree".into(),
            excerpts,
        }]),
    }
}

fn extract(tarball: &Path, into: &Path) -> Result<(), String> {
    let mut cmd = command("tar");
    cmd.arg("-xf").arg(tarball).arg("-C").arg(into);
    let out = run_bounded(cmd, Limits::new(TIMEOUT)).map_err(|e| format!("tar could not run: {e}"))?;
    if out.success() {
        Ok(())
    } else {
        Err(format!("tar could not unpack the tree: {}", out.stderr_text().trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_zero_is_clean() {
        assert_eq!(interpret(Some(0), "", ""), Ok(None));
    }

    /// What cargo-hakari 0.9.39 printed for a workspace-hack that lacked `base64` (captured from the real tool, 2026-11-12): the
    /// diff is on STANDARD ERROR, standard output is empty.
    const REAL_DRIFT_STDERR: &str = include_str!("../../tests/fixtures/merge-check/hakari/drift.stderr.txt");

    #[test]
    fn the_real_tools_diff_on_standard_error_is_drift_and_keeps_the_diff() {
        match interpret(Some(1), "", REAL_DRIFT_STDERR) {
            Ok(Some(lines)) => assert!(lines.iter().any(|l| l.starts_with("+base64")), "{lines:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_diff_on_standard_output_is_drift_too() {
        match interpret(Some(1), "info: \n--- original\n+++ modified\n+x = 1\n", "") {
            Ok(Some(lines)) => assert!(lines.iter().any(|l| l == "+x = 1"), "{lines:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn exit_one_without_a_diff_is_not_measured_not_a_pass_and_not_drift() {
        let v = interpret(Some(1), "", "Error: building package graph failed");
        assert!(matches!(&v, Err(w) if w.contains("building package graph")), "{v:?}");
        assert!(interpret(None, "", "").is_err());
        assert!(interpret(Some(101), "", "no such subcommand: `hakari`").is_err());
    }
}
