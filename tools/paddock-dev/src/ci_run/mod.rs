//! `dev ci run <project> <phase>`: the gate executor (ADR 0038 §1, CI-in-Rust WP-3). It runs the commands of one phase of a
//! project's `paddock.ci.json` ([`contract`]), the port of `conformance/ci/run-commands.sh` (which `app-gates` ran) for every
//! project: ci.yml's root jobs, the app legs and `./dev check --pre-push` read the same files.
//!
//! The rules are the script's, case for case:
//!
//! - one command per line, blank lines skipped, run from the repository root;
//! - a `cd <folder>` line holds for the lines after it (a plain folder; a `cd` with a variable, quote or operator is refused:
//!   it could not hold);
//! - each line runs in its own `bash -euo pipefail -c <line>` (the script's `( set -e; eval line )` under `set -uo pipefail`),
//!   with the job's whole environment, as the step's shell had it; so `$((..))`, pipes, `VAR=1 cmd` and `$E2E_INDEX` work;
//! - the first failing line ends the phase with its exit code, unless `--keep-going` (every line runs, the exit is 1).
//!
//! What it adds (ER-27, ER-32, ER-33): every line is logged with its duration and how it ended (`ran:`, with the NTSTATUS in hex for
//! a Windows crash code), a failure is one `::error::` line with the line, the folder and the next step, no line runs unbounded
//! (`--max-secs`, a line's whole process tree is ended at the limit through `paddock_proc`), a phase that does not exist is an error,
//! and a Ctrl+C or SIGTERM ends the running line's tree. The conformance project's `conformance` field becomes
//! `PADDOCK_CONFORMANCE_ARGS` for every line (the one copy of the check list). `E2E_INDEX` is derived from `E2E_SHARD` (`i/N`
//! gives `i`), as the end-to-end step did. `--heavy` puts each line through the machine-wide slot (ER-24), as the pre-push gate does.

pub mod contract;

use crate::ci_tool::{describe_status, error_line};
use clap::Args;
use paddock_proc::{Outcome, Sink, run_waited_cancellable};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A line's limit when the caller sets none: well above the longest gate (the crates coverage run), well below a job's own limit.
pub const DEFAULT_MAX_SECS: u64 = 10_800;

/// `dev ci run`.
#[derive(Args, Debug)]
pub struct RunArgs {
    /// The project: an app under apps/, or crates, ui, conformance, tools, icons, eslint.
    pub project: String,
    /// The phase: a field of the project's paddock.ci.json that lists commands (prepare, checks, ui, ...).
    pub phase: String,
    /// Run every line even after one failed (the exit code is then 1); the default stops at the first failure.
    #[arg(long)]
    pub keep_going: bool,
    /// Print the lines of the phase, one per line with the folder they run in, and run nothing.
    #[arg(long)]
    pub list: bool,
    /// The most seconds one line may run before its process tree is ended.
    #[arg(long, default_value_t = DEFAULT_MAX_SECS)]
    pub max_secs: u64,
    /// Put every line through the machine-wide heavy slot (ER-24): `PADDOCK_HEAVY`, else `heavy.sh` or `heavy` on PATH.
    #[arg(long)]
    pub heavy: bool,
    /// The repository root (default: the folder with a `.git` above the current one).
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// One step of a phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A `cd` to this folder (relative to the root, normalised).
    Cd(String),
    /// A command, with the folder it runs in (relative to the root; empty for the root).
    Cmd {
        /// The shell line.
        text: String,
        /// The folder it runs in.
        cwd: String,
    },
}

/// The lines of a phase after the `cd`s are applied: what runs where. `Err` for a `cd` that cannot be held.
pub fn expand(lines: &[String]) -> Result<Vec<Line>, String> {
    let mut cwd = String::new();
    let mut out = Vec::new();
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(target) = line.strip_prefix("cd ").map(str::trim) {
            if target.is_empty() || target.contains(['$', '"', '\'', '`', '\\', ';', '&', '|', '<', '>', '(', ')', '*', '?', ' ']) {
                return Err(format!(
                    "`{line}`: only a plain folder can be held by a cd line (no variable, quote, space or operator); write the path out"
                ));
            }
            cwd = join(&cwd, target)?;
            out.push(Line::Cd(cwd.clone()));
            continue;
        }
        out.push(Line::Cmd {
            text: line.to_string(),
            cwd: cwd.clone(),
        });
    }
    Ok(out)
}

