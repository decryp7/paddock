//! The repository model of ADR 0117: typed nodes, declared inputs and owners, and bulk queries over a change. This module is
//! the project level (WP-1): owners, named inputs ([`inputs`]), the attribute-aware `.gitattributes` input ([`attributes`]) and
//! [`reach`], which answers, for a whole changed-file set at once, every project the change reaches, how (`Reach`) and which
//! half (Rust, UI). CI, test-file and OS nodes join in later work packages through the same query.
//!
//! Pure: the caller supplies the graphs, the tracked files and the texts it needs (one bulk read, ER-20).

pub mod attributes;
pub mod content;
pub mod extract;
#[cfg(test)]
mod fixture;
pub mod generators;
pub mod glob;
pub mod guard;
pub mod hash;
pub mod inputs;
pub mod narrow;
pub mod nodes;
pub mod os;

use crate::Graph;
#[cfg(feature = "content")]
pub use content::detect;
pub use content::{DetectOf, Detected};
use inputs::{Inputs, Owner, Target};
use narrow::Narrowing;
pub use nodes::Unresolved;
use std::collections::{BTreeMap, BTreeSet};

/// A changed file's text before and after the change (`None`: the file does not exist there).
pub type TextPair = (Option<String>, Option<String>);

/// The reader of [`TextPair`]s that [`reach`] asks for attribute inputs.
pub type TextOf<'a> = &'a dyn Fn(&str) -> TextPair;

/// How a change reaches a node, weakest first: the strongest kind over every path that arrives at the node wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reach {
    /// Documentation only: the docs checks.
    Docs,
    /// CI plumbing only (a setup or cache step): the node's declared smoke set.
    Plumbing,
    /// Test code only: the test targets that import it.
    Test,
    /// A release step (bundling, signing, publishing): the installers.
    Release,
    /// Source, manifests or configuration: the node's whole suite.
    Code,
}

impl Reach {
    /// The word of the inputs file and the plan output.
    pub fn name(self) -> &'static str {
        match self {
            Reach::Docs => "docs",
            Reach::Plumbing => "plumbing",
            Reach::Test => "test",
            Reach::Release => "release",
            Reach::Code => "code",
        }
    }

    /// The kind a word names.
    pub fn parse(s: &str) -> Result<Reach, String> {
        [
            Reach::Docs,
            Reach::Plumbing,
            Reach::Test,
            Reach::Release,
            Reach::Code,
        ]
        .into_iter()
        .find(|r| r.name() == s)
        .ok_or_else(|| format!("unknown reach {s:?} (code, release, test, plumbing or docs)"))
    }
}

/// What the model knows about the repository's projects, supplied by the caller.
#[derive(Clone, Copy)]
pub struct Facts<'a> {
    /// Cargo crates and their path dependencies.
    pub cargo: &'a Graph,
    /// npm packages and their edges.
    pub npm: &'a Graph,
    /// Project id to its folder: the root projects (`crates` = `crates`, `paddock-dev` = `tools/paddock-dev`) and the apps
    /// (`app:dyno` = `apps/dyno`). The deepest folder that contains a path owns it.
    pub projects: &'a [(String, String)],
    /// Every tracked file (what a `.gitattributes` pattern can match).
    pub files: &'a [String],
    /// Content detection (D14, the `content` feature): only an aid under doc owners and for the owner check's error; `None`
    /// classes documentation assets by extension alone.
    pub detect: Option<DetectOf<'a>>,
    /// The node and edge model the extractors built (CI, scripts, tests); `None` at the project level alone. A changed file a
    /// model node owns is reached through it; a node that `runs` a project (a job, a step) passes its kind to that project.
    pub model: Option<&'a nodes::Model>,
    /// Narrowed seeds: (changed file, node id, why). A file listed here seeds these nodes instead of its owner (a workflow edit
    /// seeds the jobs whose text changed).
    pub seeds: &'a [(String, String, String)],
}

impl std::fmt::Debug for Facts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Facts")
            .field("projects", &self.projects)
            .field("files", &self.files.len())
            .field("detect", &self.detect.is_some())
            .field("model", &self.model.is_some())
            .field("seeds", &self.seeds.len())
            .finish()
    }
}

