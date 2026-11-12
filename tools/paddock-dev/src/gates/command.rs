//! The `./dev gates` subcommands: argument handling and output only.

use super::{ci, edge, evaluate, register::Date};
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
    /// The revision the register may only shrink against.
    pub base: String,
}

fn dev_command_paths() -> std::collections::BTreeSet<String> {
    use clap::CommandFactory;
    fn walk(c: &clap::Command, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
        for s in c.get_subcommands().filter(|s| !s.is_hide_set()) {
            let path = if prefix.is_empty() {
                s.get_name().to_string()
            } else {
                format!("{prefix} {}", s.get_name())
            };
            out.insert(path.clone());
            walk(s, &path, out);
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(&crate::cli::Cli::command(), "", &mut out);
    out
}

/// `dev gates check`.
pub fn check(a: &CheckArgs) -> Result<Report, CliError> {
    let root = repo_root(a.root.as_deref())?;
    let started = std::time::Instant::now();
    // First, so that a sparse checkout is named by one line before anything reads the tree.
    if let Some(line) = edge::sparse_problem(&root) {
        return Err(CliError::failure(line));
    }
    let tree = edge::GitTree::open(&root).map_err(CliError::failure)?;
    let sources = edge::load_sources(&root).map_err(CliError::failure)?;
    let planner = edge::NodePlanner::new(&root);
    let history = edge::register_history(&root, &a.base);
    let input = evaluate::Input {
        tree: &tree,
        sources: &sources,
        conformance_list: edge::conformance_json(&root, "list"),
        conformance_commands: edge::conformance_json(&root, "commands").ok(),
        dev_commands: dev_command_paths(),
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
    let mut text = String::new();
    for f in &out.findings {
        text.push_str(&f.annotation());
        text.push('\n');
    }
    text.push_str(&format!(
        "\nC100: {} checks, {} findings ({} ms)\n\n{table}",
        out.rows.len(),
        out.findings.len(),
        started.elapsed().as_millis()
    ));
    let findings: Vec<Value> = out
        .findings
        .iter()
        .map(|f| json!({"file": f.file, "line": f.line, "id": f.id, "message": f.message, "fix": f.fix}))
        .collect();
    let report = Report::new(json!({"findings": findings, "checks": out.rows.len(), "warn": a.warn}), text);
    Ok(if out.findings.is_empty() || a.warn {
        report
    } else {
        report.with_exit(paddock_cli::Exit::Findings)
    })
}

/// `dev gates list --json`: every check with its gating.
pub fn list(root: Option<&Path>, base: &str) -> Result<Report, CliError> {
    let root = repo_root(root)?;
    let tree = edge::GitTree::open(&root).map_err(CliError::failure)?;
    let sources = edge::load_sources(&root).map_err(CliError::failure)?;
    let planner = edge::NodePlanner::new(&root);
    let input = evaluate::Input {
        tree: &tree,
        sources: &sources,
        conformance_list: edge::conformance_json(&root, "list"),
        conformance_commands: None,
        dev_commands: dev_command_paths(),
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
    let refusals = super::register::release_refusals(register.as_deref(), base_text.as_deref(), tag);
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
