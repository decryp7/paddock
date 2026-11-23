//! The `./dev gates` subcommands: argument handling and output only.

use super::{ci, edge, evaluate, register, register::Date, time_based};
use crate::input_hash;
use paddock_cli::{CliError, Report};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The repository root: the given one, or git's top level from the current folder.
pub fn repo_root(given: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(r) = given {
        return Ok(r.to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    let out = input_hash::Exec::run(&input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
    Ok(PathBuf::from(out.trim()))
}

/// `dev gates model`.
pub fn model(root: Option<&Path>) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let src = edge::load_sources(&root).map_err(CliError::failure)?;
    let m = ci::build(&src);
    let lines: Vec<Value> = m
        .lines
        .iter()
        .map(|l| {
            json!({
                "file": l.file, "line": l.line, "root": l.root, "job": l.job, "app": l.app, "os": l.os, "cwd": l.cwd,
                "text": l.text, "origin": l.origin, "planned": l.planned,
                "levels": l.levels.iter().map(|(k, v)| (k.name().to_string(), format!("{v:?}"))).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    let text = format!(
        "{} commands, {} roots, {} workflows and {} actions reached, {} problems",
        m.lines.len(),
        m.roots.len(),
        m.workflows.len(),
        m.actions.len(),
        m.problems.len()
    );
    Ok(Report::new(
        json!({"lines": lines, "roots": m.roots, "workflows": m.workflows.keys().collect::<Vec<_>>(), "actions": m.actions.keys().collect::<Vec<_>>(), "problems": m.problems}),
        text,
    ))
}

/// Arguments of `dev gates check`.
pub struct CheckArgs {
    /// The root.
    pub root: Option<PathBuf>,
    /// A release tag about to be made.
    pub release_tag: Option<String>,
    /// Report findings but exit clean.
    pub warn: bool,
    /// Time-based conditions block everything (the scheduled job, a release).
    pub strict: bool,
    /// Only print the reminders for entries due within this many days.
    pub upcoming: Option<i64>,
    /// The revision the register may only shrink against.
    pub base: String,
}

/// `dev gates check`.
/// `dev_commands` is the command tree of `./dev` (every visible subcommand path), handed in by the one place that owns it, `cli`: this
/// module never imports `cli`, which imports every command (conformance C155 follows import edges).
pub fn check(a: &CheckArgs, dev_commands: std::collections::BTreeSet<String>) -> Result<Report, CliError> {
    let root = repo_root(a.root.as_deref())?;
    if let Some(days) = a.upcoming {
        return upcoming(&root, days);
    }
    let started = std::time::Instant::now();
    // First, so that a sparse checkout is named by one line before anything reads the tree.
    if let Some(line) = edge::sparse_problem(&root) {
        return Err(CliError::failure(line));
    }
    let tree = edge::GitTree::open(&root).map_err(CliError::failure)?;
    let sources = edge::load_sources(&root).map_err(CliError::failure)?;
    let planner = edge::planner(&root);
    let history = edge::register_history(&root, &a.base);
    let input = evaluate::Input {
        tree: &tree,
        sources: &sources,
        conformance_list: edge::conformance_json(&root, "list"),
        conformance_commands: edge::conformance_json(&root, "commands").ok(),
        dev_commands,
        register_text: std::fs::read_to_string(root.join("docs/gates.toml")).ok(),
        base_register_text: edge::base_register(&root, &a.base),
        history: history.clone().unwrap_or_default(),
        history_error: history.as_ref().err().cloned(),
        today: Date::today(),
        release_tag: a.release_tag.clone(),
        planner: &planner,
        warn: a.warn,
        shallow: edge::is_shallow(&root),
    };
    let out = evaluate::evaluate(&input);
    let table = evaluate::summary(&out.rows);
    // Time-based conditions (ER-41): a push warns about an expired entry it does not touch; a release tag and `--strict` block all.
    let mode = if a.strict || a.release_tag.is_some() {
        time_based::Mode::Strict
    } else {
        time_based::Mode::Push
    };
    let reg = register::parse(input.register_text.as_deref().unwrap_or("")).0;
    let base_reg = input.base_register_text.as_deref().map(|t| register::parse(t).0);
    let touched = time_based::touched(&reg, base_reg.as_ref());
    let split = time_based::split(out.findings.clone(), mode, &touched);
    let mut text = String::new();
    for f in &split.warnings {
        text.push_str(&f.warning());
        text.push('\n');
    }
    for f in &split.blocking {
        text.push_str(&f.annotation());
        text.push('\n');
    }
    text.push_str(&format!(
        "\nC100: {} checks, {} findings, {} time-based warnings ({} ms)\n\n{table}",
        out.rows.len(),
        split.blocking.len(),
        split.warnings.len(),
        started.elapsed().as_millis()
    ));
    let finding_json = |f: &evaluate::Finding| json!({"file": f.file, "line": f.line, "id": f.id, "message": f.message, "fix": f.fix});
    let findings: Vec<Value> = split.blocking.iter().map(finding_json).collect();
    let warnings: Vec<Value> = split.warnings.iter().map(finding_json).collect();
    let report = Report::new(
        json!({"findings": findings, "warnings": warnings, "checks": out.rows.len(), "warn": a.warn}),
        text,
    );
    Ok(if split.blocking.is_empty() || a.warn {
        report
    } else {
        report.with_exit(paddock_cli::Exit::Findings)
    })
}

/// `dev gates check --upcoming`: the reminders for entries whose review date falls within `days` days (ER-41). Reads only the
/// register; it never fails on a finding.
fn upcoming(root: &Path, days: i64) -> Result<Report, CliError> {
    let text = std::fs::read_to_string(root.join("docs/gates.toml")).map_err(|e| {
        CliError::failure(format!(
            "docs/gates.toml cannot be read: {e}. Run from the repository root or pass --root."
        ))
    })?;
    let (reg, errors) = register::parse(&text);
    if let Some(e) = errors.first() {
        return Err(CliError::failure(format!(
            "docs/gates.toml is not valid ({e}), so no date can be read. Fix it, then run `./dev gates check`."
        )));
    }
    let lines = time_based::reminders(&reg, Date::today(), days);
    let body = if lines.is_empty() {
        format!("C100: no review date falls within {days} day(s)\n")
    } else {
        lines.join("\n") + "\n"
    };
    Ok(Report::new(json!({"upcoming": lines, "within_days": days}), body))
}

/// `dev gates list --json`: every check with its gating.
/// `dev_commands`: as for [`check`].
pub fn list(root: Option<&Path>, base: &str, dev_commands: std::collections::BTreeSet<String>) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let tree = edge::GitTree::open(&root).map_err(CliError::failure)?;
    let sources = edge::load_sources(&root).map_err(CliError::failure)?;
    let planner = edge::planner(&root);
    let input = evaluate::Input {
        tree: &tree,
        sources: &sources,
        conformance_list: edge::conformance_json(&root, "list"),
        conformance_commands: None,
        dev_commands,
        register_text: std::fs::read_to_string(root.join("docs/gates.toml")).ok(),
        base_register_text: edge::base_register(&root, base),
        history: Vec::new(),
        history_error: None,
        today: Date::today(),
        release_tag: None,
        planner: &planner,
        warn: true,
        shallow: false,
    };
    let out = evaluate::evaluate(&input);
    let rows: Vec<Value> = out
        .rows
        .iter()
        .map(|r| {
            json!({
                "kind": r.check.kind.name(), "id": r.check.id, "file": r.check.file, "line": r.check.line, "state": r.state,
                "gated": r.gating.gated.iter().map(|l| l.name()).collect::<Vec<_>>(),
                "conditional": r.gating.conditional.iter().map(|l| l.name()).collect::<Vec<_>>(),
                "report_only": r.gating.report_only.iter().map(|l| l.name()).collect::<Vec<_>>(),
                "missing_apps": r.gating.missing_apps, "unresolved": r.gating.unresolved,
            })
        })
        .collect();
    let text = evaluate::summary(&out.rows);
    Ok(Report::new(json!({"checks": rows}), text))
}

/// `dev gates inputs`: the globs whose files change C100's answer, for the planner (ADR 0060 §4).
pub fn inputs() -> Result<Report, CliError> {
    let globs = super::inputs::GLOBS;
    Ok(Report::new(json!({"globs": globs}), globs.join("\n")))
}

/// `dev gates ratchet --release-tag <tag>`: only the lint-adoption ratchets of the tag's app, nothing else of C100 (ER-31).
pub fn ratchet(root: Option<&Path>, base: &str, tag: &str) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let register = std::fs::read_to_string(root.join("docs/gates.toml")).ok();
    let base_text = edge::base_register(&root, base);
    let mut refusals = super::register::release_refusals(register.as_deref(), base_text.as_deref(), tag);
    // A review date that has passed blocks a tag and a release, whoever touched the entry (ER-41).
    refusals.extend(time_based::release_refusals(register.as_deref(), Date::today()));
    let mut text = String::new();
    for r in &refusals {
        text.push_str(&format!("::error file=docs/gates.toml::C100 ratchet {r}\n"));
    }
    let reports = super::register::release_reports(register.as_deref(), tag);
    for r in &reports {
        text.push_str(&format!("::warning file=docs/gates.toml::C100 ratchet {r}\n"));
    }
    if refusals.is_empty() {
        text.push_str(&format!("C100 ratchet: tag {tag} is not refused by any lint-adoption ratchet\n"));
    }
    let report = Report::new(json!({"tag": tag, "refusals": refusals, "reports": reports}), text);
    Ok(if refusals.is_empty() {
        report
    } else {
        report.with_exit(paddock_cli::Exit::Findings)
    })
}