/// `base` then `target`, normalised (`..` resolved); a path that leaves the repository root is refused.
fn join(base: &str, target: &str) -> Result<String, String> {
    if target.starts_with('/') || target.starts_with('~') || target.chars().nth(1) == Some(':') {
        return Err(format!("`cd {target}` leaves the repository: use a folder below the root"));
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in target.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if p == ".." {
            if parts.pop().is_none() {
                return Err(format!("`cd {target}` leaves the repository: use a folder below the root"));
            }
        } else {
            parts.push(p);
        }
    }
    Ok(parts.join("/"))
}

/// The repository root above `start`: the nearest folder with a `.git` (a folder or the file of a worktree).
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// `dev ci run`: a CI step's contract (plain lines, `::error::` on failure, the failing line's exit code).
pub fn run(args: &RunArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env: HashMap<String, String> = std::env::vars().collect();
    let stop = Arc::new(AtomicBool::new(false));
    crate::pre_push::stop_on_signals(&stop);
    run_with(args, &env, &stop, out, err)
}

/// [`run`] with the environment and the stop flag given (tests).
pub fn run_with(args: &RunArgs, env: &HashMap<String, String>, stop: &Arc<AtomicBool>, out: &mut dyn Write, _err: &mut dyn Write) -> i32 {
    let fail = |out: &mut dyn Write, cause: String, next: &str| {
        let _ = writeln!(
            out,
            "{}",
            error_line(&format!("ci run {} {}: {cause}", args.project, args.phase), next)
        );
        1
    };
    let Some(root) = args
        .root
        .clone()
        .or_else(|| std::env::current_dir().ok().and_then(|d| find_root(&d)))
    else {
        return fail(
            out,
            "no repository root (a folder with .git) above the current folder".into(),
            "run it inside the checkout, or pass --root",
        );
    };
    let contract = match contract::load(&root, &args.project) {
        Ok(c) => c,
        Err(e) => {
            return fail(
                out,
                e,
                "check the project name and that its paddock.ci.json is in the checkout (sparse checkouts need the folder)",
            );
        }
    };
    let lines = match contract::phase_lines(&contract, &args.project, &args.phase).and_then(|l| expand(&l)) {
        Ok(l) => l,
        Err(e) => return fail(out, e, "fix the phase in the project's paddock.ci.json"),
    };
    let mut child_env = env.clone();
    if args.project == "conformance" && contract.get("conformance").is_some() {
        match contract::conformance_args(&contract) {
            Ok(a) => {
                child_env.insert("PADDOCK_CONFORMANCE_ARGS".into(), a[1..].join(" "));
            }
            Err(e) => return fail(out, e, "fix the `conformance` object in conformance/paddock.ci.json"),
        }
    }
    if let Some(shard) = env.get("E2E_SHARD").filter(|s| !s.is_empty()) {
        child_env.insert("E2E_INDEX".into(), shard.split('/').next().unwrap_or("").to_string());
    }
    if args.list {
        for l in &lines {
            match l {
                Line::Cd(dir) => {
                    let _ = writeln!(out, "cd {dir}");
                }
                Line::Cmd { text, cwd } => {
                    let _ = writeln!(out, "[{}] {text}", if cwd.is_empty() { "." } else { cwd });
                }
            }
        }
        return 0;
    }
    let heavy = if args.heavy { heavy_prefix(env) } else { Vec::new() };
    if args.heavy && heavy.is_empty() {
        return fail(
            out,
            "--heavy asked for the machine-wide slot, but neither PADDOCK_HEAVY nor `heavy.sh` or `heavy` is available".into(),
            "install tools/heavy (`cargo install --path tools/heavy --locked`), or run without --heavy",
        );
    }
    execute(args, &root, &lines, &child_env, &heavy, stop, out)
}

