//! `./dev gen check` and the generator step of `./dev merge check` (ER-40: a generated or pinned file is regenerated, never
//! edited by hand, and drift is found where the change is made, not in CI).
//!
//! What goes stale after a merge is always a file some tool derives from other files: a lockfile, the message catalogues, the
//! notices, the workspace hack, a baseline, TypeScript from Rust. `docs/generators.toml` ([`registry`]) lists each family once,
//! with the command that checks it, the command that fixes it and the paths that mark it; this module runs the checks whose
//! paths a change touches ([`run`]) and reports drift with the regenerate command. The integrator's hand list of per-merge
//! checks is that file.
//!
//! Comparable practice (ER-37). Bazel keeps generated files out of the source tree and checks a `diff_test` per generated file;
//! Nx and turborepo declare a task's `inputs` and `outputs` globs and run only the tasks whose inputs changed; `cargo hakari
//! verify`, `cargo metadata --locked` and `npm ci` are each one tool's own check of one family. The registry takes the Nx
//! shape (inputs and outputs per entry, run what a change touches) and keeps each tool's own check as the command; it adds no
//! cache, no dependency graph and no new generator.
//!
//! Outcomes follow ADR 0089: current (exit 0), stale (exit 5, an `::error::` with the regenerate command) and not measured
//! (exit 6: a tool is missing, a required file is not on this tree, a timeout, the checkout is not the merge result). A check
//! that did not look is never a pass.

pub mod registry;
pub mod run;

use crate::merge_check::git::Git;
use paddock_cli::{CliError, Exit, Report};
use paddock_cli::{Outcome, Verdict};
use registry::{PATH, Registry};
use run::{Checked, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Arguments of `./dev gen check`.
pub struct CheckArgs {
    /// The repository root (default: found from the current folder).
    pub root: Option<PathBuf>,
    /// Run every generator, not only those a change touches.
    pub all: bool,
    /// Run only these ids.
    pub only: Vec<String>,
    /// The changed paths, instead of asking git.
    pub files: Vec<String>,
    /// The revision the change is measured against (its merge base with HEAD).
    pub base: String,
}

/// Reads and validates the registry of the repository at `root`.
pub fn load(root: &Path) -> Result<Registry, CliError> {
    let path = root.join(PATH);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        CliError::failure(format!(
            "::error::generator registry NOT MEASURED: {} cannot be read: {e}. Next step: run from a Paddock checkout, or add the registry (see the schema at the top of {PATH}).",
            path.display()
        ))
    })?;
    Registry::parse(&text).map_err(|errs| {
        CliError::failure(format!(
            "::error::{PATH} is invalid:\n  {}\nNext step: fix the entries named above; the schema is documented at the top of the file.",
            errs.join("\n  ")
        ))
    })
}

/// The exit of a set of results, folded by the shared [`Verdict`] (broken 7, stale 5, not measured 6, current 0). No result at
/// all is a run in which nothing was due: it checked nothing and says so, and is exit 0 (a whole-tree run with nothing to check
/// is [`check_with`]'s not-measured).
pub fn exit_of(results: &[Checked]) -> Exit {
    if results.is_empty() {
        return Exit::Ok;
    }
    Verdict::strict(results.iter().map(|r| (format!("gen:{}", r.id), r.outcome.clone()))).exit()
}

/// The text of a finding list: its failing messages.
fn tail_of(o: &Outcome) -> String {
    o.detail()
}

/// The result lines: one per check, and an `::error::` for each stale or unmeasured one with the next step.
pub fn lines_of(results: &[Checked]) -> Vec<String> {
    let mut out = Vec::new();
    for r in results {
        match &r.outcome {
            Outcome::Clean(_) => out.push(format!("gen {}: current ({:.1}s) [{}]", r.id, r.took.as_secs_f64(), r.line)),
            Outcome::Findings(_) => out.push(format!(
                "::error::gen {}: STALE [{}]: {}. Next step: run `{}`, review and commit the result, then `./dev gen check --only {}`.",
                r.id,
                r.line,
                tail_of(&r.outcome),
                r.regenerate,
                r.id
            )),
            other => out.push(format!(
                "::error::gen {}: NOT MEASURED: {}. Next step: make the check runnable ([{}]); an unmeasured generator is not a current one.",
                r.id,
                tail_of(other),
                r.line
            )),
        }
    }
    out
}

