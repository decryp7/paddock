//! The repository model in the planner (ADR 0117 WP-1): the facts the model needs, read from the tree the planner already read
//! ([`crate::plan::RepoGraph`]) and the inputs file ([`crate::repo_inputs::load`], the one accessor); the file texts through one
//! batched git reader ([`GitBulk`], `git cat-file --batch-command`, one process for every read); and `--compare`, which sets the
//! model's answer beside the rule tables' answer project by project (ADR 0117 §8, the migration net).
//!
//! The rule tables still decide every plan until WP-5; this module decides nothing CI runs.

use crate::plan::{RepoGraph, Request};
use paddock_repo_graph::History;
use paddock_repo_graph::model::extract::{BulkRead, ExtractError, Read};
use paddock_repo_graph::model::inputs::Inputs;
use paddock_repo_graph::model::{Detected, Facts, Reach, Reached, reach};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Every project id with its folder: the declared projects (`[[project]]` in `docs/repo-graph.toml`) and each app (`app:<name>` =
/// `<apps folder>/<name>`).
pub fn projects(inputs: &Inputs, graph: &RepoGraph) -> Vec<(String, String)> {
    inputs
        .projects
        .iter()
        .map(|p| (p.id.clone(), p.folder.clone()))
        .chain(graph.apps.iter().map(|a| (format!("app:{a}"), format!("{}{a}", inputs.paths.apps))))
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

/// What a read of raw bytes gave: the bytes, `None` for a file that is not there, or why it could not be read.
pub type RawRead = Result<Option<Vec<u8>>, String>;

/// The one bulk reader of file texts at a revision (or the working tree), over one `git cat-file --batch-command` process
/// (in process through gix where it can): no process per file (ER-20). Every method keeps absent and failed apart.
pub struct GitBulk {
    reader: RefCell<paddock_git::Reader>,
    rev: Option<String>,
    root: PathBuf,
    /// Revisions already shown to exist (git answers "missing" for a path of a revision it does not know).
    known: RefCell<BTreeSet<String>>,
}

impl GitBulk {
    /// A reader of `rev`'s blobs, or of the working tree when `rev` is `None`.
    pub fn new(root: &Path, rev: Option<&str>) -> GitBulk {
        let r = root.to_path_buf();
        GitBulk {
            reader: RefCell::new(paddock_git::Reader::open(root, move || git(&r))),
            rev: rev.map(str::to_string),
            root: root.to_path_buf(),
            known: RefCell::default(),
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
            known: RefCell::default(),
        }
    }

    /// How many git processes it started (the spawn budget test reads it).
    pub fn starts(&self) -> usize {
        self.reader.borrow().starts()
    }

    /// The tracked files of `rev`, sorted, through the same reader (no `git ls-tree` of its own).
    pub fn tracked_at(&self, rev: &str) -> Result<Vec<String>, String> {
        self.check_rev(rev)?;
        let spec = format!("{rev}^{{tree}}");
        match self.reader.borrow_mut().walk(&spec) {
            Ok(Some(entries)) => Ok(entries.into_iter().map(|e| e.path).collect()),
            Ok(None) => Err(format!("revision {rev} has no tree")),
            Err(e) => Err(format!("the files of {rev} cannot be listed: {e}")),
        }
    }

    /// Fails unless `rev` names a commit with a tree: `cat-file` answers "missing" for `<rev>:<path>` of a revision it does not
    /// know, which would read as an absent file. Each revision is checked once.
    fn check_rev(&self, rev: &str) -> Result<(), String> {
        if self.known.borrow().contains(rev) {
            return Ok(());
        }
        match self.reader.borrow_mut().tree(&format!("{rev}^{{tree}}")) {
            Ok(Some(_)) => {
                self.known.borrow_mut().insert(rev.to_string());
                Ok(())
            }
            Ok(None) => Err(format!("revision {rev} is not in the repository")),
            Err(e) => Err(format!("revision {rev} cannot be checked: {e}")),
        }
    }

    /// The bytes of each `(rev, path)`, in order, through the one process.
    pub fn bytes_all(&self, items: &[(String, String)]) -> Vec<RawRead> {
        let revs: BTreeSet<&str> = items.iter().map(|(r, _)| r.as_str()).collect();
        let bad: BTreeMap<&str, String> = revs.into_iter().filter_map(|r| self.check_rev(r).err().map(|e| (r, e))).collect();
        // Items of a revision git does not know fail; the others are read in the one call, in order.
        let good: Vec<&(String, String)> = items.iter().filter(|(r, _)| !bad.contains_key(r.as_str())).collect();
        let refs: Vec<(&str, &str)> = good.iter().map(|(r, p)| (r.as_str(), p.as_str())).collect();
        let mut read = self
            .reader
            .borrow_mut()
            .files_at(&refs)
            .into_iter()
            .zip(good)
            .map(|(r, (rev, path))| r.map_err(|e| format!("{path} at {rev} cannot be read: {e}")));
        items
            .iter()
            .map(|(r, _)| match bad.get(r.as_str()) {
                Some(e) => Err(e.clone()),
                None => read.next().unwrap_or(Ok(None)),
            })
            .collect()
    }

    /// One file's bytes at `rev` through the same reader.
    pub fn bytes_at(&self, rev: &str, path: &str) -> RawRead {
        self.bytes_all(&[(rev.to_string(), path.to_string())]).remove(0)
    }

    /// One working-tree file's bytes: a file that is not there is `Ok(None)`, any other error is the reason.
    pub fn working_bytes(&self, path: &str) -> RawRead {
        match std::fs::read(self.root.join(path)) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{path} cannot be read: {e}")),
        }
    }
}

/// UTF-8 text from raw bytes: not UTF-8 is an error, never "absent".
pub fn text_of(path: &str, r: RawRead) -> Read {
    match r {
        Ok(Some(b)) => String::from_utf8(b).map(Some).map_err(|_| format!("{path} is not UTF-8 text")),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

impl BulkRead for GitBulk {
    fn read_all(&self, paths: &[String]) -> Vec<Read> {
        let raw: Vec<RawRead> = match &self.rev {
            None => paths.iter().map(|p| self.working_bytes(p)).collect(),
            Some(rev) => {
                let items: Vec<(String, String)> = paths.iter().map(|p| (rev.clone(), p.clone())).collect();
                self.bytes_all(&items)
            }
        };
        raw.into_iter().zip(paths).map(|(r, p)| text_of(p, r)).collect()
    }
}

/// The changed files' bytes before and after the change, read once for the whole set: base mode reads both sides at their
/// revisions through the one reader, files mode reads HEAD through it and the working tree from disk.
struct ChangedBytes {
    before: BTreeMap<String, RawRead>,
    after: BTreeMap<String, RawRead>,
}

impl ChangedBytes {
    fn read(bulk: &GitBulk, req: &Request, changed: &[String]) -> ChangedBytes {
        let (before_rev, after_rev) = match &req.base {
            Some(base) => (base.as_str(), Some(req.head.as_str())),
            None => ("HEAD", None),
        };
        let mut items: Vec<(String, String)> = changed.iter().map(|f| (before_rev.to_string(), f.clone())).collect();
        if let Some(h) = after_rev {
            items.extend(changed.iter().map(|f| (h.to_string(), f.clone())));
        }
        let mut got = bulk.bytes_all(&items).into_iter();
        let before: BTreeMap<String, RawRead> = changed.iter().map(|f| (f.clone(), got.next().unwrap_or(Ok(None)))).collect();
        let after: BTreeMap<String, RawRead> = changed
            .iter()
            .map(|f| {
                let r = match after_rev {
                    Some(_) => got.next().unwrap_or(Ok(None)),
                    None => bulk.working_bytes(f),
                };
                (f.clone(), r)
            })
            .collect();
        ChangedBytes { before, after }
    }

    /// Every failed read as an adapter error: the plan answers everything.
    fn errors(&self) -> Vec<ExtractError> {
        self.before
            .iter()
            .chain(&self.after)
            .filter_map(|(f, r)| {
                r.as_ref().err().map(|e| ExtractError {
                    extractor: "reach",
                    file: f.clone(),
                    reason: e.clone(),
                })
            })
            .collect()
    }

    fn text(&self, side: &BTreeMap<String, RawRead>, f: &str, late: &RefCell<Vec<ExtractError>>) -> Option<String> {
        match text_of(f, side.get(f).cloned().unwrap_or(Ok(None))) {
            Ok(t) => t,
            Err(reason) => {
                late.borrow_mut().push(ExtractError {
                    extractor: "reach",
                    file: f.to_string(),
                    reason,
                });
                None
            }
        }
    }
}

/// What a run of the model started, for the spawn-budget test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Spawns {
    /// Processes of the one bulk reader.
    pub reader: usize,
    /// Processes of the adapters (the code adapter's one `git grep`).
    pub adapters: usize,
}

/// The model's reach for a planner request on a graph already read: the same changed files the rule tables see, the inputs file
/// through its accessor, and every text through one batched reader, all at the requested revision (the working tree in files
/// mode), so a replay of old commits does not depend on the checkout.
pub fn reach_for(root: &Path, history: &dyn History, req: &Request, graph: &RepoGraph) -> Result<(Vec<String>, Reached), String> {
    let bulk = GitBulk::new(root, req.base.as_ref().map(|_| req.head.as_str()));
    reach_with(root, history, req, graph, &bulk).map(|(c, r, _)| (c, r))
}

/// [`reach_for`] over a given reader, with what it started.
pub fn reach_with(
    root: &Path,
    history: &dyn History,
    req: &Request,
    graph: &RepoGraph,
    bulk: &GitBulk,
) -> Result<(Vec<String>, Reached, Spawns), String> {
    let inputs = match &req.base {
        Some(_) => {
            let text = text_of(crate::repo_inputs::FILE, bulk.bytes_at(&req.head, crate::repo_inputs::FILE))?
                .ok_or_else(|| format!("{} is not in {}", crate::repo_inputs::FILE, req.head))?;
            crate::repo_inputs::parse(&text)?
        }
        None => crate::repo_inputs::load(root)?,
    };
    crate::repo_inputs::check_apps(&inputs, &graph.apps)?;
    let files = match &req.base {
        Some(_) => bulk.tracked_at(&req.head)?,
        None => history.tracked_files().map_err(|e| e.to_string())?,
    };
    let changed = if req.all {
        Vec::new()
    } else if let Some(base) = &req.base {
        history.changed_files(base, &req.head).map_err(|e| e.to_string())?
    } else {
        req.files.clone()
    };
    let projects = projects(&inputs, graph);
    // The changed files' texts, both sides, in one read (a failed read fails closed below).
    let bytes = ChangedBytes::read(bulk, req, &changed);
    let late: RefCell<Vec<ExtractError>> = RefCell::new(Vec::new());
    // The CI adapter over the head's texts (the working tree in files mode), and the jobs a workflow edit changed (G5).
    let adapter = ci::CiExtractor {
        apps: graph.apps.clone(),
        projects: inputs.projects.iter().map(|p| p.id.clone()).collect(),
        decl: ci::CiDecl {
            setup_only: inputs.setup_only.iter().cloned().collect(),
            release_actions: inputs.release_actions.iter().cloned().collect(),
            paths: inputs.paths.clone(),
        },
    };
    let adapters: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor>> = vec![Box::new(adapter)];
    let built = paddock_repo_graph::model::extract::build(&files, bulk, &adapters);
    let mut model = built.model;
    let mut errors = built.errors;
    errors.extend(bytes.errors());
    // The Cargo adapter (test targets and files, replaying the metadata read_graph recorded) and the code adapter (reads, script
    // imports, spec targets), which follows the imports of the scripts the CI adapter found.
    let recorded = paddock_repo_graph::RecordedMetadata(graph.metadata.clone());
    let scripts: BTreeSet<String> = model
        .nodes()
        .filter(|n| n.kind == paddock_repo_graph::model::nodes::NodeKind::Script)
        .flat_map(|n| n.files.iter().cloned())
        .collect();
    let spawned: std::rc::Rc<std::cell::Cell<usize>> = Default::default();
    let code = code::CodeExtractor {
        root: root.to_path_buf(),
        cargo: graph.cargo.clone(),
        npm: graph.npm.clone(),
        scripts,
        paths: inputs.paths.clone(),
        rev: req.base.as_ref().map(|_| req.head.clone()),
        spawned: spawned.clone(),
    };
    let mut second: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor + '_>> = vec![Box::new(code)];
    if !graph.metadata.is_empty() {
        second.insert(
            0,
            Box::new(paddock_repo_graph::model::extract::CargoExtractor::new(&recorded, &graph.root_text)),
        );
    }
    let more = paddock_repo_graph::model::extract::build(&files, bulk, &second);
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
    let mut seeds: Vec<(String, String, String)> = Vec::new();
    for f in changed
        .iter()
        .filter(|f| model.node(&ci::workflow_id(f)).is_some_and(|n| n.files.contains(f)))
    {
        let (before, after) = (bytes.text(&bytes.before, f, &late), bytes.text(&bytes.after, f, &late));
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
        bytes
            .after
            .get(f)
            .and_then(|r| r.as_ref().ok())
            .and_then(Option::as_ref)
            .map(|b| paddock_repo_graph::model::detect(b))
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
    let text = |f: &str| (bytes.text(&bytes.before, f, &late), bytes.text(&bytes.after, f, &late));
    let mut reached = reach(&changed, &facts, &inputs, &text);
    errors.extend(late.into_inner());
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
    let spawns = Spawns {
        reader: bulk.starts(),
        adapters: spawned.get(),
    };
    Ok((changed, reached, spawns))
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
fn old_runs(plan: &Value, inputs: &Inputs, graph: &RepoGraph) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (id, _) in projects(inputs, graph).iter().filter(|(id, _)| !id.starts_with("app:")) {
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
pub fn compare(plan: &Value, reached: &Reached, graph: &RepoGraph, inputs: &Inputs) -> Value {
    let old = old_runs(plan, inputs, graph);
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

pub mod ci;
pub mod code;

#[cfg(test)]
mod tests;
