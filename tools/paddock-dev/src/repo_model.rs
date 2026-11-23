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
    // The CI adapter over the head's texts (the working tree in files mode), and the jobs a workflow edit changed (G5).
    let head_read = GitBulk::new(root, req.base.as_ref().map(|_| req.head.as_str()));
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
    let decls = decl::load(root, graph)?;
    let code_adapter = code::CodeExtractor {
        root: root.to_path_buf(),
        cargo: graph.cargo.clone(),
        npm: graph.npm.clone(),
        scripts,
        tests: decls.values().map(|d| d.tests.clone()).collect(),
        dynamic: decls.values().flat_map(|d| d.reads.clone()).collect(),
    };
    // A changed file the head no longer has: the reads of the base tree name its readers (review P1-3).
    let tracked: BTreeSet<&String> = files.iter().collect();
    let deleted: BTreeSet<String> = changed.iter().filter(|f| !tracked.contains(f)).cloned().collect();
    let base_reads = match (&req.base, deleted.is_empty()) {
        (Some(base), false) => {
            let base_files: BTreeSet<String> = git(root)
                .args(["ls-tree", "-r", "--name-only", base])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect())
                .unwrap_or_default();
            code_adapter.base_reads(base, &base_files, &GitBulk::new(root, Some(base)), &deleted)
        }
        _ => Default::default(),
    };
    let mut second: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor + '_>> = vec![Box::new(code_adapter)];
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
    for u in more.model.unresolved() {
        model.add_unresolved(u.clone());
    }
    for n in base_reads.nodes {
        if model.node(&n.id).is_none() {
            model.add_node(n);
        }
    }
    for e in base_reads.edges {
        model.add_edge(e);
    }
    errors.extend(more.errors);
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
        files: &files,
        detect: Some(&detect),
        model: Some(&model),
        seeds: &seeds,
    };
    let text = |f: &str| match &req.base {
        Some(base) => (bulk.text_at(base, f), bulk.text_at(&req.head, f)),
        None => (bulk.text_at("HEAD", f), std::fs::read_to_string(root.join(f)).ok()),
    };
    let mut reached = reach(&changed, &facts, &inputs, &text);
    // An adapter that cannot read a file fails closed (ADR 0117 §2, review round 2 P2-6).
    if !errors.is_empty() {
        reached.everything = true;
        let why = errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ");
        for (id, _) in &projects {
            reached.nodes.insert(
                id.clone(),
                paddock_repo_graph::model::NodeReach::full(&format!("fail closed: {why}")),
            );
        }
    }
    if req.all {
        reached.everything = true;
        for (id, _) in &projects {
            reached
                .nodes
                .insert(id.clone(), paddock_repo_graph::model::NodeReach::full("everything (no base)"));
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
            // Less than full is named by what the selection will run (ADR 0117 §5): the smoke set, the reached tests, installers.
            let h = match n.reach {
                Reach::Plumbing => "smoke",
                Reach::Test => "tests",
                Reach::Release => "release",
                _ if id.starts_with("app:") => halves_of(n.rust, n.ui),
                _ => "all",
            };
            (id.clone(), h.to_string())
        })
        .collect();
    // A project reached only through its contract phases runs those phases (difference 4 of §8).
    for (id, phases) in &reached.phases {
        out.entry(id.clone())
            .or_insert_with(|| format!("phases:{}", phases.iter().cloned().collect::<Vec<_>>().join(",")));
    }
    out.insert("conformance".to_string(), "all".to_string());
    out
}

/// `--compare`: the projects where the model and the rule tables differ, each with both answers and both reasons. `dropped` lists
/// the projects the model runs less of (ADR 0117 §8: a narrowing names what it no longer runs).
pub fn compare(plan: &Value, reached: &Reached, graph: &RepoGraph, smoke: &BTreeSet<String>) -> Value {
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
            (Some(_), Some("smoke" | "tests" | "release")) => true,
            (Some(_), Some(h)) if h.starts_with("phases:") => true,
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
    out.insert("unresolvedRefs".into(), json!(reached.unresolved_refs));
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
    let sel: Map<String, Value> = paddock_repo_graph::model::selection(reached, smoke)
        .into_iter()
        .map(|(id, s)| {
            let mut v = Map::new();
            if s.skip() {
                v.insert("skip".into(), json!(true));
            }
            if s.full {
                v.insert("full".into(), json!(true));
                // G11: the reached packages of a workspace, unless something other than their own files reached the project.
                let packages: Vec<&str> = reached
                    .crates
                    .iter()
                    .filter(|c| crate::repo_model::project_dir_of(graph, &id).is_some_and(|d| c.starts_with(&format!("{d}/")) || **c == d))
                    .filter_map(|c| graph.cargo.get(c).map(|n| n.name.as_str()))
                    .collect();
                if !reached.whole.contains(&id) && !reached.everything && !packages.is_empty() {
                    v.insert("packages".into(), json!(packages));
                }
            }
            if s.smoke {
                v.insert("smoke".into(), json!(true));
            }
            if !s.affected.is_empty() {
                v.insert("affected".into(), json!(s.affected));
                v.insert("files".into(), json!(reached.test_files.get(&id).cloned().unwrap_or_default()));
            }
            if s.installers {
                v.insert("installers".into(), json!(true));
            }
            if !s.phases.is_empty() {
                v.insert("phases".into(), json!(s.phases));
            }
            (id, Value::Object(v))
        })
        .collect();
    out.insert("selection".into(), Value::Object(sel));
    Value::Object(out)
}

/// The folder of a project id (`app:dyno` is `apps/dyno`, a root project its own folder).
pub fn project_dir_of(graph: &RepoGraph, id: &str) -> Option<String> {
    projects(graph)
        .into_iter()
        .find(|(p, _)| p == id)
        .map(|(_, d)| d)
        .filter(|d| !d.is_empty())
}

pub mod ci;
pub mod code;
pub mod decl;
pub mod reads;

#[cfg(test)]
mod tests;
