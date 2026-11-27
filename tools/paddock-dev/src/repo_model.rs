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
use paddock_repo_graph::model::{Detected, Facts, Reached, reach};
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

/// What the model is built from once per tree: the inputs file, the tracked files and the nodes and edges the adapters found, all
/// read at one revision (the working tree in files mode). A replay of many changes against one tree builds it once
/// ([`build_world`]) and asks [`reach_in`] per change.
pub struct World {
    /// The declared inputs and owners (`docs/repo-graph.toml` through its one accessor, at the tree's revision).
    pub inputs: Inputs,
    /// Every tracked file of the tree the model was built from.
    pub files: Vec<String>,
    /// The nodes and edges.
    pub model: paddock_repo_graph::model::nodes::Model,
    /// The projects whose contract declares a smoke set (`graph.smoke`).
    pub smoke: BTreeSet<String>,
    /// Every project's contract declarations (`graph.tests`, `graph.smoke`, `graph.reads`), read at the tree's revision.
    pub decls: BTreeMap<String, decl::ProjectDecl>,
    /// An adapter that could not read a file (the plan fails closed on them).
    pub errors: Vec<ExtractError>,
}

/// The model of the tree at `head_rev` (a revision, or the working tree for `None`).
pub fn build_world(root: &Path, history: &dyn History, graph: &RepoGraph, head_rev: Option<&str>) -> Result<World, String> {
    let bulk = GitBulk::new(root, head_rev);
    build_world_with(root, history, graph, head_rev, &bulk).map(|(w, _)| w)
}

/// [`build_world`] over a given reader, with the processes the adapters started.
pub fn build_world_with(
    root: &Path,
    history: &dyn History,
    graph: &RepoGraph,
    head_rev: Option<&str>,
    bulk: &GitBulk,
) -> Result<(World, usize), String> {
    let inputs = match head_rev {
        Some(head) => {
            let text = text_of(crate::repo_inputs::FILE, bulk.bytes_at(head, crate::repo_inputs::FILE))?
                .ok_or_else(|| format!("{} is not in {head}", crate::repo_inputs::FILE))?;
            crate::repo_inputs::parse(&text)?
        }
        None => crate::repo_inputs::load(root)?,
    };
    crate::repo_inputs::check_apps(&inputs, &graph.apps)?;
    let files = match head_rev {
        Some(head) => bulk.tracked_at(head)?,
        None => history.tracked_files().map_err(|e| e.to_string())?,
    };
    // The CI adapter over the head's texts (the working tree in files mode).
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
    // The Cargo adapter (test targets and files, replaying the metadata read_graph recorded) and the code adapter (reads, script
    // imports, spec targets), which follows the imports of the scripts the CI adapter found.
    let recorded = paddock_repo_graph::RecordedMetadata(graph.metadata.clone());
    let scripts: BTreeSet<String> = model
        .nodes()
        .filter(|n| n.kind == paddock_repo_graph::model::nodes::NodeKind::Script)
        .flat_map(|n| n.files.iter().cloned())
        .collect();
    let spawned: std::rc::Rc<std::cell::Cell<usize>> = Default::default();
    let at_rev = |f: &str| match head_rev {
        Some(head) => text_of(f, bulk.bytes_at(head, f)),
        None => text_of(f, bulk.working_bytes(f)),
    };
    let decls = decl::load(&at_rev, graph, &inputs)?;
    let code_adapter = code::CodeExtractor {
        root: root.to_path_buf(),
        cargo: graph.cargo.clone(),
        npm: graph.npm.clone(),
        scripts,
        tests: decls.values().map(|d| d.tests.clone()).collect(),
        dynamic: decls.values().flat_map(|d| d.reads.clone()).collect(),
        rev: head_rev.map(str::to_string),
        spawned: spawned.clone(),
    };
    let mut second: Vec<Box<dyn paddock_repo_graph::model::extract::Extractor + '_>> = vec![Box::new(code_adapter)];
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
    for u in more.model.unresolved() {
        model.add_unresolved(u.clone());
    }
    for u in more.model.blind() {
        model.add_blind(u.clone());
    }
    errors.extend(more.errors);
    let world = World {
        inputs,
        files,
        model,
        smoke: decl::smoke_declared(&decls),
        decls,
        errors,
    };
    Ok((world, spawned.get()))
}

