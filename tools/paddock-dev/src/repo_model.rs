//! The repository model in the planner (ADR 0117 WP-1): the facts the model needs, read from the tree the planner already read
//! ([`crate::plan::RepoGraph`]) and the inputs file ([`crate::repo_inputs::load`], the one accessor); the file texts through one
//! batched git reader ([`GitBulk`], `git cat-file --batch-command`, one process for every read); and `--compare`, which sets the
//! model's answer beside the rule tables' answer project by project (ADR 0117 §8, the migration net).
//!
//! The rule tables still decide every plan until WP-5; this module decides nothing CI runs.

use crate::plan::{RepoGraph, Request};
use paddock_repo_graph::History;
use paddock_repo_graph::model::extract::BulkRead;
use paddock_repo_graph::model::{Detected, Facts, Reach, Reached, reach};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The root projects and their folders: the plan's project keys that are folders. `c99` is a check job with no folder; it is
/// reached only as a named input's target.
pub const ROOT_PROJECTS: [(&str, &str); 8] = [
    ("crates", "crates"),
    ("ui", "ui"),
    ("eslint", "eslint"),
    ("conformance", "conformance"),
    ("tools", "tools"),
    ("icons", "icons"),
    ("ci-image", "ci-image"),
    ("paddock-dev", "tools/paddock-dev"),
];

/// Every project id with its folder: the root projects and each app (`app:<name>` = `apps/<name>`).
pub fn projects(graph: &RepoGraph) -> Vec<(String, String)> {
    ROOT_PROJECTS
        .iter()
        .map(|(id, dir)| (id.to_string(), dir.to_string()))
        .chain(graph.apps.iter().map(|a| (format!("app:{a}"), format!("apps/{a}"))))
        .collect()
}

/// A git command with the planner's policy (stable output, no hooks, a scrubbed environment).
fn git(root: &Path) -> std::process::Command {
    let mut cmd = crate::git_env::git();
    cmd.arg("-C")
        .arg(root)
        .args(["-c", "core.quotePath=false", "-c", "core.fsmonitor=false"]);
    let parent: Vec<String> = std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()).collect();
    crate::child_env::scrub(&mut cmd, &parent);
    cmd
}

/// The one bulk reader of file texts at a revision (or the working tree), over one `git cat-file --batch-command` process
/// (in process through gix where it can): no process per file (ER-20).
pub struct GitBulk {
    reader: RefCell<paddock_git::Reader>,
    rev: Option<String>,
    root: PathBuf,
}

impl GitBulk {
    /// A reader of `rev`'s blobs, or of the working tree when `rev` is `None`.
    pub fn new(root: &Path, rev: Option<&str>) -> GitBulk {
        let r = root.to_path_buf();
        GitBulk {
            reader: RefCell::new(paddock_git::Reader::open(root, move || git(&r))),
            rev: rev.map(str::to_string),
            root: root.to_path_buf(),
        }
    }

    /// The same reader with every read through `git cat-file --batch-command` (no in-process gix): the spawn-budget test's
    /// worst case.
    pub fn through_git(root: &Path, rev: Option<&str>) -> GitBulk {
        let r = root.to_path_buf();
        GitBulk {
            reader: RefCell::new(paddock_git::Reader::new(move || git(&r))),
            rev: rev.map(str::to_string),
            root: root.to_path_buf(),
        }
    }

    /// How many git processes it started (the spawn budget test reads it).
    pub fn starts(&self) -> usize {
        self.reader.borrow().starts()
    }

    /// One file's bytes at `rev` through the same reader (`None`: absent).
    pub fn bytes_at(&self, rev: &str, path: &str) -> Option<Vec<u8>> {
        self.reader.borrow_mut().file_at(rev, path).ok().flatten()
    }

    /// One file's text at `rev` through the same reader (`None`: absent or not UTF-8).
    pub fn text_at(&self, rev: &str, path: &str) -> Option<String> {
        self.bytes_at(rev, path).and_then(|b| String::from_utf8(b).ok())
    }
}

impl BulkRead for GitBulk {
    fn read_all(&self, paths: &[String]) -> Vec<Option<String>> {
        match &self.rev {
            None => paths.iter().map(|p| std::fs::read_to_string(self.root.join(p)).ok()).collect(),
            Some(rev) => {
                let items: Vec<(&str, &str)> = paths.iter().map(|p| (rev.as_str(), p.as_str())).collect();
                self.reader
                    .borrow_mut()
                    .files_at(&items)
                    .into_iter()
                    .map(|r| r.ok().flatten().and_then(|b| String::from_utf8(b).ok()))
                    .collect()
            }
        }
    }
}

