//! The `Extractor` port and its registry (ADR 0117 §15): one adapter per format reads its manifests and sources through the one
//! bulk reader ([`BulkRead`]) and emits nodes and edges. The core knows nodes, edges and queries, never a language; a new language
//! is one adapter and one line in [`extractors`].
//!
//! The reader is the spawn budget's other half (ER-20): every text goes through one [`BulkRead::read_all`] call per adapter, and an
//! adapter declares the processes it may start ([`Extractor::spawn_budget`]); [`build`] sums both so a test can pin them.

use super::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind, Unresolved};
use crate::{CargoMetadata, FileSource, GraphError, cargo_graph, npm_graph};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

/// The one bulk reader of file texts: many paths in one call, an answer per path (`None`: not there or not text).
pub trait BulkRead {
    /// The texts of `paths`, in order.
    fn read_all(&self, paths: &[String]) -> Vec<Option<String>>;
}

/// Files in memory, counting the bulk calls (tests, and callers that hold the texts).
#[derive(Debug, Default)]
pub struct MemoryRead {
    files: BTreeMap<String, String>,
    calls: Cell<usize>,
}

impl MemoryRead {
    /// An empty set.
    pub fn new() -> MemoryRead {
        MemoryRead::default()
    }

    /// Adds a file.
    pub fn with(mut self, path: &str, text: &str) -> MemoryRead {
        self.files.insert(path.to_string(), text.to_string());
        self
    }

    /// Every path (what `git ls-files` would list).
    pub fn paths(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    /// How many bulk calls were made.
    pub fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl BulkRead for MemoryRead {
    fn read_all(&self, paths: &[String]) -> Vec<Option<String>> {
        self.calls.set(self.calls.get() + 1);
        paths.iter().map(|p| self.files.get(p).cloned()).collect()
    }
}

/// The texts one bulk call returned, as the [`FileSource`] the project-level builders take.
struct Prefetched(BTreeMap<String, String>);

impl Prefetched {
    fn of(read: &dyn BulkRead, paths: Vec<String>) -> Prefetched {
        let texts = read.read_all(&paths);
        Prefetched(
            paths
                .into_iter()
                .zip(texts)
                .filter_map(|(p, t)| t.map(|t| (p, t)))
                .collect(),
        )
    }
}

impl FileSource for Prefetched {
    fn read_to_string(&self, path: &str) -> Result<String, GraphError> {
        self.0.get(path).cloned().ok_or_else(|| GraphError::Read {
            path: path.to_string(),
            reason: "not in the bulk read".to_string(),
        })
    }
}

/// What one adapter found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    /// Its nodes.
    pub nodes: Vec<ModelNode>,
    /// Its edges.
    pub edges: Vec<Edge>,
    /// The references it found and could not resolve: [`build`] keeps them in the model, and the plan runs each referrer's
    /// projects in full (ADR 0117 §4, §14: resolved, unresolved or error, never silently no edge).
    pub unresolved: Vec<Unresolved>,
}

/// An adapter could not read a file: that file's project goes on full (ADR 0117 §2, review round 2 P2-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractError {
    /// The adapter.
    pub extractor: &'static str,
    /// The file it could not read.
    pub file: String,
    /// Why.
    pub reason: String,
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} could not read {}: {}",
            self.extractor, self.file, self.reason
        )
    }
}

impl std::error::Error for ExtractError {}

