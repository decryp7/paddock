//! `./dev check --pre-push` (ADR 0038 §3, docs/plans/ci-in-rust.md WP-2, cut to ER-9's fast gates): the gates CI would run for
//! what this push changes, run here first, in parallel, bounded by the machine's load (ER-24).
//!
//! - [`gates`]: the plan and the changed files become gates (pure).
//! - [`ci_line`]: the conformance command, read from ci.yml in this one place.
//! - [`exec`]: running them, with a fake for tests.
//!
//! One table at the end: what ran, what was skipped and why, what could not be measured (ER-33: never a pass), and for each
//! failure one line with its cause and the next step. Exit 0 all passed or skipped; 5 something failed; 6 only "not measured";
//! 7 a check is broken (beats the others).

pub mod ci_line;
pub mod exec;
pub mod gates;

use crate::plan::{Request, plan_with_graph};
use crate::plan_repo::git_text;
use clap::Args;
use exec::{RealExec, Verdict, parse_load, run_all, workers};
use gates::{Change, Facts};
use paddock_cli::{CliError, Exit, Report};
use paddock_repo_graph::GitHistory;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The arguments of `./dev check --pre-push`.
#[derive(Args)]
pub struct PrePushArgs {
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// The base the push is measured from (default: origin/main).
    #[arg(long, default_value = "origin/main")]
    pub base: String,
    /// Only the gates whose row starts with one of these words (for example `fmt,clippy`); the hook always runs them all.
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,
    /// Print the gates and their commands; run nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Take these repository-relative files as the change instead of asking git.
    #[arg(long, value_delimiter = ',')]
    pub files: Vec<String>,
}

const GIT_TIMEOUT: Duration = Duration::from_secs(120);

/// The command.
pub fn command(args: &PrePushArgs) -> Result<Report, CliError> {
    let root = match &args.root {
        Some(r) => r.clone(),
        None => {
            let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
            git_text(&cwd, &["rev-parse", "--show-toplevel"])
                .map(|t| PathBuf::from(t.trim()))
                .ok_or_else(|| CliError::failure("This is not a git checkout.").with_hint("Run it inside the repository."))?
        }
    };
    let env: HashMap<String, String> = std::env::vars().collect();
    let real = RealExec {
        root: root.clone(),
        env: env.clone(),
    };
    let change = discover(&root, &args.base, &args.files)?;
    if change.files.is_empty() && change.range.is_none() {
        return Ok(Report::new(
            json!({"gates": [], "changed": 0}),
            format!("pre-push: nothing changed since {}; nothing to check.", args.base),
        ));
    }
    let history = GitHistory::new(&root, GIT_TIMEOUT);
    let req = Request {
        files: change.files.clone(),
        all: false,
        explain: false,
        root: None,
        base: None,
        head: "HEAD".into(),
    };
    let (_, plan) = plan_with_graph(&root, &history, &req).map_err(|e| {
        CliError::failure(format!("The planner could not plan this change: {e}."))
            .with_hint("Run `./dev plan --files <a,b>` to see the same error.")
    })?;
    let facts = read_facts(&root, &plan, &env);
    let mut planned = gates::plan_gates(&plan, &change, &facts);
    if !args.only.is_empty() {
        planned.retain(|p| {
            let id = match p {
                gates::Planned::Run(g) => &g.id,
                gates::Planned::Skip { id, .. } | gates::Planned::Broken { id, .. } => id,
            };
            args.only.iter().any(|w| id.split(' ').next() == Some(w.as_str()))
        });
    }
    if args.dry_run {
        return Ok(dry_run(&planned));
    }
    let load = uptime_load(&real);
    let n = workers(load, std::thread::available_parallelism().map_or(1, |n| n.get()));
    let mut head = format!(
        "pre-push: {} changed file(s) against {}; {} gate(s) at a time (1-minute load {})",
        change.files.len(),
        args.base,
        n,
        load.map_or("unknown".to_string(), |l| format!("{l:.1}"))
    );
    if env.get("PADDOCK_HEAVY").is_none_or(|h| h.trim().is_empty()) {
        head.push_str(
            "\npre-push: PADDOCK_HEAVY is not set: compiles run without the machine-wide slot (CARGO_BUILD_JOBS=2 still applies)",
        );
    }
    let rows = run_all(&planned, &real, n);
    Ok(render(&head, &rows))
}

fn dry_run(planned: &[gates::Planned]) -> Report {
    let mut text = String::new();
    let mut data = Vec::new();
    for p in planned {
        match p {
            gates::Planned::Run(g) => {
                text.push_str(&format!("run   {:<34} {}\n", g.id, g.rerun()));
                data.push(json!({"id": g.id, "action": "run", "command": g.rerun(), "heavy": g.heavy}));
            }
            gates::Planned::Skip { id, reason } => {
                text.push_str(&format!("skip  {id:<34} {reason}\n"));
                data.push(json!({"id": id, "action": "skip", "reason": reason}));
            }
            gates::Planned::Broken { id, cause, .. } => {
                text.push_str(&format!("BROKEN {id:<33} {cause}\n"));
                data.push(json!({"id": id, "action": "broken", "reason": cause}));
            }
        }
    }
    Report::new(json!({ "dry_run": true, "gates": data }), text)
}

