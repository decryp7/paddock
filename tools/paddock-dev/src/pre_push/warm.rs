//! `./dev check --warm-linux [workspace...]`: builds the Linux Clippy's dependencies in the CI image's kept volume, one container
//! run per workspace, with no push budget and the container's own output live. The first run of a workspace compiles everything
//! (see docs/benchmarks.md, "Pre-push Linux Clippy"); the push gate's row then only ever sees warm runs. It runs the push gate's own command ([`gates::linux_gate`]), so what it warms is what the row reuses; a lint it finds in
//! the workspace itself is reported (exit 5) but the dependencies are warm either way.

use super::budget::with_heavy_flags;
use super::exec::{docker_answers, heavy_command_for, on_path};
use super::gates::{self, Gate, LinuxTarget};
use super::linux_inputs::{ci_version, left_out};
use super::repo_root;
use crate::plan::{Request, plan_with_graph};
use paddock_cli::{CliError, Exit, Report};
use paddock_repo_graph::GitHistory;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The hang backstop of one workspace's run (ER-12, ER-32): about twice the largest cold run measured on the native image
/// (apps/dyno, 173.7 s; docs/benchmarks.md, "Pre-push Linux Clippy, native"), so a hung container ends, a loaded machine still fits.
const RUN_MAX: Duration = Duration::from_secs(360);
/// The wait for the machine-wide slot: one full `--warm-linux` of another agent holds it for the sum of the seven cold runs
/// measured (767.6 s), rounded up to 15 minutes.
const QUEUE_MAX: Duration = Duration::from_secs(15 * 60);
/// The hang backstop of the local CI image build (`linux-container.sh --ci-image`: the garage stage and ci-image/Dockerfile): about
/// twice the cold build measured with `--no-cache`, 379 s (docs/benchmarks.md, "Pre-push Linux Clippy, native"): 760 s.
const IMAGE_MAX: Duration = Duration::from_secs(760);

/// The workspaces to warm: the ones named (each must be a Cargo workspace folder), else every Cargo workspace.
pub fn targets(root: &Path, named: &[String], all_workspaces: &[String]) -> Result<Vec<String>, CliError> {
    if named.is_empty() {
        return Ok(all_workspaces.to_vec());
    }
    let mut out = Vec::new();
    for w in named {
        let w = w.trim_end_matches('/');
        if !root.join(w).join("Cargo.toml").is_file() {
            return Err(
                CliError::failure(format!("`{w}` is not a Cargo workspace folder (no {w}/Cargo.toml)."))
                    .with_hint("Name a folder like `crates`, `tools/paddock-dev` or `apps/dyno`."),
            );
        }
        out.push(w.to_string());
    }
    Ok(out)
}

/// One container run per workspace, the push gate's own command; `left_out` reads a workspace's Tauri crates.
pub fn runs(targets: &[String], version: &str, left_out: &dyn Fn(&str) -> Result<Vec<String>, String>) -> Result<Vec<Gate>, String> {
    targets
        .iter()
        .map(|w| {
            let t = LinuxTarget {
                ws: w.clone(),
                left_out: left_out(w)?,
            };
            Ok(gates::linux_gate(&[t], version))
        })
        .collect()
}

fn not_measured(why: &str, next: &str) -> Report {
    Report {
        data: json!({"outcome": "not measured", "why": why, "next": next}),
        text: format!("NOT MEASURED  warm-linux: {why}. Next step: {next}"),
        exit: Exit::NotMeasured,
    }
}