/// One adapter (ADR 0117 §15).
pub trait Extractor {
    /// Its name (the registry key and the plan's reason).
    fn name(&self) -> &'static str;
    /// The file names or suffixes it reads (`Cargo.toml`, `package.json`, `.yml`): [`build`] passes it only the tracked files whose
    /// name ends with one of them, and the owner check and `input_hash` know them.
    fn claims(&self) -> &[&'static str];
    /// Nodes and edges for these files, read through `read` (no spawn of its own beyond [`Extractor::spawn_budget`]).
    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError>;
    /// The processes it may start for these files (`cargo metadata` per workspace).
    fn spawn_budget(&self, files: &[String]) -> usize;
}

/// The model built from every adapter, and what it cost.
#[derive(Debug, Default)]
pub struct Built {
    /// The model.
    pub model: Model,
    /// The adapters that failed, with the file each could not read.
    pub errors: Vec<ExtractError>,
    /// The processes the adapters may have started (the sum of their budgets).
    pub spawns: usize,
}

/// Runs every adapter over the tracked `files` it claims and merges what they found.
pub fn build(files: &[String], read: &dyn BulkRead, adapters: &[Box<dyn Extractor + '_>]) -> Built {
    let mut out = Built::default();
    for a in adapters {
        let mine: Vec<String> = files
            .iter()
            .filter(|f| a.claims().iter().any(|c| f.ends_with(c)))
            .cloned()
            .collect();
        out.spawns += a.spawn_budget(&mine);
        match a.extract(&mine, read) {
            Ok(x) => {
                x.nodes.into_iter().for_each(|n| out.model.add_node(n));
                x.edges.into_iter().for_each(|e| out.model.add_edge(e));
                x.unresolved
                    .into_iter()
                    .for_each(|u| out.model.add_unresolved(u));
            }
            Err(e) => out.errors.push(e),
        }
    }
    out
}

/// The ports the built-in adapters need: `cargo metadata` and the repository root as cargo prints it.
pub struct Ports<'a> {
    /// `cargo metadata` per workspace; `None` leaves the Cargo adapter out (a hostile repository, ADR 0022 §6).
    pub cargo: Option<&'a dyn CargoMetadata>,
    /// The absolute root in the spelling cargo prints.
    pub root: &'a str,
}

/// The registry: every built-in adapter, in order (ADR 0117 §15). The CI, test and import adapters join here in WP-2 and WP-3.
pub fn extractors<'a>(ports: &Ports<'a>) -> Vec<Box<dyn Extractor + 'a>> {
    let mut v: Vec<Box<dyn Extractor + 'a>> = Vec::new();
    if let Some(cargo) = ports.cargo {
        v.push(Box::new(CargoExtractor {
            metadata: cargo,
            root: ports.root.to_string(),
        }));
    }
    v.push(Box::new(NpmExtractor));
    v
}

/// The id of a Cargo crate's node.
pub fn crate_id(dir: &str) -> String {
    format!("crate:{dir}")
}

/// The id of an npm package's node.
pub fn npm_id(dir: &str) -> String {
    format!("npm:{dir}")
}

/// Cargo: a project node per crate (owning its folder) and a `depends` edge per path dependency, from `cargo metadata` once per
/// workspace.
pub struct CargoExtractor<'a> {
    metadata: &'a dyn CargoMetadata,
    root: String,
}

impl<'a> CargoExtractor<'a> {
    /// The adapter over a metadata port.
    pub fn new(metadata: &'a dyn CargoMetadata, root: &str) -> CargoExtractor<'a> {
        CargoExtractor {
            metadata,
            root: root.to_string(),
        }
    }
}

/// Counts the metadata calls an adapter actually makes (the budget test compares it with the declaration).
struct Counting<'a>(&'a dyn CargoMetadata, &'a Cell<usize>);

impl CargoMetadata for Counting<'_> {
    fn metadata(&self, manifest: &str) -> Result<String, GraphError> {
        self.1.set(self.1.get() + 1);
        self.0.metadata(manifest)
    }
}

impl Extractor for CargoExtractor<'_> {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn claims(&self) -> &[&'static str] {
        &["Cargo.toml", ".rs"]
    }

    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError> {
        let manifests: Vec<String> = files
            .iter()
            .filter(|f| f.ends_with("Cargo.toml"))
            .cloned()
            .collect();
        let src = Prefetched::of(read, manifests.clone());
        let calls = Cell::new(0);
        let recording = crate::Recording::new(self.metadata);
        let err = |e: GraphError| ExtractError {
            extractor: "cargo",
            file: match &e {
                GraphError::Read { path, .. } | GraphError::Parse { path, .. } => path.clone(),
                GraphError::Metadata { manifest, .. } => manifest.clone(),
                _ => "Cargo.toml".to_string(),
            },
            reason: e.to_string(),
        };
        let g = cargo_graph(&manifests, &src, &Counting(&recording, &calls), &self.root)
            .map_err(err)?;
        let mut x = from_graph(&g, crate_id, |_| EdgeKind::Depends);
        let mut targets = Vec::new();
        for json in recording.texts.borrow().values() {
            targets.extend(crate::parse_targets(json, &self.root).map_err(err)?);
        }
        test_targets(&mut x, &targets, files);
        Ok(x)
    }

    fn spawn_budget(&self, files: &[String]) -> usize {
        // One `cargo metadata` per workspace root; which manifests are roots is known only after reading them, so the budget is
        // the bound: every claimed manifest (a member manifest is never asked).
        files.iter().filter(|f| f.ends_with("Cargo.toml")).count()
    }
}