/// How one project is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeReach {
    /// The strongest kind.
    pub reach: Reach,
    /// Every kind that arrived (review P1-1: the selection is their union, not the strongest alone).
    pub kinds: BTreeSet<Reach>,
    /// Its Rust half is reached.
    pub rust: bool,
    /// Its UI half is reached.
    pub ui: bool,
    /// Why, one line per reason.
    pub why: Vec<String>,
}

/// The answer of [`reach`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reached {
    /// Project id to how it is reached (`docs` is the documentation node).
    pub nodes: BTreeMap<String, NodeReach>,
    /// Changed files that nothing builds (owner `none`).
    pub not_built: Vec<String>,
    /// Changed files no input, project folder or owner rule claims, and non-prose files under a doc owner that no input names
    /// (ADR 0117 §2, review round 2 P1-A/P1-B). The plan fails closed on them: [`Reached::everything`] is set and every project is
    /// reached in full, and the owner check names each file.
    pub unowned: Vec<String>,
    /// The plan must answer everything because a file is unowned (fail closed).
    pub everything: bool,
    /// The detected kind of each unowned file the detector could read (D14), for the owner check's error.
    pub unowned_kind: BTreeMap<String, Detected>,
    /// Per project, the test targets the change reached (a `test` reach selects these, ADR 0117 §5).
    pub targets: BTreeMap<String, BTreeSet<String>>,
    /// Per project, the changed test files (a spec runner is given these: Vitest `--include`, Playwright file arguments), and the
    /// specs a changed helper reaches through `imports`.
    pub test_files: BTreeMap<String, BTreeSet<String>>,
    /// The Cargo crates (folders) the change reaches: their own files, their Cargo dependents and readers (G11: a run names
    /// these packages).
    pub crates: BTreeSet<String>,
    /// Projects reached in full by something other than their crates' own files (an input, a project file outside every crate, an
    /// unowned file): their whole workspace runs, never a package list (G11).
    pub whole: BTreeSet<String>,
    /// The model's unresolved references, one line each: every plan runs their referrers' projects in full.
    pub unresolved_refs: Vec<String>,
    /// Per project, the contract phases a change reaches through their command lines (a shared script a phase runs, such as the
    /// conformance CLI of an app's `checks`): those phases run, not the project's suite (ADR 0117 §6, difference 4 of §8).
    pub phases: BTreeMap<String, BTreeSet<String>>,
}

impl NodeReach {
    /// A project reached in full for `why` (the plan answers everything, or an extractor failed).
    pub fn full(why: &str) -> NodeReach {
        NodeReach {
            reach: Reach::Code,
            kinds: BTreeSet::from([Reach::Code]),
            rust: true,
            ui: true,
            why: vec![why.to_string()],
        }
    }
}

impl Reached {
    fn mark(&mut self, id: &str, reach: Reach, rust: bool, ui: bool, why: String) {
        let n = self.nodes.entry(id.to_string()).or_insert(NodeReach {
            reach,
            kinds: BTreeSet::new(),
            rust: false,
            ui: false,
            why: Vec::new(),
        });
        n.reach = n.reach.max(reach);
        n.kinds.insert(reach);
        n.rust |= rust;
        n.ui |= ui;
        if !n.why.contains(&why) {
            n.why.push(why);
        }
    }
}

/// The project that owns `dir` or a file in it: the deepest project folder that contains it.
pub fn project_of<'a>(facts: &Facts<'a>, path: &str) -> Option<&'a str> {
    facts
        .projects
        .iter()
        // A project with an empty folder (a check job such as c99) owns no path; it is reached by inputs and edges only.
        .filter(|(_, d)| {
            !d.is_empty()
                && (path == d
                    || path
                        .strip_prefix(d.as_str())
                        .is_some_and(|r| r.starts_with('/')))
        })
        .max_by_key(|(_, d)| d.len())
        .map(|(id, _)| id.as_str())
}