/// Runs `./dev check --warm-linux`.
pub fn command(root: &Option<PathBuf>, named: &[String]) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let env: HashMap<String, String> = std::env::vars().collect();
    let version = ci_version(&root).map_err(|e| CliError::failure(format!("{e}.")).with_hint("Run it from a full checkout."))?;
    let all = if named.is_empty() {
        let history = GitHistory::new(&root, Duration::from_secs(60));
        let req = Request {
            files: Vec::new(),
            all: true,
            explain: false,
            root: None,
            base: None,
            head: "HEAD".into(),
        };
        let (_, plan) = plan_with_graph(&root, &history, &req)
            .map_err(|e| CliError::failure(format!("The planner could not list the workspaces: {e}.")))?;
        gates::workspaces(&plan)
    } else {
        Vec::new()
    };
    let todo = targets(&root, named, &all)?;
    if todo.is_empty() {
        return Ok(Report::new(json!({"warmed": []}), "warm-linux: no Cargo workspace to warm."));
    }
    if !docker_answers(&env) {
        return Ok(not_measured(
            "no container runtime answers (Docker or OrbStack is not running)",
            "start it yourself (an agent never starts or restarts it, ER-14) and run `./dev check --warm-linux` again",
        ));
    }
    let built = std::sync::OnceLock::new();
    let base_prefix = match heavy_command_for(&root, &env, &|p| on_path(&env, p), &built, Duration::from_secs(600)) {
        Ok(p) => p,
        Err(e) => {
            return Ok(not_measured(
                &e,
                "make `heavy` available (it is built from tools/heavy) and run it again",
            ));
        }
    };
    let prefix = with_heavy_flags(&base_prefix, false, QUEUE_MAX, RUN_MAX);
    let mut rows = Vec::new();
    let mut worst = Exit::Ok;
    // The local CI image is its own step with its own bound: it is minutes and gigabytes, and neither a workspace's run limit nor
    // the push budget may cut it off. Built once per change to ci-image/; a probe (exit 3: not built) decides.
    let probe = paddock_proc::command("bash")
        .args([
            "tools/linux-container.sh",
            "--id",
            "paddock",
            "--ci-image",
            "--image-ready",
            "--root",
        ])
        .arg(&root)
        .current_dir(&root)
        .status();
    if matches!(probe, Ok(s) if s.code() == Some(3)) {
        eprintln!(
            "warm-linux: local CI image: building (once per change to ci-image/; limit {} s)",
            IMAGE_MAX.as_secs()
        );
        let started = Instant::now();
        let mut argv = with_heavy_flags(&base_prefix, false, QUEUE_MAX, IMAGE_MAX);
        argv.extend(["bash", "tools/linux-container.sh", "--id", "paddock", "--ci-image", "--", "true"].map(String::from));
        let status = paddock_proc::command(&argv[0]).args(&argv[1..]).current_dir(&root).status();
        let secs = started.elapsed().as_secs_f64();
        let code = status.map_or(-1, |s| s.code().unwrap_or(-1));
        eprintln!(
            "warm-linux: local CI image: {} after {secs:.1} s (exit {code})",
            if code == 0 { "built" } else { "failed" }
        );
        rows.push(json!({"workspace": "local CI image", "seconds": (secs * 10.0).round() / 10.0, "exit": code}));
        if code != 0 {
            return Ok(Report {
                data: json!({"warmed": rows}),
                text: format!(
                    "warm-linux: the local CI image build failed (exit {code}) after {secs:.1} s. Next step: read the output above (a stopped build, exit 80, hit the {} s bound)",
                    IMAGE_MAX.as_secs()
                ),
                exit: Exit::Findings,
            });
        }
    }
    let gates = runs(&todo, &version, &|ws| left_out(&root, ws)).map_err(|e| {
        CliError::failure(format!("{e}.")).with_hint("Run `cargo metadata --no-deps` in that workspace and fix what it prints.")
    })?;
    for (ws, g) in todo.iter().zip(gates) {
        eprintln!("warm-linux: {ws}: start (the container's own output follows; no budget)");
        let started = Instant::now();
        let mut argv: Vec<String> = prefix.clone();
        argv.push(g.program.clone());
        argv.extend(g.args.iter().cloned());
        let status = paddock_proc::command(&argv[0]).args(&argv[1..]).current_dir(&root).status();
        let secs = started.elapsed().as_secs_f64();
        let (word, code) = match status {
            Ok(s) if s.success() => ("warm", 0),
            Ok(s) => (
                "clippy or the build failed (dependencies compiled so far are kept)",
                s.code().unwrap_or(-1),
            ),
            Err(e) => {
                eprintln!("warm-linux: {ws}: could not start: {e}");
                ("could not start", -1)
            }
        };
        eprintln!("warm-linux: {ws}: {word} after {secs:.1} s (exit {code})");
        if code != 0 {
            worst = Exit::Findings;
        }
        rows.push(json!({"workspace": ws, "seconds": (secs * 10.0).round() / 10.0, "exit": code}));
    }
    let text = rows
        .iter()
        .map(|r| {
            format!(
                "{:<28} {:>7.1} s  exit {}",
                r["workspace"].as_str().unwrap_or(""),
                r["seconds"].as_f64().unwrap_or(0.0),
                r["exit"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut report = Report::new(json!({"warmed": rows}), format!("warm-linux:\n{text}"));
    report.exit = worst;
    Ok(report)
}
