//! The CI contract of a project (`paddock.ci.json`): ADR 0038 §1, the one source of CI's commands. An app's is
//! `apps/<app>/paddock.ci.json`, a root project's (`crates`, `ui`, `conformance`, `tools`, `icons`, `eslint`) is
//! `<project>/paddock.ci.json`.
//!
//! A *phase* is a top-level field whose value is an array of strings: the commands of that phase, one per line, run from the
//! repository root; a `cd <folder>` line holds for the lines after it (the `run-commands.sh` rules). Every other field is the
//! project's own data (`cargo`, `os`, `notices`, ...); the conformance project's `conformance` object holds the checks Paddock runs
//! on itself, the one copy that CI's conformance step, the merge queue's train gates and `./dev check --pre-push` all read.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// The projects that are not apps: a folder of the repository root with its own contract.
pub const ROOT_PROJECTS: [&str; 6] = ["crates", "ui", "conformance", "tools", "icons", "eslint"];

/// The contract file of `project` under `root`: a root project's own folder, else the app's.
pub fn path_of(root: &Path, project: &str) -> PathBuf {
    if ROOT_PROJECTS.contains(&project) {
        root.join(project).join("paddock.ci.json")
    } else {
        root.join("apps").join(project).join("paddock.ci.json")
    }
}

/// The repository path (forward slashes) of the contract file of `project`.
pub fn repo_path_of(project: &str) -> String {
    if ROOT_PROJECTS.contains(&project) {
        format!("{project}/paddock.ci.json")
    } else {
        format!("apps/{project}/paddock.ci.json")
    }
}

/// The contract of `project`, parsed, or why it cannot be (`Err` is the whole explanation).
pub fn load(root: &Path, project: &str) -> Result<Value, String> {
    let path = path_of(root, project);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{} cannot be read ({e}). Projects: an app under apps/ or one of {}",
            repo_path_of(project),
            ROOT_PROJECTS.join(", ")
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", repo_path_of(project)))
}

/// The commands of `phase` in a parsed contract: the strings of its array (blank lines dropped by the executor, not here).
pub fn phase_lines(contract: &Value, project: &str, phase: &str) -> Result<Vec<String>, String> {
    let Some(v) = contract.get(phase) else {
        let known: Vec<&str> = phases(contract);
        return Err(format!(
            "{} has no phase `{phase}` (phases: {})",
            repo_path_of(project),
            if known.is_empty() { "none".to_string() } else { known.join(", ") }
        ));
    };
    let Some(items) = v.as_array() else {
        return Err(format!("{}: `{phase}` is not an array of command lines", repo_path_of(project)));
    };
    items
        .iter()
        .map(|i| {
            i.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{}: `{phase}` holds a value that is not a string", repo_path_of(project)))
        })
        .collect()
}

/// The names of the phases of a contract: its fields that are arrays of strings (not the app-only lists that are data).
pub fn phases(contract: &Value) -> Vec<&str> {
    contract
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, v)| !DATA_LISTS.contains(&k.as_str()) && v.as_array().is_some_and(|a| a.iter().all(Value::is_string)))
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// Array-of-strings fields of an app contract that are data for the planner and the harness, not command lists.
pub const DATA_LISTS: [&str; 3] = ["os", "processPatterns", "$comment"];

/// The arguments after `node` of the conformance command Paddock runs on itself, from `conformance/paddock.ci.json`'s
/// `conformance` object: `conformance/bin/paddock-conformance.mjs --app <app> --root <root> --only <ids> [--strict <ids>]`.
pub fn conformance_args(contract: &Value) -> Result<Vec<String>, String> {
    let c = contract
        .get("conformance")
        .and_then(Value::as_object)
        .ok_or("conformance/paddock.ci.json has no `conformance` object")?;
    let text = |k: &str| {
        c.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("conformance/paddock.ci.json: `conformance.{k}` is not a non-empty string"))
    };
    let ids = |k: &str| -> Result<Vec<String>, String> {
        c.get(k)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("conformance/paddock.ci.json: `conformance.{k}` is not an array of check ids"))?
            .iter()
            .map(|i| {
                i.as_str()
                    .filter(|s| !s.is_empty() && !s.contains(',') && !s.contains(char::is_whitespace))
                    .map(str::to_string)
                    .ok_or_else(|| format!("conformance/paddock.ci.json: `conformance.{k}` holds an invalid check id"))
            })
            .collect()
    };
    let only = ids("only")?;
    if only.is_empty() {
        return Err("conformance/paddock.ci.json: `conformance.only` is empty (no check would run: ER-33)".into());
    }
    let strict = ids("strict")?;
    let mut args = vec![
        "conformance/bin/paddock-conformance.mjs".to_string(),
        "--app".into(),
        text("app")?.into(),
        "--root".into(),
        text("root")?.into(),
        "--only".into(),
        only.join(","),
    ];
    if !strict.is_empty() {
        args.push("--strict".into());
        args.push(strict.join(","));
    }
    Ok(args)
}

/// [`conformance_args`] read from the repository at `root`.
pub fn conformance_args_at(root: &Path) -> Result<Vec<String>, String> {
    conformance_args(&load(root, "conformance")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_phase_is_an_array_of_strings_and_data_lists_are_not_phases() {
        let c = json!({"os": ["a"], "prepare": ["x"], "cargo": ".", "checks": [], "bad": [1]});
        assert_eq!(phases(&c), vec!["prepare", "checks"]);
        assert_eq!(phase_lines(&c, "p", "prepare").unwrap(), vec!["x"]);
        assert!(phase_lines(&c, "p", "nope").unwrap_err().contains("phases: prepare, checks"));
        assert!(phase_lines(&c, "p", "cargo").unwrap_err().contains("not an array"));
        assert!(phase_lines(&c, "p", "bad").unwrap_err().contains("not a string"));
    }

    #[test]
    fn the_conformance_arguments_are_built_from_the_fields() {
        let c = json!({"conformance": {"app": "paddock", "root": ".", "only": ["C06", "C14"], "strict": ["C36"]}});
        assert_eq!(
            conformance_args(&c).unwrap().join(" "),
            "conformance/bin/paddock-conformance.mjs --app paddock --root . --only C06,C14 --strict C36"
        );
        let no_strict = json!({"conformance": {"app": "p", "root": ".", "only": ["C06"], "strict": []}});
        assert!(!conformance_args(&no_strict).unwrap().contains(&"--strict".to_string()));
    }

    #[test]
    fn an_empty_or_malformed_conformance_list_is_an_error_never_a_pass() {
        let empty = json!({"conformance": {"app": "p", "root": ".", "only": [], "strict": []}});
        assert!(conformance_args(&empty).unwrap_err().contains("ER-33"));
        assert!(conformance_args(&json!({})).is_err());
        let comma = json!({"conformance": {"app": "p", "root": ".", "only": ["C06,C14"], "strict": []}});
        assert!(conformance_args(&comma).unwrap_err().contains("invalid check id"));
    }

    #[test]
    fn root_projects_and_apps_have_their_own_paths() {
        assert_eq!(repo_path_of("ui"), "ui/paddock.ci.json");
        assert_eq!(repo_path_of("dyno"), "apps/dyno/paddock.ci.json");
    }
}