/// The table, one line per row, and the exit code (see the module doc). A failure's line carries its cause and next step.
pub fn render(head: &str, rows: &[(String, Verdict)]) -> Report {
    let mut text = format!("{head}\n");
    let (mut ok, mut fail, mut skip, mut unmeasured, mut broken) = (0, 0, 0, 0, 0);
    let mut data = Vec::new();
    for (id, v) in rows {
        let (line, kind) = match v {
            Verdict::Passed { secs } => {
                ok += 1;
                (format!("ok            {id:<34} {secs:.1} s"), "passed")
            }
            Verdict::Skipped { reason } => {
                skip += 1;
                (format!("skip          {id:<34} {reason}"), "skipped")
            }
            Verdict::NotMeasured { reason, next } => {
                unmeasured += 1;
                (format!("NOT MEASURED  {id:<34} {reason}. Next: {next}"), "not measured")
            }
            Verdict::Failed { cause, tail, next, secs } => {
                fail += 1;
                let shown = if tail.is_empty() {
                    String::new()
                } else {
                    format!("\n    {}", tail.replace('\n', "\n    "))
                };
                (
                    format!("FAIL          {id:<34} {secs:.1} s\n  ::error::{id}: {cause} Next step: {next}{shown}"),
                    "failed",
                )
            }
            Verdict::Broken { cause, next } => {
                broken += 1;
                (format!("CHECK BROKEN  {id:<34} {cause}. Next: {next}"), "broken")
            }
        };
        text.push_str(&line);
        text.push('\n');
        data.push(json!({"id": id, "verdict": kind}));
    }
    text.push_str(&format!(
        "pre-push: {ok} passed, {fail} failed, {unmeasured} not measured, {broken} broken, {skip} skipped. {}",
        if fail + unmeasured + broken == 0 {
            "Safe to push."
        } else {
            "Not proven: do not push."
        }
    ));
    let exit = if broken > 0 {
        Exit::CheckBroken
    } else if fail > 0 {
        Exit::Findings
    } else if unmeasured > 0 {
        Exit::NotMeasured
    } else {
        Exit::Ok
    };
    Report::new(
        json!({ "gates": data, "passed": ok, "failed": fail, "not_measured": unmeasured, "broken": broken, "skipped": skip }),
        text,
    )
    .with_exit(exit)
}

/// The change: files since the merge base (committed, modified, untracked), and the commits to lint.
fn discover(root: &Path, base: &str, files: &[String]) -> Result<Change, CliError> {
    let mb = git_text(root, &["merge-base", base, "HEAD"])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            CliError::failure(format!("There is no merge base of {base} and HEAD.")).with_hint(format!(
                "Run `git fetch origin` or pass --base <ref> (this check measures the push from {base})."
            ))
        })?;
    let lines = |t: Option<String>| {
        t.unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let mut changed = if files.is_empty() {
        let tracked = git_text(root, &["diff", "--name-only", &mb])
            .ok_or_else(|| CliError::failure(format!("git diff against {mb} failed.")).with_hint("Run it by hand to see why."))?;
        let mut all = lines(Some(tracked));
        all.extend(lines(git_text(root, &["ls-files", "--others", "--exclude-standard"])));
        all
    } else {
        files.to_vec()
    };
    changed.sort();
    changed.dedup();
    let count = git_text(root, &["rev-list", "--count", &format!("{mb}..HEAD")])
        .and_then(|t| t.trim().parse::<u32>().ok())
        .unwrap_or(0);
    Ok(Change {
        files: changed,
        range: (count > 0).then(|| format!("{mb}..HEAD")),
    })
}

/// What the repository says that the plan does not: ci.yml's conformance line, the apps that lint Windows, the CI image.
fn read_facts(root: &Path, plan: &Value, env: &HashMap<String, String>) -> Facts {
    let ci = std::fs::read_to_string(root.join(".gitea/workflows/ci.yml"))
        .map_err(|e| format!("ci.yml cannot be read: {e}"))
        .and_then(|t| ci_line::conformance_args(&t));
    let mut windows = std::collections::BTreeSet::new();
    for ws in gates::workspaces(plan) {
        let Some(app) = ws.strip_prefix("apps/").and_then(|r| r.split('/').next()) else {
            continue;
        };
        let contract = std::fs::read_to_string(root.join(format!("apps/{app}/paddock.ci.json")))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if contract.is_some_and(|c| c["windowsClippy"] == true) {
            windows.insert(ws);
        }
    }
    let image = std::fs::read_to_string(root.join("ci-image/VERSION"))
        .map(|v| format!("dev.decryptology.net/decryp7/paddock-ci:v{}", v.trim()))
        .map_err(|e| format!("ci-image/VERSION cannot be read: {e}"));
    let mut tests: Vec<String> = std::fs::read_dir(root.join("apps/pitwall/scripts/ci"))
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".test.mjs"))
                .map(|n| format!("scripts/ci/{n}"))
                .collect()
        })
        .unwrap_or_default();
    tests.sort();
    Facts {
        conformance: Some(ci),
        windows_clippy: windows,
        image: Some(image),
        pitwall_tests: tests,
        dev_exe: std::env::current_exe().map_or_else(|_| "./dev".into(), |p| p.to_string_lossy().to_string()),
        target_dir: env.get("CARGO_TARGET_DIR").cloned(),
    }
}

fn uptime_load(exec: &RealExec) -> Option<f64> {
    use crate::ci_tool::ToolCall;
    let ran = ToolCall::new("uptime")
        .timeout(Duration::from_secs(10))
        .run(&exec.env, &mut std::io::sink())
        .ok()?;
    parse_load(&ran.stdout)
}

#[cfg(test)]
mod tests;