/// The halves of a project a file reaches: Rust when the deepest package that holds it is a crate, UI when it is an npm package,
/// both when neither (configuration of the project itself).
fn halves(facts: &Facts, path: &str) -> (bool, bool) {
    let c = facts.cargo.node_of(path);
    let n = facts.npm.node_of(path);
    match (c, n) {
        (Some(c), Some(n)) if c.len() >= n.len() => (true, false),
        (Some(_), Some(_)) => (false, true),
        (Some(_), None) => (true, false),
        (None, Some(_)) => (false, true),
        (None, None) => (true, true),
    }
}

/// The folder of a project id.
fn dir_of<'a>(facts: &Facts<'a>, id: &str) -> Option<&'a str> {
    facts
        .projects
        .iter()
        .find(|(i, _)| i == id)
        .map(|(_, d)| d.as_str())
}

/// Whether a file under a doc owner is a documentation asset: by its detected content when a detector is given (a binary file
/// is, a text file is not, whatever its name; D14), else by its image extension.
fn doc_asset(facts: &Facts, inputs: &Inputs, path: &str) -> bool {
    match facts.detect.and_then(|d| d(path)) {
        Some(k) => k == Detected::Binary,
        None => inputs.is_doc_asset(path),
    }
}

/// Every project a change reaches, how and which half (ADR 0117 §5, the project level). `text(path)` gives a changed file's
/// text before and after the change, asked only for attribute inputs. Cargo edges carry the Rust half, npm edges the UI half
/// only (G9): a Tauri shell reached through npm is not rebuilt for its Rust code.
pub fn reach(changed: &[String], facts: &Facts, inputs: &Inputs, text: TextOf) -> Reached {
    let mut out = Reached::default();
    let mut crates: BTreeSet<String> = BTreeSet::new();
    let mut packages: BTreeSet<String> = BTreeSet::new();
    let cargo_projects: BTreeSet<&str> = facts
        .cargo
        .iter()
        .filter_map(|(d, _)| project_of(facts, d))
        .collect();
    let npm_projects: BTreeSet<&str> = facts
        .npm
        .iter()
        .filter_map(|(d, _)| project_of(facts, d))
        .collect();
    let mut model_seeds: Vec<(String, String)> = Vec::new();
    for f in changed {
        let named = inputs.inputs_of(f);
        // A file a model node owns outside every project folder: its node reaches what it reaches, and additive inputs add to it.
        let mut model_owned = false;
        // A file a model node owns (a workflow, an action, a script) is reached through the model; outside every project folder
        // and every input, that is all it is.
        // Prose outside every project folder is the doc owner's even inside an action folder (G7).
        // A read file (a `reads` edge names it) is code for its reader even when it is prose (§2).
        let read = facts.model.and_then(|m| m.owner_of(f)).is_some_and(|n| {
            facts
                .model
                .is_some_and(|m| !m.sources(&n.id, nodes::EdgeKind::Reads).is_empty())
        });
        let doc_prose = inputs.is_prose(f) && project_of(facts, f).is_none() && !read;
        if let Some(m) = facts.model.filter(|_| !doc_prose) {
            let narrowed: Vec<&(String, String, String)> = facts
                .seeds
                .iter()
                .filter(|(file, _, _)| file == f)
                .collect();
            // Every node that owns the file (a test file and the data node a crate reads it through, P1-3) is a seed.
            let owners = m.owners_of(f);
            // A test file in a project folder is test for its project: its targets, not the project's dependents (G12).
            if narrowed.is_empty()
                && named.iter().all(|i| i.adds)
                && owners.iter().any(|n| n.kind == nodes::NodeKind::TestFile)
                && let Some(p) = project_of(facts, f)
            {
                model_seeds.extend(owners.iter().map(|n| (n.id.clone(), f.clone())));
                let (rust, ui) = halves(facts, f);
                out.mark(p, Reach::Test, rust, ui, f.clone());
                out.test_files
                    .entry(p.to_string())
                    .or_default()
                    .insert(f.clone());
                for i in named.iter() {
                    for t in &i.reaches {
                        if let Target::Project(q) = t {
                            out.mark(q, i.reach, true, true, format!("input {} ({f})", i.name));
                        }
                    }
                }
                continue;
            }
            let owned = if narrowed.is_empty() {
                model_seeds.extend(owners.iter().map(|n| (n.id.clone(), f.clone())));
                !owners.is_empty()
            } else {
                narrowed
                    .iter()
                    .for_each(|(_, id, why)| model_seeds.push((id.clone(), why.clone())));
                true
            };
            if owned && named.is_empty() && project_of(facts, f).is_none() {
                continue;
            }
            model_owned = owned;
        }
        let mut fail_closed: Vec<String> = Vec::new();
        for i in &named {
            let why = format!("input {} ({f})", i.name);
            let mut targets_reached = true;
            if let Some(name) = &i.extractor {
                let (before, after) = text(f);
                // The same text on both sides (files mode names a committed file, or only the mode changed): nothing can be
                // narrowed, so the extractor says what that reaches, by default a fail closed like a workflow's (review P2).
                let extractor = narrow::content_extractor(name);
                let narrowed = match &extractor {
                    Some(x) if before.is_some() && before == after => Some(x.unchanged()),
                    Some(x) => Some(x.narrow(before.as_deref(), after.as_deref())),
                    None => None,
                };
                match narrowed {
                    Some(Ok(Narrowing::Patterns(patterns))) => {
                        for pattern in patterns {
                            for hit in facts.files.iter().filter(|t| glob::matches(&pattern, t)) {
                                if let Some(p) = project_of(facts, hit) {
                                    out.mark(
                                        p,
                                        i.reach,
                                        true,
                                        true,
                                        format!("{why}: the attributes of {pattern} changed"),
                                    );
                                }
                            }
                        }
                    }
                    Some(Ok(Narrowing::Packages(spaces))) => {
                        for ws in spaces {
                            if let Some(p) = project_of(facts, &ws) {
                                out.mark(
                                    p,
                                    i.reach,
                                    false,
                                    true,
                                    format!("{why}: the packages of {ws} changed"),
                                );
                            }
                            if facts.npm.contains(&ws) {
                                packages.insert(ws);
                            }
                        }
                    }
                    Some(Ok(Narrowing::Changed(c))) => targets_reached = c,
                    Some(Err(e)) => fail_closed.push(format!(
                        "{why}: the {name} extractor could not read it ({e})"
                    )),
                    None => fail_closed.push(format!("{why}: no {name} extractor in this build")),
                }
            }
            if !targets_reached {
                continue;
            }
            for t in &i.reaches {
                match t {
                    Target::All => {
                        for (id, _) in facts.projects {
                            out.mark(id, i.reach, true, true, why.clone());
                            out.whole.insert(id.clone());
                        }
                    }
                    Target::AppSegment(n) => {
                        let id = f.split('/').nth(*n).map(|seg| format!("app:{seg}"));
                        if let Some(id) =
                            id.filter(|id| facts.projects.iter().any(|(p, _)| p == id))
                        {
                            out.mark(&id, i.reach, true, true, why.clone());
                            out.whole.insert(id);
                        }
                    }
                    Target::Cargo => cargo_projects.iter().for_each(|p| {
                        out.mark(p, i.reach, true, false, why.clone());
                        out.whole.insert(p.to_string());
                    }),
                    Target::Npm => npm_projects.iter().for_each(|p| {
                        out.mark(p, i.reach, false, true, why.clone());
                        out.whole.insert(p.to_string());
                    }),
                    Target::Apps => {
                        for (id, _) in facts
                            .projects
                            .iter()
                            .filter(|(id, _)| id.starts_with("app:"))
                        {
                            out.mark(id, i.reach, true, true, why.clone());
                            out.whole.insert(id.clone());
                        }
                    }
                    Target::Project(p) => {
                        out.mark(p, i.reach, true, true, why.clone());
                        out.whole.insert(p.clone());
                    }
                    Target::Owner => match project_of(facts, f) {
                        Some(p) => {
                            out.mark(p, i.reach, true, true, why.clone());
                            // The project's own file: its package's dependents follow, as for any file of the project.
                            if let Some(c) = facts.cargo.node_of(f) {
                                crates.insert(c.to_string());
                            }
                            if let Some(n) = facts.npm.node_of(f) {
                                packages.insert(n.to_string());
                            }
                        }
                        None => out.unowned.push(f.clone()),
                    },
                }
            }
        }
        // An extractor that cannot read its input puts every project on full (ADR 0117 §2, review round 2 P2-6).
        for why in fail_closed {
            for (id, _) in facts.projects {
                out.mark(id, Reach::Code, true, true, why.clone());
                out.whole.insert(id.clone());
            }
        }
        if named.iter().any(|i| !i.adds) || (model_owned && project_of(facts, f).is_none()) {
            continue;
        }
        // A project folder comes before the owner rules (§2): its prose (README, CHANGELOG, LICENSE, its docs/) is docs for the
        // project, everything else is code.
        if let Some(p) = project_of(facts, f) {
            if dir_of(facts, p).is_some_and(|d| inputs.is_project_prose(d, f)) {
                out.mark(p, Reach::Docs, false, false, f.clone());
                continue;
            }
            let (rust, ui) = halves(facts, f);
            out.mark(p, Reach::Code, rust, ui, f.clone());
            match facts.cargo.node_of(f) {
                Some(c) => {
                    crates.insert(c.to_string());
                }
                // A file of a Cargo workspace's own folder outside every member and every npm package (its Cargo.toml, its
                // .config/) reaches every member.
                None if facts.npm.node_of(f).is_none() => {
                    out.whole.insert(p.to_string());
                    for (d, x) in facts.cargo.iter() {
                        let ws = x.workspace.as_str();
                        if !ws.is_empty() && f.strip_prefix(ws).is_some_and(|r| r.starts_with('/'))
                        {
                            crates.insert(d.to_string());
                        }
                    }
                }
                None => {}
            }
            if let Some(n) = facts.npm.node_of(f) {
                packages.insert(n.to_string());
            }
            continue;
        }
        match inputs.owner_of(f) {
            Some(Owner::None) => out.not_built.push(f.clone()),
            Some(Owner::Doc) if inputs.is_prose(f) || doc_asset(facts, inputs, f) => {
                out.mark("docs", Reach::Docs, false, false, f.clone())
            }
            Some(Owner::Project(p)) => {
                out.mark(p, Reach::Code, true, true, f.clone());
                out.whole.insert(p.clone());
            }
            // Data under a doc owner that no input names, or a path nothing claims.
            Some(Owner::Doc) | None => out.unowned.push(f.clone()),
        }
    }
    if let Some(m) = facts.model {
        for (id, hit) in m.reach_from(&model_seeds) {
            for &kind in &hit.kinds {
                mark_hit(&mut out, &mut crates, facts, m, &id, kind, &hit.why);
            }
        }
        // An unresolved reference (an import, a read, a script path no extractor could resolve) leaves the referrer's edges
        // incomplete, so its projects run in full in every plan: one rule for every extractor (ADR 0117 §4, §14).
        let unresolved: Vec<(String, Option<String>, String)> = m
            .unresolved()
            .iter()
            .map(|u| {
                (
                    u.referrer.clone(),
                    u.node.clone(),
                    format!("unresolved {} in {} ({})", u.reference, u.referrer, u.why),
                )
            })
            .collect();
        out.unresolved_refs = unresolved.iter().map(|(_, _, why)| why.clone()).collect();
        for (file, held_by, why) in &unresolved {
            if let Some(p) = project_of(facts, file) {
                out.mark(p, Reach::Code, true, true, why.clone());
                out.whole.insert(p.to_string());
                if let Some(c) = facts.cargo.node_of(file) {
                    crates.insert(c.to_string());
                }
            }
            let mut seeds: Vec<(String, String)> = m
                .owners_of(file)
                .into_iter()
                .map(|n| (n.id.clone(), why.clone()))
                .collect();
            seeds.extend(held_by.iter().map(|n| (n.clone(), why.clone())));
            for (id, hit) in m.reach_from(&seeds) {
                let mut reasons = hit.why.clone();
                reasons.push(why.clone());
                mark_hit(&mut out, &mut crates, facts, m, &id, Reach::Code, &reasons);
            }
        }
    }
    if !out.unowned.is_empty() {
        out.everything = true;
        if let Some(d) = facts.detect {
            for f in &out.unowned {
                if let Some(k) = d(f) {
                    out.unowned_kind.insert(f.clone(), k);
                }
            }
        }
        let named: Vec<String> = out
            .unowned
            .iter()
            .map(|f| match out.unowned_kind.get(f) {
                Some(k) => format!("{f} ({})", k.name()),
                None => f.clone(),
            })
            .collect();
        let why = format!(
            "unowned {}: the plan answers everything (fail closed; declare an owner or an input)",
            named.join(", ")
        );
        for (id, _) in facts.projects {
            out.mark(id, Reach::Code, true, true, why.clone());
            out.whole.insert(id.clone());
        }
    }
    for d in facts.cargo.dependents(&crates).difference(&crates) {
        if let Some(p) = project_of(facts, d) {
            out.mark(p, Reach::Code, true, false, format!("cargo {d}"));
        }
    }
    out.crates = facts
        .cargo
        .dependents(&crates)
        .union(&crates)
        .cloned()
        .collect();
    // A package nested in another package's folder is part of what the outer one builds (ui builds ui/dist from ui/projects).
    let nested: Vec<String> = packages
        .iter()
        .flat_map(|n| {
            facts
                .npm
                .iter()
                .filter(move |(d, _)| n.strip_prefix(*d).is_some_and(|r| r.starts_with('/')))
                .map(|(d, _)| d.to_string())
        })
        .collect();
    packages.extend(nested);
    for d in facts.npm.dependents(&packages).difference(&packages) {
        if let Some(p) = project_of(facts, d) {
            out.mark(p, Reach::Code, false, true, format!("npm {d}"));
        }
    }
    out
}