/// The model's reach for a planner request on a graph already read: the same changed files the rule tables see, the inputs file
/// through its accessor, and every text through one batched reader, all at the requested revision (the working tree in files
/// mode), so a replay of old commits does not depend on the checkout.
pub fn reach_for(root: &Path, history: &dyn History, req: &Request, graph: &RepoGraph) -> Result<(Vec<String>, Reached), String> {
    let bulk = GitBulk::new(root, req.base.as_ref().map(|_| req.head.as_str()));
    reach_with(root, history, req, graph, &bulk).map(|(c, r, _)| (c, r))
}

/// [`reach_for`] over a given reader, with what it started: the world and the change read through the same reader.
pub fn reach_with(
    root: &Path,
    history: &dyn History,
    req: &Request,
    graph: &RepoGraph,
    bulk: &GitBulk,
) -> Result<(Vec<String>, Reached, Spawns), String> {
    let (world, adapters) = build_world_with(root, history, graph, req.base.as_ref().map(|_| req.head.as_str()), bulk)?;
    let changed = changed_files(history, req)?;
    let (reached, base_spawns) = reach_in_with(&world, root, req, &changed, graph, bulk);
    let spawns = Spawns {
        reader: bulk.starts(),
        adapters: adapters + base_spawns,
    };
    Ok((changed, reached, spawns))
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

/// The reach of `changed` (a request's files) in a built [`World`]: the texts before and after come from `req`'s base and head,
/// read once for the whole set; a failed read fails closed.
pub fn reach_in(world: &World, root: &Path, req: &Request, changed: &[String], graph: &RepoGraph) -> Reached {
    let bulk = GitBulk::new(root, req.base.as_ref().map(|_| req.head.as_str()));
    reach_in_with(world, root, req, changed, graph, &bulk).0
}

/// [`reach_in`] over a given reader, with the processes the base reads started.
fn reach_in_with(world: &World, root: &Path, req: &Request, changed: &[String], graph: &RepoGraph, bulk: &GitBulk) -> (Reached, usize) {
    let World {
        inputs,
        files,
        model,
        errors: world_errors,
        ..
    } = world;
    let projects = projects(inputs, graph);
    let mut errors: Vec<ExtractError> = world_errors.clone();
    // The changed files' texts, both sides, in one read (a failed read fails closed below).
    let bytes = ChangedBytes::read(bulk, req, changed);
    errors.extend(bytes.errors());
    let late: RefCell<Vec<ExtractError>> = RefCell::new(Vec::new());
    // A changed file the head no longer has: the reads of the base tree name its readers (review P1-3). Its tree and texts come
    // from a reader at the base; a failed read fails closed below.
    let tracked: BTreeSet<&String> = files.iter().collect();
    let deleted: BTreeSet<String> = changed.iter().filter(|f| !tracked.contains(f)).cloned().collect();
    let spawned: std::rc::Rc<std::cell::Cell<usize>> = Default::default();
    let base_reads = match (&req.base, deleted.is_empty()) {
        (Some(base), false) => {
            let reader = code::CodeExtractor {
                root: root.to_path_buf(),
                cargo: graph.cargo.clone(),
                npm: graph.npm.clone(),
                scripts: BTreeSet::new(),
                tests: Vec::new(),
                dynamic: BTreeMap::new(),
                rev: Some(base.clone()),
                spawned: spawned.clone(),
            };
            let base_bulk = GitBulk::new(root, Some(base));
            base_bulk
                .tracked_at(base)
                .map_err(|e| ExtractError {
                    extractor: "code",
                    file: format!("the tree of {base}"),
                    reason: e,
                })
                .and_then(|base_files| {
                    let base_files: BTreeSet<String> = base_files.into_iter().collect();
                    reader.base_reads(base, &base_files, &base_bulk, &deleted)
                })
        }
        _ => Ok(Default::default()),
    };
    // The world's model, with the base reads' nodes and edges when the change deleted a read target (a copy only then).
    let mut with_base: Option<paddock_repo_graph::model::nodes::Model> = None;
    match base_reads {
        Ok(x) if !x.nodes.is_empty() || !x.edges.is_empty() => {
            let mut m = model.clone();
            for n in x.nodes {
                if m.node(&n.id).is_none() {
                    m.add_node(n);
                }
            }
            for e in x.edges {
                m.add_edge(e);
            }
            with_base = Some(m);
        }
        Ok(_) => {}
        Err(e) => errors.push(e),
    }
    let model = with_base.as_ref().unwrap_or(model);
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
        files,
        detect: Some(&detect),
        model: Some(model),
        seeds: &seeds,
    };
    let text = |f: &str| (bytes.text(&bytes.before, f, &late), bytes.text(&bytes.after, f, &late));
    let mut reached = reach(changed, &facts, inputs, &text);
    errors.extend(late.into_inner());
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
    (reached, spawned.get())
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
pub(crate) fn old_runs(plan: &Value, inputs: &Inputs, graph: &RepoGraph) -> BTreeMap<String, String> {
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

/// The model's answer in the same shape: what the selection runs for every project (ADR 0117 §5), with its halves: `all` (or an
/// app's `rust`/`ui` half) for the whole suite, else the parts of the union, joined by `+`: `release` (the installers), `smoke` (the
/// declared smoke set), `tests` (the reached test targets), `phases:<a,b>` (contract phases). Conformance is in every plan on both
/// sides (a planner output fact, not a reach).
fn new_runs(reached: &Reached, smoke: &BTreeSet<String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (id, s) in paddock_repo_graph::model::selection(reached, smoke) {
        if id == "docs" || s.skip() {
            continue;
        }
        let h = if s.full {
            match reached.nodes.get(&id) {
                Some(n) if id.starts_with("app:") && !reached.everything => halves_of(n.rust, n.ui).to_string(),
                _ => "all".to_string(),
            }
        } else {
            let mut parts: Vec<String> = Vec::new();
            if s.installers {
                parts.push("release".into());
            }
            if s.smoke {
                parts.push("smoke".into());
            }
            if !s.affected.is_empty() {
                parts.push("tests".into());
            }
            if !s.phases.is_empty() {
                parts.push(format!("phases:{}", s.phases.iter().cloned().collect::<Vec<_>>().join(",")));
            }
            parts.join("+")
        };
        out.insert(id, h);
    }
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
pub fn compare(plan: &Value, reached: &Reached, graph: &RepoGraph, inputs: &Inputs, smoke: &BTreeSet<String>) -> Value {
    let old = old_runs(plan, inputs, graph);
    let new = new_runs(reached, smoke);
    let mut differences = Vec::new();
    let mut dropped = Vec::new();
    let ids: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    for id in ids {
        let (o, n) = (old.get(id), new.get(id));
        if o == n {
            continue;
        }
        // Anything but a whole suite or a half (`smoke`, `tests`, `release`, `phases:..` and their unions) runs less than any rule.
        let narrower = match (o.map(String::as_str), n.map(String::as_str)) {
            (Some(_), None) => true,
            (Some(_), Some(h)) if !matches!(h, "all" | "rust" | "ui") => true,
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
                    .filter(|c| {
                        crate::repo_model::project_dir_of(inputs, graph, &id).is_some_and(|d| c.starts_with(&format!("{d}/")) || **c == d)
                    })
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
pub fn project_dir_of(inputs: &Inputs, graph: &RepoGraph, id: &str) -> Option<String> {
    projects(inputs, graph)
        .into_iter()
        .find(|(p, _)| p == id)
        .map(|(_, d)| d)
        .filter(|d| !d.is_empty())
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
    // A union (`release+smoke`) runs the targets of each of its parts.
    let mut out = BTreeSet::new();
    for part in label.into_iter().flat_map(|l| l.split('+')) {
        match part {
            "all" => out.extend(all.iter().cloned()),
            "rust" => out.extend(all.iter().filter(|t| t.starts_with("nextest:")).cloned()),
            "ui" => out.extend(all.iter().filter(|t| !t.starts_with("nextest:")).cloned()),
            "tests" => out.extend(reached.targets.get(id).cloned().unwrap_or_default()),
            _ => {}
        }
    }
    out
}

/// [`compare`] plus `droppedTargets`: for every project the model selects less of, the targets it no longer runs (ADR 0117 §8,
/// `--compare` prints what a narrowing drops). A project with no test target the model knows names itself, so no narrowing is silent.
pub fn compare_targets(plan: &Value, reached: &Reached, graph: &RepoGraph, world: &World) -> Value {
    let (inputs, smoke, model) = (&world.inputs, &world.smoke, &world.model);
    let mut out = compare(plan, reached, graph, inputs, smoke);
    let old = old_runs(plan, inputs, graph);
    let new = new_runs(reached, smoke);
    let projects = projects(inputs, graph);
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
pub mod decl;
pub mod hash;
pub mod reads;
pub mod replay;
pub mod ts;

#[cfg(test)]
mod tests;
