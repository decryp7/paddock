//! The typed nodes and edges of the repository model (ADR 0117 §2, §4) and the reverse closure over them (§5 `reach`).
//!
//! An edge `from → to` says `from` needs `to` (a crate depends on a crate, a job uses an action, a test target reads a file), so a
//! change to `to` reaches `from`. The kind that arrives at a node is decided by [`carry`]: a path's kind is the kind of its first
//! edge out of the changed file's owner (an action or script reached through `uses` or `runs` is plumbing, a test file is test,
//! everything else is code), and a `uses` edge out of a release job turns plumbing into release unless the action is declared
//! setup-only.

use super::Reach;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The kinds of node (ADR 0117 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeKind {
    /// A Cargo crate, an npm package, an app or a root project.
    Project,
    /// A workflow file.
    Workflow,
    /// One job of a workflow.
    Job,
    /// One step of a job.
    Step,
    /// A local action (`.gitea/actions/<name>`).
    Action,
    /// A script a step or another script runs.
    Script,
    /// A test source file.
    TestFile,
    /// A runnable test target (a nextest binary, a Vitest project, a Playwright project and engine).
    TestTarget,
    /// A generator of `docs/generators.toml`.
    Generator,
    /// Documentation.
    Doc,
}

impl NodeKind {
    /// Every kind, in order (the positive-control table walks it).
    pub const ALL: [NodeKind; 10] = [
        NodeKind::Project,
        NodeKind::Workflow,
        NodeKind::Job,
        NodeKind::Step,
        NodeKind::Action,
        NodeKind::Script,
        NodeKind::TestFile,
        NodeKind::TestTarget,
        NodeKind::Generator,
        NodeKind::Doc,
    ];

    /// The word of the plan output.
    pub fn name(self) -> &'static str {
        match self {
            NodeKind::Project => "project",
            NodeKind::Workflow => "workflow",
            NodeKind::Job => "job",
            NodeKind::Step => "step",
            NodeKind::Action => "action",
            NodeKind::Script => "script",
            NodeKind::TestFile => "test-file",
            NodeKind::TestTarget => "test-target",
            NodeKind::Generator => "generator",
            NodeKind::Doc => "doc",
        }
    }
}

/// The kinds of edge (ADR 0117 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    /// A crate on a crate, a package on a package.
    Depends,
    /// A project on another through a path in its config (`frontendDist`, `tsconfig`, `angular.json`, ESLint).
    PathRef,
    /// A workflow, job, step or action on an action or a reusable workflow.
    Uses,
    /// A step on a script, a project phase or a test target; a contract check line on a script.
    Runs,
    /// A target on a data file or folder it reads at build or test time.
    Reads,
    /// A script on a script it runs.
    ReadsScript,
    /// A script or test file on a module or package it imports.
    Imports,
    /// A test target on a test file.
    Tests,
    /// A generator on an input node.
    Input,
    /// A generator's output on the generator.
    Output,
    /// A node on an OS mark.
    Os,
}

impl EdgeKind {
    /// Every kind, in order (the positive-control table walks it).
    pub const ALL: [EdgeKind; 11] = [
        EdgeKind::Depends,
        EdgeKind::PathRef,
        EdgeKind::Uses,
        EdgeKind::Runs,
        EdgeKind::Reads,
        EdgeKind::ReadsScript,
        EdgeKind::Imports,
        EdgeKind::Tests,
        EdgeKind::Input,
        EdgeKind::Output,
        EdgeKind::Os,
    ];

    /// The word of the plan output.
    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::Depends => "depends",
            EdgeKind::PathRef => "path-ref",
            EdgeKind::Uses => "uses",
            EdgeKind::Runs => "runs",
            EdgeKind::Reads => "reads",
            EdgeKind::ReadsScript => "reads-script",
            EdgeKind::Imports => "imports",
            EdgeKind::Tests => "tests",
            EdgeKind::Input => "input",
            EdgeKind::Output => "output",
            EdgeKind::Os => "os",
        }
    }
}