/// The model's reach for a planner request on a graph already read: the same changed files the rule tables see, the inputs file
/// through its accessor, and the changed files' texts through one batched reader.
pub fn reach_for(root: &Path, history: &dyn History, req: &Request, graph: &RepoGraph) -> Result<(Vec<String>, Reached), String> {
    let inputs = crate::repo_inputs::load(root)?;
    let files = history.tracked_files().map_err(|e| e.to_string())?;
    let changed = if req.all {
        Vec::new()
    } else if let Some(base) = &req.base {
        history.changed_files(base, &req.head).map_err(|e| e.to_string())?
    } else {
        req.files.clone()
    };
    let projects = projects(graph);
    let bulk = GitBulk::new(root, None);
    // Content detection is an aid only (D14): documentation assets under doc owners, and the kind of an unowned file.
    let detect = |f: &str| -> Option<Detected> {
        let bytes = match &req.base {
            Some(_) => bulk.bytes_at(&req.head, f),
            None => std::fs::read(root.join(f)).ok(),
        };
        bytes.map(|b| paddock_repo_graph::model::detect(&b))
    };
    let facts = Facts {
        cargo: &graph.cargo,
        npm: &graph.npm,
        projects: &projects,
        files: &files,
        detect: Some(&detect),
        model: None,
        seeds: &[],
    };
    let text = |f: &str| match &req.base {
        Some(base) => (bulk.text_at(base, f), bulk.text_at(&req.head, f)),
        None => (bulk.text_at("HEAD", f), std::fs::read_to_string(root.join(f)).ok()),
    };
    let mut reached = reach(&changed, &facts, &inputs, &text);
    if req.all {
        reached.everything = true;
        for (id, _) in &projects {
            reached.nodes.insert(
                id.clone(),
                paddock_repo_graph::model::NodeReach {
                    reach: Reach::Code,
                    rust: true,
                    ui: true,
                    why: vec!["everything (no base)".to_string()],
                },
            );
        }
    }
    Ok((changed, reached))
}

/// The plan key of a project id (`paddock-dev` is `paddockDev`, `ci-image` is `ciImage`); apps are listed under `apps`.
fn plan_key(id: &str) -> &str {
    match id {
        "paddock-dev" => "paddockDev",
        "ci-image" => "ciImage",
        other => other,
    }
}

/// What one side ran for a project: nothing, or which halves (`rust`, `ui`, `all`).
fn halves_of(rust: bool, ui: bool) -> &'static str {
    match (rust, ui) {
        (true, true) => "all",
        (true, false) => "rust",
        (false, true) => "ui",
        (false, false) => "all",
    }
}

/// The rule tables' answer, project by project: id to the halves it runs (`all` for a root project).
fn old_runs(plan: &Value, graph: &RepoGraph) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (id, _) in projects(graph).iter().filter(|(id, _)| !id.starts_with("app:")) {
        if plan[plan_key(id)] == true {
            out.insert(id.clone(), "all".to_string());
        }
    }
    if plan["c99"] == true {
        out.insert("c99".to_string(), "all".to_string());
    }
    for a in plan["apps"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let kinds: BTreeSet<&str> = plan["kinds"][a]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let k = if kinds.contains("all") || (kinds.contains("rust") && kinds.contains("ui")) || kinds.is_empty() {
            "all"
        } else if kinds.contains("rust") {
            "rust"
        } else {
            "ui"
        };
        out.insert(format!("app:{a}"), k.to_string());
    }
    out
}

/// The model's answer in the same shape: every project reached above `docs`, with its halves. Conformance is in every plan on both
/// sides (a planner output fact, not a reach).
fn new_runs(reached: &Reached) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = reached
        .nodes
        .iter()
        .filter(|(id, n)| n.reach > Reach::Docs && id.as_str() != "docs")
        .map(|(id, n)| {
            let h = if id.starts_with("app:") { halves_of(n.rust, n.ui) } else { "all" };
            (id.clone(), h.to_string())
        })
        .collect();
    if reached.everything {
        out.insert("c99".to_string(), "all".to_string());
    }
    out.insert("conformance".to_string(), "all".to_string());
    out
}

/// `--compare`: the projects where the model and the rule tables differ, each with both answers and both reasons. `dropped` lists
/// the projects the model runs less of (ADR 0117 §8: a narrowing names what it no longer runs).
pub fn compare(plan: &Value, reached: &Reached, graph: &RepoGraph) -> Value {
    let old = old_runs(plan, graph);
    let new = new_runs(reached);
    let mut differences = Vec::new();
    let mut dropped = Vec::new();
    let ids: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    for id in ids {
        let (o, n) = (old.get(id), new.get(id));
        if o == n {
            continue;
        }
        let narrower = match (o.map(String::as_str), n.map(String::as_str)) {
            (Some(_), None) => true,
            (Some("all"), Some(h)) => h != "all",
            _ => false,
        };
        if narrower {
            dropped.push(json!(id));
        }
        let why_new: Vec<String> = reached.nodes.get(id.as_str()).map(|n| n.why.clone()).unwrap_or_default();
        let why_old = plan["why"].get(id.as_str()).cloned().unwrap_or(Value::Null);
        differences.push(json!({
            "project": id,
            "rules": o,
            "model": n,
            "whyRules": why_old,
            "whyModel": why_new,
        }));
    }
    let mut out = Map::new();
    out.insert("same".into(), json!(differences.is_empty()));
    out.insert("allRules".into(), plan["all"].clone());
    out.insert("allModel".into(), json!(reached.everything));
    out.insert("differences".into(), Value::Array(differences));
    out.insert("dropped".into(), Value::Array(dropped));
    out.insert("unowned".into(), json!(reached.unowned));
    out.insert("notBuilt".into(), json!(reached.not_built));
    Value::Object(out)
}

#[cfg(test)]
mod tests;
