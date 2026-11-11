//! Detector 5: is the workspace-hack of the merge result up to date? Several merges of one train can each be right and
//! still leave `crates/paddock-workspace-hack/Cargo.toml` stale, because `cargo hakari generate` computes it from every
//! member's dependencies together (trains 2026-11-10-4 and 2026-11-11-5 went red on exactly that). The check writes the
//! result tree's `crates/` and `.cargo/` to a scratch folder and runs `cargo hakari generate --diff` there: exit 0 is clean,
//! exit 1 with a diff is drift, anything else (no `cargo-hakari`, no registry) is not measured, never a pass (ADR 0089).
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

/// What `cargo hakari generate --diff` said.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Exit 0: the generated file is what is committed.
    Clean,
    /// Exit 1 and a diff on standard output.
    Drift(Vec<String>),
    /// Anything else: the tool could not answer.
    NotMeasured(String),
}

/// Reads the exit code and output of `cargo hakari generate --diff` (pure, so every shape is tested).
pub fn interpret(code: Option<i32>, stdout: &str, stderr: &str) -> Verdict {
    match code {
        Some(0) => Verdict::Clean,
        Some(1) if stdout.contains("--- original") => Verdict::Drift(
            stdout
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(EXCERPT_LINES)
                .map(str::to_string)
                .collect(),
        ),
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
            Verdict::NotMeasured(format!("`cargo hakari generate --diff` ended with {other:?} and no diff: {tail}"))
        }
    }
}

/// A scratch folder that is removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    /// Inside the repository's git folder (explicit, on the repository's own disk, never a system temp folder).
    fn new(git: &Git) -> Result<Scratch, String> {
        let gitdir = git.text(&["rev-parse", "--absolute-git-dir"])?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = Path::new(gitdir.trim()).join(format!("paddock-merge-check-hakari-{}-{nanos}", std::process::id()));
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
    let scratch = Scratch::new(git)?;
    let tarball = scratch.0.join("tree.tar");
    let tarball_arg = tarball.to_string_lossy().into_owned();
    // `.cargo/` holds the registry the lockfile names; a tree without it (a small fixture) is archived without.
    let mut args = vec!["archive", "--format=tar", "-o", &tarball_arg, tree, "crates"];
    if files.keys().any(|p| p.starts_with(".cargo/")) {
        args.push(".cargo");
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
    match interpret(code, &String::from_utf8_lossy(&out.stdout), &out.stderr_text()) {
        Verdict::Clean => Ok(Vec::new()),
        Verdict::NotMeasured(why) => Err(why),
        Verdict::Drift(excerpts) => Ok(vec![Finding {
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
        assert_eq!(interpret(Some(0), "", ""), Verdict::Clean);
    }

    #[test]
    fn exit_one_with_a_diff_is_drift_and_keeps_the_diff() {
        let diff = "info: \n--- original\n+++ modified\n@@ -1,5 +1,6 @@\n+base64 = { version = \"0.23\" }\n";
        match interpret(Some(1), diff, "") {
            Verdict::Drift(lines) => assert!(lines.iter().any(|l| l.starts_with("+base64")), "{lines:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn exit_one_without_a_diff_is_not_measured_not_a_pass_and_not_drift() {
        let v = interpret(Some(1), "", "Error: building package graph failed");
        assert!(
            matches!(&v, Verdict::NotMeasured(w) if w.contains("building package graph")),
            "{v:?}"
        );
        assert!(matches!(interpret(None, "", ""), Verdict::NotMeasured(_)));
        assert!(matches!(
            interpret(Some(101), "", "no such subcommand: `hakari`"),
            Verdict::NotMeasured(_)
        ));
    }
}