/// Marks what one reached model node selects, for one kind that arrived at it (ADR 0117 §5).
fn mark_hit(
    out: &mut Reached,
    crates: &mut BTreeSet<String>,
    facts: &Facts,
    m: &nodes::Model,
    id: &str,
    kind: Reach,
    why: &[String],
) {
    let node = m.node(id);
    let why = why.join("; ");
    // A contract phase (a step node whose home is its project's folder): the phase runs, whatever kind arrived, unless the project
    // runs in full for another reason.
    if let Some(n) = node.filter(|n| n.kind == nodes::NodeKind::Step)
        && let Some((_, phase)) = id.split_once("paddock.ci.json#")
        && let Some(p) = n.home.as_deref().and_then(|h| project_of(facts, h))
    {
        out.phases
            .entry(p.to_string())
            .or_default()
            .insert(phase.to_string());
        out.mark(
            p,
            Reach::Docs,
            false,
            false,
            format!("{id} runs it ({why})"),
        );
        return;
    }
    // A spec reached through a helper or spec it imports: its runner is given the spec.
    if let Some(n) = node.filter(|n| n.kind == nodes::NodeKind::TestFile && kind == Reach::Test)
        && !m.sources(id, nodes::EdgeKind::Tests).is_empty()
        && let Some(p) = n.files.first().and_then(|f| project_of(facts, f))
    {
        for f in &n.files {
            out.test_files
                .entry(p.to_string())
                .or_default()
                .insert(f.clone());
        }
    }
    // A reached test target: its project selects it (and is marked with the kind that arrived).
    if let Some(n) = node.filter(|n| n.kind == nodes::NodeKind::TestTarget)
        && let Some(p) = n.home.as_deref().and_then(|h| project_of(facts, h))
    {
        if kind == Reach::Test {
            out.targets
                .entry(p.to_string())
                .or_default()
                .insert(id.to_string());
        }
        let (rust, ui) = halves(facts, n.home.as_deref().unwrap_or_default());
        out.mark(p, kind, rust, ui, format!("{id} ({why})"));
        return;
    }
    // A crate reached over a `reads` edge (it reads data at build or test time) is code for its project.
    if let Some(dir) = id.strip_prefix("crate:")
        && kind >= Reach::Test
        && why.contains("reads ")
        && let Some(p) = project_of(facts, dir)
    {
        out.mark(
            p,
            Reach::Code,
            true,
            false,
            format!("{id} reads it ({why})"),
        );
        crates.insert(dir.to_string());
        return;
    }
    // A setup-only action changed itself is plumbing for the projects it runs; any other action or script is its users' code (the
    // coordinator's decision of 2026-11-23: only declared setup-only actions are plumbing). A job or step passes on its kind.
    let setup = node.is_some_and(|n| {
        n.setup_only && matches!(n.kind, nodes::NodeKind::Action | nodes::NodeKind::Script)
    });
    let r = if setup && kind == Reach::Code {
        Reach::Plumbing
    } else {
        kind
    };
    for p in m.targets(id, nodes::EdgeKind::Runs) {
        if facts.projects.iter().any(|(pid, _)| pid == p) || p == "c99" {
            if r >= Reach::Release {
                out.whole.insert(p.to_string());
            }
            out.mark(p, r, true, true, format!("{id} runs it ({why})"));
        }
    }
}