fn heavy_prefix(env: &HashMap<String, String>) -> Vec<String> {
    crate::pre_push::exec::heavy_prefix(env, &|p| crate::pre_push::exec::on_path(env, p))
}

fn execute(
    args: &RunArgs,
    root: &Path,
    lines: &[Line],
    env: &HashMap<String, String>,
    heavy: &[String],
    stop: &Arc<AtomicBool>,
    out: &mut dyn Write,
) -> i32 {
    let started = Instant::now();
    let (mut ran, mut failed, mut first_code) = (0usize, 0usize, 0i32);
    for l in lines {
        let (text, cwd) = match l {
            Line::Cd(dir) => {
                let _ = writeln!(out, "+ cd {dir}");
                if !root.join(dir).is_dir() {
                    let cause = format!("ci run {} {}: command failed (exit 1): cd {dir}", args.project, args.phase);
                    let _ = writeln!(
                        out,
                        "{}",
                        error_line(&cause, "the folder is not in this checkout (a sparse checkout or a stale contract)")
                    );
                    return 1;
                }
                continue;
            }
            Line::Cmd { text, cwd } => (text, cwd),
        };
        if stop.load(Ordering::SeqCst) {
            let _ = writeln!(
                out,
                "{}",
                error_line(
                    &format!("ci run {} {}: cancelled before `{text}`", args.project, args.phase),
                    "re-run the job"
                )
            );
            return 130;
        }
        let dir = root.join(cwd);
        let (code, how) = run_line(text, &dir, env, heavy, args.max_secs, stop, out);
        ran += 1;
        if code != 0 {
            failed += 1;
            if first_code == 0 {
                first_code = code;
            }
            let shown = if cwd.is_empty() { ".".to_string() } else { cwd.clone() };
            let cause = format!("command failed ({how}): {text} (in {shown})");
            let next = format!(
                "run `{text}` in {shown} to reproduce, or `./dev ci run {} {}`",
                args.project, args.phase
            );
            let _ = writeln!(out, "{}", error_line(&cause, &next));
            if !args.keep_going {
                return code;
            }
        }
    }
    let _ = writeln!(
        out,
        "ci run {} {}: {ran} command(s), {failed} failed, {:.1} s",
        args.project,
        args.phase,
        started.elapsed().as_secs_f64()
    );
    if failed == 0 { 0 } else { 1 }
}

/// Runs one line; the exit code (124 for the time limit, 130 cancelled, 127 not started) and how it ended.
fn run_line(
    text: &str,
    dir: &Path,
    env: &HashMap<String, String>,
    heavy: &[String],
    max_secs: u64,
    stop: &Arc<AtomicBool>,
    out: &mut dyn Write,
) -> (i32, String) {
    let _ = writeln!(out, "::group::{text}");
    let _ = out.flush();
    let mut argv: Vec<String> = heavy.to_vec();
    argv.extend(["bash", "-euo", "pipefail", "-c", text].map(String::from));
    let mut cmd = paddock_proc::command(&argv[0]);
    cmd.args(&argv[1..]).current_dir(dir).env_clear().envs(env);
    let start = Instant::now();
    let result = run_waited_cancellable(cmd, Duration::from_secs(max_secs), Sink::Inherit, stop);
    let secs = start.elapsed().as_secs_f64();
    let (code, how) = match result {
        Ok((Outcome::Exited(s), _)) => (s.code().unwrap_or(1), describe_status(s)),
        Ok((_, true)) => (130, "cancelled".to_string()),
        Ok((_, false)) => (124, format!("timed out after {max_secs} s; the line's process tree was ended")),
        Err(e) => (127, format!("not started: {e}")),
    };
    let _ = writeln!(out, "ran: {text} ({how}, {secs:.1} s)");
    let _ = writeln!(out, "::endgroup::");
    (code, how)
}

#[cfg(test)]
mod tests;