/// One node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelNode {
    /// Its id (`crate:crates/paddock-proc`, `job:ci.yml#crates`, `action:setup-rust`).
    pub id: String,
    /// Its kind.
    pub kind: NodeKind,
    /// The files it owns: exact paths, or folders written with a trailing `/` (everything under them). The longest match owns a
    /// file.
    pub files: Vec<String>,
    /// A job or step that bundles, signs or publishes (a `uses` edge out of it carries `release`).
    pub release: bool,
    /// An action declared setup-only (`setup-rust`, the cache actions): plumbing even for a release job (rule 54).
    pub setup_only: bool,
}

impl ModelNode {
    /// A node of `kind` owning `files`.
    pub fn new(id: impl Into<String>, kind: NodeKind, files: &[&str]) -> ModelNode {
        ModelNode {
            id: id.into(),
            kind,
            files: files.iter().map(|s| s.to_string()).collect(),
            release: false,
            setup_only: false,
        }
    }
}

/// One edge: `from` needs `to`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    /// The node that needs.
    pub from: String,
    /// The node it needs.
    pub to: String,
    /// How.
    pub kind: EdgeKind,
}

impl Edge {
    /// An edge.
    pub fn new(from: impl Into<String>, to: impl Into<String>, kind: EdgeKind) -> Edge {
        Edge {
            from: from.into(),
            to: to.into(),
            kind,
        }
    }
}

/// How a node is reached, with the reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The strongest kind over every path that arrives.
    pub reach: Reach,
    /// One line per path: the changed file, or the edge it came over.
    pub why: Vec<String>,
}

/// The model: nodes by id and edges, with the reverse index built on insert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Model {
    nodes: BTreeMap<String, ModelNode>,
    edges: BTreeSet<Edge>,
    rev: BTreeMap<String, BTreeSet<(String, EdgeKind)>>,
}

impl Model {
    /// An empty model.
    pub fn new() -> Model {
        Model::default()
    }

    /// Adds or replaces a node.
    pub fn add_node(&mut self, n: ModelNode) {
        self.nodes.insert(n.id.clone(), n);
    }

    /// Adds an edge (its ends need not exist yet; an edge to a missing node reaches nothing).
    pub fn add_edge(&mut self, e: Edge) {
        self.rev
            .entry(e.to.clone())
            .or_default()
            .insert((e.from.clone(), e.kind));
        self.edges.insert(e);
    }

    /// The node with this id.
    pub fn node(&self, id: &str) -> Option<&ModelNode> {
        self.nodes.get(id)
    }

    /// Every node, by id.
    pub fn nodes(&self) -> impl Iterator<Item = &ModelNode> {
        self.nodes.values()
    }