/// The JSON array of results.
pub fn json_of(results: &[Checked]) -> serde_json::Value {
    results
        .iter()
        .map(|r| {
            let verdict = match &r.outcome {
                Outcome::Clean(_) => "current",
                Outcome::Findings(_) => "stale",
                _ => "not measured",
            };
            let detail = if matches!(r.outcome, Outcome::Clean(_)) {
                String::new()
            } else {
                tail_of(&r.outcome)
            };
            json!({"id": r.id, "verdict": verdict, "detail": detail, "check": r.line, "regenerate": r.regenerate,
                   "cost": r.cost.name(), "seconds": r.took.as_secs_f64(), "outcome": r.outcome.json()})
        })
        .collect()
}

fn summary(due: &[&registry::Generator], total: usize) -> String {
    let ids: Vec<&str> = due.iter().map(|g| g.id.as_str()).collect();
    if due.is_empty() {
        format!("gen check: no generator is due ({total} registered; none of their paths changed)")
    } else {
        format!("gen check: {} of {total} due: {}", due.len(), ids.join(", "))
    }
}

/// Selects what is due for `changed` (or everything), runs it and builds the report. Pure but for the runner.
pub fn check_with(
    reg: &Registry,
    root: &Path,
    changed: &[String],
    all: bool,
    only: &[String],
    runner: &dyn Runner,
    log: &(dyn Fn(&str) + Sync),
) -> Result<Report, CliError> {
    for id in only {
        if reg.get(id).is_none() {
            return Err(CliError::usage(format!("No generator `{id}` in {PATH}.")).with_hint(format!(
                "Known: {}.",
                reg.generators.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(", ")
            )));
        }
    }
    // `--only` names what to run whatever the change is; otherwise everything (`--all`) or what the change touches.
    let due: Vec<&registry::Generator> = if !only.is_empty() {
        reg.generators.iter().filter(|g| only.contains(&g.id)).collect()
    } else if all {
        reg.generators.iter().collect()
    } else {
        reg.due(changed)
    };
    // A manual entry has no check: it is named, with what to do by hand, and does not change the exit.
    let (manual, due): (Vec<_>, Vec<_>) = due.into_iter().partition(|g| g.manual.is_some());
    let results = run::execute(root, &due, runner, log);
    let mut text = vec![summary(&due, reg.generators.len())];
    text.extend(lines_of(&results));
    for g in &manual {
        text.push(format!(
            "gen {}: no automatic check ({}); its paths changed, so regenerate by hand if its inputs did: `{}`",
            g.id,
            g.manual.as_deref().unwrap_or_default(),
            g.regenerate_line()
        ));
    }
    let exit = exit_of(&results);
    let data = json!({"outcome": match exit { Exit::Ok => "clean", Exit::Findings => "stale", _ => "not measured" },
                      "registered": reg.generators.len(), "due": due.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
                      "manual": manual.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
                      "results": json_of(&results)});
    let report = Report::new(data, text.join("\n"));
    Ok(if exit == Exit::Ok { report } else { report.with_exit(exit) })
}

/// `./dev gen check`.
pub fn command(a: &CheckArgs) -> Result<Report, CliError> {
    let root = crate::gates::command::repo_root(a.root.as_deref())?;
    let reg = load(&root)?;
    let changed = if a.all || !a.files.is_empty() {
        a.files.clone()
    } else {
        changed_since(&root, &a.base)?
    };
    check_with(&reg, &root, &changed, a.all, &a.only, &run::Real::new(&root), &|l| eprintln!("{l}"))
}

/// The paths changed in the working tree since the merge base of `base` and HEAD (committed, staged, unstaged and new files).
fn changed_since(root: &Path, base: &str) -> Result<Vec<String>, CliError> {
    let git = Git::new(root);
    let gap = |e: String| {
        CliError::failure(format!(
            "::error::gen check NOT MEASURED: git could not list the change: {e}. Next step: give --base <rev> that exists, or --files a,b, or --all."
        ))
    };
    let mb = git.text(&["merge-base", base, "HEAD"]).map_err(gap)?;
    let mut paths = git.paths(&["diff", "--name-only", "-z", "--no-renames", mb.trim()]).map_err(gap)?;
    paths.extend(git.paths(&["ls-files", "-z", "--others", "--exclude-standard"]).map_err(gap)?);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[cfg(test)]
mod tests;