/// What a project runs (ADR 0117 §5 `selection`): the union of what every kind that reached it asks (review P1-1). `full` absorbs
/// `smoke` and `affected`; `installers` stands beside either (rule 54: a release change builds them, a code change runs the suite).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Its whole suite.
    pub full: bool,
    /// Its declared smoke set.
    pub smoke: bool,
    /// Only these test targets.
    pub affected: BTreeSet<String>,
    /// Its installers.
    pub installers: bool,
    /// Only these contract phases (their command lines run a changed shared script).
    pub phases: BTreeSet<String>,
}

impl Selection {
    /// Nothing beyond the docs checks.
    pub fn skip(&self) -> bool {
        !self.full
            && !self.smoke
            && self.affected.is_empty()
            && !self.installers
            && self.phases.is_empty()
    }
}

/// What each reached project runs: `code` its whole suite, `test` the reached test targets, `plumbing` its declared smoke set,
/// `release` an app's installers, `docs` nothing, and the union of these over every kind that reached it. Never less than declared
/// (§5): a `plumbing` reach of a project with no smoke set in `smoke`, a `test` reach with no target the model can name, or a
/// `release` reach of a project that is not an app runs the whole suite; a plan that answers everything runs every project in full.
pub fn selection(reached: &Reached, smoke: &BTreeSet<String>) -> BTreeMap<String, Selection> {
    reached
        .nodes
        .iter()
        .map(|(id, n)| {
            let mut s = Selection::default();
            if id != "docs" {
                for k in &n.kinds {
                    match k {
                        Reach::Docs => {}
                        _ if reached.everything => s.full = true,
                        Reach::Code => s.full = true,
                        // Rule 54 builds the installers, and the app's own gates still run: its smoke set, or its suite
                        // without one (never less than declared, §5; the WP-5 replay's 39 release-installers-only cases).
                        Reach::Release if id.starts_with("app:") => {
                            s.installers = true;
                            if smoke.contains(id) {
                                s.smoke = true;
                            } else {
                                s.full = true;
                            }
                        }
                        Reach::Release => s.full = true,
                        Reach::Plumbing if smoke.contains(id) => s.smoke = true,
                        Reach::Plumbing => s.full = true,
                        Reach::Test => match reached.targets.get(id) {
                            Some(t) if !t.is_empty() => s.affected.extend(t.iter().cloned()),
                            _ => s.full = true,
                        },
                    }
                }
            }
            if let Some(p) = reached.phases.get(id) {
                s.phases = p.clone();
            }
            if reached.everything {
                s.full = true;
            }
            if s.full {
                s.smoke = false;
                s.affected.clear();
                s.phases.clear();
            }
            (id.clone(), s)
        })
        .collect()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "union/tests.rs"]
mod tests_union;
