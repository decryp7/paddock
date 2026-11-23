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

/// The root projects and their folders: the plan's project keys. `c99` is a check job with no folder (an empty one owns no path);
/// it is reached by named inputs and by a true global.
pub const ROOT_PROJECTS: [(&str, &str); 9] = [
    ("crates", "crates"),
    ("ui", "ui"),
    ("eslint", "eslint"),
    ("conformance", "conformance"),
    ("tools", "tools"),
    ("icons", "icons"),
    ("ci-image", "ci-image"),
    ("paddock-dev", "tools/paddock-dev"),
    ("c99", ""),
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

/// What the model is built from once per tree: the inputs file, the tracked files and the nodes and edges the adapters found.
/// A replay of many changes against one tree builds it once ([`build_world`]) and asks [`reach_in`] per change.
pub struct World {
    /// The declared inputs and owners (`docs/repo-graph.toml` through its one accessor).
    pub inputs: paddock_repo_graph::model::inputs::Inputs,
    /// Every tracked file of the tree the model was built from.
    pub files: Vec<String>,
    /// The nodes and edges.
    pub model: paddock_repo_graph::model::nodes::Model,
    /// An adapter that could not read a file (the plan fails closed on them).
    pub errors: Vec<paddock_repo_graph::model::extract::ExtractError>,
}

/// The model of the tree whose texts `head_read` reads (a revision, or the working tree for `None`).
pub fn build_world(root: &Path, history: &dyn History, graph: &RepoGraph, head_rev: Option<&str>) -> Result<World, String> {
    let inputs = crate::repo_inputs::load(root)?;
    let files = history.tracked_files().map_err(|e| e.to_string())?;
    // The CI adapter over the head's texts (the working tree in files mode), and the jobs a workflow edit changed (G5).
    let head_read = GitBulk::new(root, head_rev);
    let adapter = ci::CiExtractor {
        apps: graph.apps.clone(),
        projects: ROOT_PROJECTS.iter().map(|(id, _)| id.to_string()).collect(),
        decl: ci::CiDecl {
            setup_only: inputs.setup_only.iter().cloned().collect(),
            release_actions: inputs.release_actions.iter().cloned().collect(),
        },
    };
    let adapters: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor>> = vec![Box::new(adapter)];
    let built = paddock_repo_graph::model::extract::build(&files, &head_read, &adapters);
    let mut model = built.model;
    let mut errors = built.errors;
    // The Cargo adapter (test targets and files, replaying the metadata read_graph recorded) and the code adapter (reads, script
    // imports, spec targets), which follows the imports of the scripts the CI adapter found.
    let recorded = paddock_repo_graph::RecordedMetadata(graph.metadata.clone());
    let scripts: BTreeSet<String> = model
        .nodes()
        .filter(|n| n.kind == paddock_repo_graph::model::nodes::NodeKind::Script)
        .flat_map(|n| n.files.iter().cloned())
        .collect();
    let mut second: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor>> = vec![Box::new(code::CodeExtractor {
        root: root.to_path_buf(),
        cargo: graph.cargo.clone(),
        npm: graph.npm.clone(),
        scripts,
    })];
    if !graph.metadata.is_empty() {
        second.insert(
            0,
            Box::new(paddock_repo_graph::model::extract::CargoExtractor::new(&recorded, &graph.root_text)),
        );
    }
    let more = paddock_repo_graph::model::extract::build(&files, &head_read, &second);
    for n in more.model.nodes() {
        // A script the CI adapter already has keeps its node (and its flags).
        if model.node(&n.id).is_none() {
            model.add_node(n.clone());
        }
    }
    for e in more.model.edges() {
        model.add_edge(e.clone());
    }
    errors.extend(more.errors);
    Ok(World {
        inputs,
        files,
        model,
        errors,
    })
}

/// The model's reach for a planner request on a graph already read: the same changed files the rule tables see, the inputs file
/// through its accessor, and the changed files' texts through one batched reader.
pub fn reach_for(root: &Path, history: &dyn History, req: &Request, graph: &RepoGraph) -> Result<(Vec<String>, Reached), String> {
    let world = build_world(root, history, graph, req.base.as_ref().map(|_| req.head.as_str()))?;
    let changed = changed_files(history, req)?;
    let reached = reach_in(&world, root, req, &changed, graph);
    Ok((changed, reached))
}

/// The changed files of a request: none for `all`, the range's files for a base, else the given files.
pub fn changed_files(history: &dyn History, req: &Request) -> Result<Vec<String>, String> {
    Ok(if req.all {
        Vec::new()
    } else if let Some(base) = &req.base {
        history.changed_files(base, &req.head).map_err(|e| e.to_string())?
    } else {
        req.files.clone()
    })
}

/// The reach of `changed` (a request's files) in a built [`World`]: the texts before and after come from `req`'s base and head.
pub fn reach_in(world: &World, root: &Path, req: &Request, changed: &[String], graph: &RepoGraph) -> Reached {
    let World {
        inputs,
        files,
        model,
        errors,
    } = world;
    let projects = projects(graph);
    let bulk = GitBulk::new(root, None);
    let mut seeds: Vec<(String, String, String)> = Vec::new();
    for f in changed
        .iter()
        .filter(|f| model.node(&ci::workflow_id(f)).is_some_and(|n| n.files.contains(f)))
    {
        let (before, after) = match &req.base {
            Some(base) => (bulk.text_at(base, f), bulk.text_at(&req.head, f)),
            None => (bulk.text_at("HEAD", f), std::fs::read_to_string(root.join(f)).ok()),
        };
        let wf = ci::workflow_id(f);
        // The same text on both sides (files mode names a committed file): nothing can be narrowed, every job (fail closed).
        let narrowed = if before == after {
            None
        } else {
            ci::changed_jobs(before.as_deref(), after.as_deref())
        };
        let jobs: Vec<String> = match narrowed {
            Some(jobs) => jobs.iter().map(|j| ci::job_id(f, j)).collect(),
            // The header changed (or a side cannot be read): every job of the workflow.
            None => model
                .targets(&wf, paddock_repo_graph::model::nodes::EdgeKind::Uses)
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };
        for j in jobs {
            seeds.push((f.clone(), j.clone(), format!("{j} changed in {f}")));
        }
        // An edit that changes no job (a moved line, a comment) still names the workflow, which runs nothing by itself.
        seeds.push((f.clone(), wf.clone(), format!("{f} changed")));
    }
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
        files,
        detect: Some(&detect),
        model: Some(model),
        seeds: &seeds,
    };
    let text = |f: &str| match &req.base {
        Some(base) => (bulk.text_at(base, f), bulk.text_at(&req.head, f)),
        None => (bulk.text_at("HEAD", f), std::fs::read_to_string(root.join(f)).ok()),
    };
    let mut reached = reach(changed, &facts, inputs, &text);
    // An adapter that cannot read a file fails closed (ADR 0117 §2, review round 2 P2-6).
    if !errors.is_empty() {
        reached.everything = true;
        let why = errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
        for (id, _) in &projects {
            reached.nodes.insert(
                id.clone(),
                paddock_repo_graph::model::NodeReach {
                    reach: Reach::Code,
                    rust: true,
                    ui: true,
                    why: vec![format!("fail closed: {why}")],
                },
            );
        }
    }
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
    reached
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

/// The model's answer in the same shape: what the selection runs for every project (ADR 0117 §5), with its halves. A plumbing reach
/// is `smoke` only for a project whose contract declares a smoke set (`smoke`); otherwise the selection is the whole suite, never less
/// than declared. Conformance is in every plan on both sides (a planner output fact, not a reach).
fn new_runs(reached: &Reached, smoke: &BTreeSet<String>) -> BTreeMap<String, String> {
    use paddock_repo_graph::model::Selection;
    let mut out: BTreeMap<String, String> = paddock_repo_graph::model::selection(reached, smoke)
        .into_iter()
        .filter(|(id, _)| id != "docs")
        .filter_map(|(id, s)| {
            let h = match s {
                Selection::Skip => return None,
                Selection::Smoke => "smoke",
                Selection::Affected(_) => "tests",
                Selection::Installers => "release",
                Selection::Full => match reached.nodes.get(&id) {
                    Some(n) if id.starts_with("app:") && !reached.everything => halves_of(n.rust, n.ui),
                    _ => "all",
                },
            };
            Some((id, h.to_string()))
        })
        .collect();
    out.insert("conformance".to_string(), "all".to_string());
    out
}

/// The projects whose contract declares a smoke set (`smoke` in `apps/<app>/paddock.ci.json`, ADR 0117 §3): only they may run
/// less than full on a plumbing reach.
pub fn smoke_declared(graph: &RepoGraph) -> BTreeSet<String> {
    graph
        .configs
        .iter()
        .filter(|(_, c)| c.get("smoke").is_some())
        .map(|(a, _)| format!("app:{a}"))
        .collect()
}

/// `--compare`: the projects where the model and the rule tables differ, each with both answers and both reasons. `dropped` lists
/// the projects the model runs less of (ADR 0117 §8: a narrowing names what it no longer runs).
pub fn compare(plan: &Value, reached: &Reached, graph: &RepoGraph) -> Value {
    let old = old_runs(plan, graph);
    let new = new_runs(reached, &smoke_declared(graph));
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
            (Some(_), Some("smoke" | "tests" | "release")) => true,
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
    let model: Map<String, Value> = reached
        .nodes
        .iter()
        .map(|(id, n)| {
            (
                id.clone(),
                json!({"reach": n.reach.name(), "rust": n.rust, "ui": n.ui, "why": n.why}),
            )
        })
        .collect();
    out.insert("model".into(), Value::Object(model));
    let sel: Map<String, Value> = paddock_repo_graph::model::selection(reached, &smoke_declared(graph))
        .into_iter()
        .map(|(id, s)| {
            let v = match s {
                paddock_repo_graph::model::Selection::Full => json!("full"),
                paddock_repo_graph::model::Selection::Smoke => json!("smoke"),
                paddock_repo_graph::model::Selection::Installers => json!("installers"),
                paddock_repo_graph::model::Selection::Skip => json!("skip"),
                paddock_repo_graph::model::Selection::Affected(t) => {
                    json!({"affected": t, "files": reached.test_files.get(&id).cloned().unwrap_or_default()})
                }
            };
            (id, v)
        })
        .collect();
    out.insert("selection".into(), Value::Object(sel));
    Value::Object(out)
}

/// The test targets of a project: the target nodes whose home folder the project owns (the deepest project folder wins, so
/// `tools/paddock-dev` is not part of `tools`).
fn targets_of(model: &paddock_repo_graph::model::nodes::Model, projects: &[(String, String)], id: &str) -> BTreeSet<String> {
    use paddock_repo_graph::model::nodes::NodeKind;
    model
        .nodes()
        .filter(|n| n.kind == NodeKind::TestTarget)
        .filter(|n| {
            let Some(home) = n.home.as_deref() else { return false };
            let owner = projects
                .iter()
                .filter(|(_, dir)| !dir.is_empty() && (home == dir || home.strip_prefix(dir.as_str()).is_some_and(|r| r.starts_with('/'))))
                .max_by_key(|(_, dir)| dir.len());
            owner.is_some_and(|(p, _)| p == id)
        })
        .map(|n| n.id.clone())
        .collect()
}

/// The targets a run label names for a project: `all` every target, `rust` the nextest ones, `ui` the others, `tests` the reached
/// ones. `smoke` and `release` name no enumerable target: every target of the project counts as dropped by them.
fn run_targets(label: Option<&str>, all: &BTreeSet<String>, reached: &Reached, id: &str) -> BTreeSet<String> {
    match label {
        Some("all") => all.clone(),
        Some("rust") => all.iter().filter(|t| t.starts_with("nextest:")).cloned().collect(),
        Some("ui") => all.iter().filter(|t| !t.starts_with("nextest:")).cloned().collect(),
        Some("tests") => reached.targets.get(id).cloned().unwrap_or_default(),
        _ => BTreeSet::new(),
    }
}

/// [`compare`] plus `droppedTargets`: for every project the model selects less of, the targets it no longer runs (ADR 0117 §8,
/// `--compare` prints what a narrowing drops). A project with no test target the model knows names itself, so no narrowing is silent.
pub fn compare_targets(plan: &Value, reached: &Reached, graph: &RepoGraph, model: &paddock_repo_graph::model::nodes::Model) -> Value {
    let mut out = compare(plan, reached, graph);
    let old = old_runs(plan, graph);
    let new = new_runs(reached, &smoke_declared(graph));
    let projects = projects(graph);
    let mut dropped = Map::new();
    for id in out["dropped"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
    {
        let all = targets_of(model, &projects, id);
        let ran = run_targets(old.get(id).map(String::as_str), &all, reached, id);
        let runs = run_targets(new.get(id).map(String::as_str), &all, reached, id);
        let mut gone: Vec<String> = ran.difference(&runs).cloned().collect();
        if gone.is_empty() && all.is_empty() {
            gone.push(format!("{id}: no test target known to the model"));
        }
        dropped.insert(id.to_string(), json!(gone));
    }
    out["droppedTargets"] = Value::Object(dropped);
    out
}

pub mod ci;
pub mod code;
pub mod hash;
pub mod replay;

#[cfg(test)]
mod tests;
