//! The `./dev gates` subcommands: argument handling and output only.

use super::{ci, edge};
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