/// The test targets of the crates (ADR 0117 §4 `tests`): a node per target nextest runs, with a `depends` edge to its crate (a
/// crate change reaches its tests); a test file per integration test's root (`tests` edge from that target only); and every other
/// `.rs` under a crate's `tests/` folder as a helper of every integration test target of that crate.
fn test_targets(x: &mut Extracted, targets: &[crate::CargoTarget], files: &[String]) {
    let mut roots: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut by_crate: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in targets {
        let Some(bin) = t.binary_id() else { continue };
        let id = format!("nextest:{bin}");
        x.nodes
            .push(ModelNode::new(&id, NodeKind::TestTarget, &[]).with_home(&t.dir));
        x.edges
            .push(Edge::new(&id, crate_id(&t.dir), EdgeKind::Depends));
        if t.kinds.iter().any(|k| k == "test") {
            roots.insert(t.src_path.clone(), (id.clone(), t.dir.clone()));
            by_crate.entry(t.dir.clone()).or_default().push(id);
        }
    }
    for f in files.iter().filter(|f| f.ends_with(".rs")) {
        let file_id = format!("test:{f}");
        if let Some((id, dir)) = roots.get(f) {
            x.nodes
                .push(ModelNode::new(&file_id, NodeKind::TestFile, &[f.as_str()]).with_home(dir));
            x.edges.push(Edge::new(id, &file_id, EdgeKind::Tests));
            continue;
        }
        let Some((dir, ids)) = by_crate
            .iter()
            .filter(|(d, _)| f.starts_with(&format!("{d}/tests/")))
            .max_by_key(|(d, _)| d.len())
        else {
            continue;
        };
        x.nodes
            .push(ModelNode::new(&file_id, NodeKind::TestFile, &[f.as_str()]).with_home(dir));
        for id in ids {
            x.edges.push(Edge::new(id, &file_id, EdgeKind::Tests));
        }
    }
}

/// npm: a project node per package (and per Tauri shell that points into one), a `depends` edge per workspace dependency and a
/// `path-ref` edge per path reference (`frontendDist`, `tsconfig`, `angular.json`, ESLint config).
pub struct NpmExtractor;

impl Extractor for NpmExtractor {
    fn name(&self) -> &'static str {
        "npm"
    }

    fn claims(&self) -> &[&'static str] {
        &[
            "package.json",
            "tauri.conf.json",
            ".json",
            "eslint.config.js",
            "eslint.config.mjs",
            "eslint.config.cjs",
        ]
    }

    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError> {
        let wanted: Vec<String> = files
            .iter()
            .filter(|f| {
                let name = f.rsplit('/').next().unwrap_or(f);
                name == "package.json"
                    || name == "tauri.conf.json"
                    || name == "angular.json"
                    || (name.starts_with("tsconfig") && name.ends_with(".json"))
                    || name.starts_with("eslint.config.")
            })
            .cloned()
            .collect();
        let src = Prefetched::of(read, wanted.clone());
        let err = |e: GraphError| ExtractError {
            extractor: "npm",
            file: match &e {
                GraphError::Read { path, .. } | GraphError::Parse { path, .. } => path.clone(),
                _ => "package.json".to_string(),
            },
            reason: e.to_string(),
        };
        let g = npm_graph(&wanted, &src).map_err(err)?;
        let declared = crate::npm::declared_dependencies(&wanted, &src).map_err(err)?;
        Ok(from_graph(&g, npm_id, |(from, to)| {
            if declared.contains(&(from.to_string(), to.to_string())) {
                EdgeKind::Depends
            } else {
                EdgeKind::PathRef
            }
        }))
    }

    fn spawn_budget(&self, _: &[String]) -> usize {
        0
    }
}

fn from_graph(
    g: &crate::Graph,
    id: fn(&str) -> String,
    kind: impl Fn((&str, &str)) -> EdgeKind,
) -> Extracted {
    let mut x = Extracted::default();
    for (dir, n) in g.iter() {
        x.nodes.push(ModelNode::new(
            id(dir),
            NodeKind::Project,
            &[&format!("{dir}/")],
        ));
        for d in n.deps.iter().collect::<BTreeSet<_>>() {
            x.edges.push(Edge::new(id(dir), id(d), kind((dir, d))));
        }
    }
    x
}

/// A [`BulkRead`] that answers from a fixed reader and records each call's paths (the spawn-budget test).
pub struct Recording<'a> {
    inner: &'a dyn BulkRead,
    /// The paths of each call.
    pub calls: RefCell<Vec<Vec<String>>>,
}

impl<'a> Recording<'a> {
    /// Wraps a reader.
    pub fn new(inner: &'a dyn BulkRead) -> Recording<'a> {
        Recording {
            inner,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl BulkRead for Recording<'_> {
    fn read_all(&self, paths: &[String]) -> Vec<Option<String>> {
        self.calls.borrow_mut().push(paths.to_vec());
        self.inner.read_all(paths)
    }
}

#[cfg(test)]
mod tests;