    /// Every edge, sorted.
    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter()
    }

    /// The node that owns `path`: the longest file entry that equals it or is a folder holding it (a tie: the first id).
    pub fn owner_of(&self, path: &str) -> Option<&ModelNode> {
        let mut best: Option<(usize, &ModelNode)> = None;
        for n in self.nodes.values() {
            for f in &n.files {
                let hit = if let Some(dir) = f.strip_suffix('/') {
                    path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
                } else {
                    f == path
                };
                if hit && best.is_none_or(|(l, _)| f.len() > l) {
                    best = Some((f.len(), n));
                }
            }
        }
        best.map(|(_, n)| n)
    }

    /// Every node a change reaches (ADR 0117 §5): the owners of `changed`, then the reverse closure over the edges, with the kind
    /// [`carry`] gives each path. Bulk: one call for the whole changed set. Files no node owns are returned apart (the caller's
    /// project level and owner rules decide them).
    pub fn reach(&self, changed: &[String]) -> (BTreeMap<String, Hit>, Vec<String>) {
        let mut seeds = Vec::new();
        let mut unowned = Vec::new();
        for f in changed {
            match self.owner_of(f) {
                Some(n) => seeds.push((n.id.clone(), f.clone())),
                None => unowned.push(f.clone()),
            }
        }
        (self.reach_from(&seeds), unowned)
    }

    /// The reverse closure from seed nodes, each with the reason it is a seed (the changed file, or the job a workflow edit
    /// changed): the seed gets its own kind ([`own_reach`]) and the paths out of it get theirs ([`carry`]).
    pub fn reach_from(&self, seeds: &[(String, String)]) -> BTreeMap<String, Hit> {
        let mut hits: BTreeMap<String, Hit> = BTreeMap::new();
        // (node, the kind that arrives, the kind of the seed node, the seed's setup-only flag, first hop)
        let mut queue: VecDeque<(String, Reach, NodeKind, bool, bool)> = VecDeque::new();
        let put = |hits: &mut BTreeMap<String, Hit>, id: &str, r: Reach, why: String| -> bool {
            let h = hits.entry(id.to_string()).or_insert(Hit {
                reach: r,
                why: Vec::new(),
            });
            let grew = r > h.reach || h.why.is_empty();
            h.reach = h.reach.max(r);
            if !h.why.contains(&why) {
                h.why.push(why);
            }
            grew
        };
        for (id, why) in seeds {
            let Some(n) = self.nodes.get(id) else {
                continue;
            };
            let own = own_reach(n.kind);
            if put(&mut hits, &n.id, own, why.clone()) {
                queue.push_back((n.id.clone(), own, n.kind, n.setup_only, true));
            }
        }
        while let Some((id, r, seed, setup_only, first)) = queue.pop_front() {
            for (from, kind) in self.rev.get(&id).into_iter().flatten() {
                let Some(up) = self.nodes.get(from) else {
                    continue;
                };
                let next = carry(r, seed, *kind, up, setup_only, first);
                let why = format!("{} {id}", kind.name());
                if put(&mut hits, from, next, why) {
                    queue.push_back((from.clone(), next, seed, setup_only, false));
                }
            }
        }
        hits
    }

    /// The nodes `id` has an edge of `kind` to (forward: what it needs).
    pub fn targets(&self, id: &str, kind: EdgeKind) -> Vec<&str> {
        self.edges
            .range(Edge::new(id, "", EdgeKind::Depends)..)
            .take_while(|e| e.from == id)
            .filter(|e| e.kind == kind)
            .map(|e| e.to.as_str())
            .collect()
    }
}

/// The kind a changed file gives its own node: a test file is test, documentation docs, anything else code (an action's or a
/// script's own files are code for it; its users see plumbing through [`carry`]).
fn own_reach(kind: NodeKind) -> Reach {
    match kind {
        NodeKind::TestFile => Reach::Test,
        NodeKind::Doc => Reach::Docs,
        _ => Reach::Code,
    }
}

