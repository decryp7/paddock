//! `./dev check --warm-linux [workspace...]`: builds the Linux Clippy's dependencies in the CI image's kept volume, one container
//! run per workspace, with no push budget and the container's own output live. The first run of a workspace compiles everything
//! (cold `crates` 522 s, cold `apps/dyno` over 15 minutes under amd64 emulation); the push gate's row then only ever sees warm
//! runs. It runs the push gate's own command ([`gates::linux_gate`]), so what it warms is what the row reuses; a lint it finds in
//! the workspace itself is reported (exit 5) but the dependencies are warm either way.

use super::budget::with_heavy_flags;
use super::exec::{docker_answers, heavy_command_for, on_path};
use super::gates::{self, Gate};
use super::{linux_cfg, repo_root};
use crate::plan::{Request, plan_with_graph};
use paddock_cli::{CliError, Exit, Report};
use paddock_repo_graph::GitHistory;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The wait for the machine-wide slot, and the hang backstop of one workspace's run (ER-32: a maximum with a reason). The cold
/// run of the largest workspace measured about 25 minutes; three hours only ends a hung container.
const QUEUE_MAX: Duration = Duration::from_secs(3600);
const RUN_MAX: Duration = Duration::from_secs(3 * 3600);

/// The workspaces to warm: the ones named (each must be a Cargo workspace folder), else every workspace with Linux-only code.
pub fn targets(root: &Path, named: &[String], all_workspaces: &[String]) -> Result<Vec<String>, CliError> {
    if named.is_empty() {
        let all: BTreeSet<String> = all_workspaces.iter().cloned().collect();
        return Ok(linux_cfg::linux_workspaces(root, &all).into_iter().collect());
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

/// One container run per workspace, the push gate's own command.
pub fn runs(targets: &[String], image: &str) -> Vec<Gate> {
    targets.iter().map(|w| gates::linux_gate(std::slice::from_ref(w), image)).collect()
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
    let image = std::fs::read_to_string(root.join("ci-image/VERSION"))
        .map(|v| format!("dev.decryptology.net/decryp7/paddock-ci:v{}", v.trim()))
        .map_err(|e| CliError::failure(format!("ci-image/VERSION cannot be read: {e}.")).with_hint("Run it from a full checkout."))?;
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
        return Ok(Report::new(
            json!({"warmed": []}),
            "warm-linux: no workspace holds Linux-only code; nothing to warm.",
        ));
    }
    if !docker_answers(&env) {
        return Ok(not_measured(
            "no container runtime answers (Docker or OrbStack is not running)",
            "start it yourself (an agent never starts or restarts it, ER-14) and run `./dev check --warm-linux` again",
        ));
    }
    let built = std::sync::OnceLock::new();
    let prefix = match heavy_command_for(&root, &env, &|p| on_path(&env, p), &built, Duration::from_secs(600)) {
        Ok(p) => with_heavy_flags(&p, false, QUEUE_MAX, RUN_MAX),
        Err(e) => {
            return Ok(not_measured(
                &e,
                "make `heavy` available (it is built from tools/heavy) and run it again",
            ));
        }
    };
    let mut rows = Vec::new();
    let mut worst = Exit::Ok;
    for (ws, g) in todo.iter().zip(runs(&todo, &image)) {
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