/// The kind that arrives at `up` over one edge `kind`, coming with `r` from a path that started at a node of kind `seed`.
///
/// - The first hop out of an action or script over `uses`, `runs` or `reads-script` is plumbing; into a release job or step (not
///   for a setup-only action) it is release.
/// - A plumbing path stays plumbing, except over `uses` into a release node (release, unless the seed is setup-only).
/// - Every other kind arrives as it left (a test file's `test` reaches its target as test; a read data file is code).
pub fn carry(
    r: Reach,
    seed: NodeKind,
    kind: EdgeKind,
    up: &ModelNode,
    setup_only: bool,
    first: bool,
) -> Reach {
    let ci_edge = matches!(
        kind,
        EdgeKind::Uses | EdgeKind::Runs | EdgeKind::ReadsScript
    );
    let from_plumbing = matches!(seed, NodeKind::Action | NodeKind::Script);
    let release = up.release && !setup_only && matches!(kind, EdgeKind::Uses | EdgeKind::Runs);
    if first && from_plumbing && ci_edge {
        return if release {
            Reach::Release
        } else {
            Reach::Plumbing
        };
    }
    if r == Reach::Plumbing && release {
        return Reach::Release;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        let mut m = Model::new();
        m.add_node(ModelNode::new(
            "action:cache",
            NodeKind::Action,
            &[".gitea/actions/cache/"],
        ));
        let mut setup = ModelNode::new(
            "action:setup-rust",
            NodeKind::Action,
            &[".gitea/actions/setup-rust/"],
        );
        setup.setup_only = true;
        m.add_node(setup);
        m.add_node(ModelNode::new("job:ci.yml#crates", NodeKind::Job, &[]));
        let mut bundle = ModelNode::new("job:dyno.yml#bundle", NodeKind::Job, &[]);
        bundle.release = true;
        m.add_node(bundle);
        m.add_node(ModelNode::new(
            "wf:ci.yml",
            NodeKind::Workflow,
            &[".gitea/workflows/ci.yml"],
        ));
        m.add_node(ModelNode::new(
            "test:a.spec.ts",
            NodeKind::TestFile,
            &["ui/a.spec.ts"],
        ));
        m.add_node(ModelNode::new("vitest:ui", NodeKind::TestTarget, &[]));
        m.add_node(ModelNode::new(
            "doc:README.md",
            NodeKind::Doc,
            &["README.md"],
        ));
        for (from, to, k) in [
            ("job:ci.yml#crates", "action:cache", EdgeKind::Uses),
            ("job:ci.yml#crates", "action:setup-rust", EdgeKind::Uses),
            ("job:dyno.yml#bundle", "action:cache", EdgeKind::Uses),
            ("job:dyno.yml#bundle", "action:setup-rust", EdgeKind::Uses),
            ("wf:ci.yml", "job:ci.yml#crates", EdgeKind::Uses),
            ("vitest:ui", "test:a.spec.ts", EdgeKind::Tests),
        ] {
            m.add_edge(Edge::new(from, to, k));
        }
        m
    }

    fn reach_of(m: &Model, f: &[&str]) -> BTreeMap<String, Reach> {
        let changed: Vec<String> = f.iter().map(|s| s.to_string()).collect();
        m.reach(&changed)
            .0
            .into_iter()
            .map(|(k, h)| (k, h.reach))
            .collect()
    }

    #[test]
    fn a_shared_action_is_plumbing_for_its_users_and_release_for_a_bundle_job() {
        let m = model();
        let r = reach_of(&m, &[".gitea/actions/cache/action.yml"]);
        assert_eq!(r["action:cache"], Reach::Code);
        assert_eq!(r["job:ci.yml#crates"], Reach::Plumbing);
        assert_eq!(r["wf:ci.yml"], Reach::Plumbing, "plumbing carries on");
        assert_eq!(r["job:dyno.yml#bundle"], Reach::Release);
    }

    #[test]
    fn a_setup_only_action_is_plumbing_even_for_a_release_job() {
        let r = reach_of(&model(), &[".gitea/actions/setup-rust/action.yml"]);
        assert_eq!(r["job:dyno.yml#bundle"], Reach::Plumbing);
        // Control: the same action without the declaration is release there.
        let mut m = model();
        let mut n = m.node("action:setup-rust").cloned().unwrap();
        n.setup_only = false;
        m.add_node(n);
        assert_eq!(
            reach_of(&m, &[".gitea/actions/setup-rust/action.yml"])["job:dyno.yml#bundle"],
            Reach::Release
        );
    }

    #[test]
    fn a_test_file_reaches_its_target_as_test_and_a_doc_as_docs() {
        let r = reach_of(&model(), &["ui/a.spec.ts", "README.md"]);
        assert_eq!(r["vitest:ui"], Reach::Test);
        assert_eq!(r["doc:README.md"], Reach::Docs);
    }

    #[test]
    fn the_longest_file_entry_owns_and_unowned_files_come_back() {
        let mut m = model();
        m.add_node(ModelNode::new(
            "action:cache-readme",
            NodeKind::Doc,
            &[".gitea/actions/cache/README.md"],
        ));
        assert_eq!(
            m.owner_of(".gitea/actions/cache/README.md").unwrap().id,
            "action:cache-readme"
        );
        assert_eq!(
            m.owner_of(".gitea/actions/cache/x.sh").unwrap().id,
            "action:cache"
        );
        assert!(
            m.owner_of(".gitea/actions/cachex/x.sh").is_none(),
            "a folder entry needs a separator"
        );
        let (_, unowned) = m.reach(&["nowhere.txt".to_string()]);
        assert_eq!(unowned, ["nowhere.txt"]);
    }

    #[test]
    fn the_kind_lists_name_every_variant_once() {
        let n: BTreeSet<&str> = NodeKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(n.len(), NodeKind::ALL.len());
        let e: BTreeSet<&str> = EdgeKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(e.len(), EdgeKind::ALL.len());
    }
}
