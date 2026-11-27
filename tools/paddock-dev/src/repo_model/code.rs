//! The code adapter of the repository model (ADR 0117 §4, WP-3): `reads` edges from Rust sources, `imports` edges between JS/TS
//! scripts, and the spec runners' test targets and test files.
//!
//! - **reads**: a Rust file reads a file or folder through `include_str!`/`include_bytes!`/`include!`, a module's `#[path]`, a
//!   `CARGO_MANIFEST_DIR` join, `concat!` or `format!`, or a build script's `rerun-if-changed` (`super::reads`, `syn`;
//!   a source `syn` cannot parse falls back to [`read_paths`], the text scan §10 names, and the fallback is counted). A path in
//!   another crate gives the reader's crate a `reads` edge to that crate; a path outside every crate (a `docs/` register, a
//!   template, an app's changelog, a .NET fixture project, another crate's test helper) becomes a data node owning exactly that file
//!   or folder with a `reads` edge from the reader, so a change there selects the reader as code and the file's own owner keeps its
//!   reach (review P1-3). A required read (`include*!`, `#[path]`) of an untracked path is an unresolved reference. The candidate
//!   files come from one `git grep -l` (one spawn); only they are read. [`CodeExtractor::base_reads`] reads the base tree too when
//!   the change deleted a file, so a deleted read target still reaches its reader.
//! - **imports**: `oxc_parser` and `oxc_resolver` over the tracked files only (`super::ts`; no `node_modules`, so
//!   local and CI give the same graph). A script node (from the CI adapter) written in JS imports other files; each imported file
//!   becomes a script node with an `imports` edge, transitively, so `conformance/src/*.mjs` reaches the CLI a contract's check line
//!   runs (difference 4 of §8); a workspace package imported by name is an `imports` edge to that package's node. A relative
//!   import that resolves to no tracked file, a computed `import()`, or a file the parser cannot read is an unresolved reference
//!   of its file (`Extracted::unresolved`): the plan runs the referrer's projects in full (ADR 0117 §4, the structural rule).
//! - **test files**: a project contract declares its test files per runner and its helper globs (`graph.tests` in
//!   `paddock.ci.json`, ADR 0117 §3); the runner comes from that declaration, never from the path (review P1-2), and an undeclared
//!   file is no test file. A spec another spec imports, and a declared helper a spec imports (transitively, through other helpers),
//!   get an `imports` edge from each importer, so their change selects the specs that import them. A helper no spec imports stays code (D6: cannot prove), and so does every helper of a
//!   project with an unresolved import in its specs or helpers (the project runs in full on a test reach).

use super::reads::{How, Parsed, rust_reads};
use super::ts::{self, Target};
use paddock_repo_graph::Graph;
use paddock_repo_graph::model::Unresolved;
use paddock_repo_graph::model::extract::{BulkRead, ExtractError, Extracted, Extractor, texts_of};
use paddock_repo_graph::model::glob;
use paddock_repo_graph::model::nodes::{Edge, EdgeKind, ModelNode, NodeKind};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The code adapter.
pub struct CodeExtractor {
    /// The repository root (for the one `git grep`).
    pub root: PathBuf,
    /// The Cargo crates by folder.
    pub cargo: Graph,
    /// The npm packages by folder.
    pub npm: Graph,
    /// The scripts the CI adapter found (their imports are followed).
    pub scripts: BTreeSet<String>,
    /// The projects' declared test files and helpers (`graph.tests` of each contract).
    pub tests: Vec<TestDecl>,
    /// Declared runtime reads (`graph.reads`): a file to the globs of what its computed `import()` loads.
    pub dynamic: BTreeMap<String, Vec<String>>,
    /// The revision the one `git grep` reads (`None`: the working tree, files mode), the revision every text is read at.
    pub rev: Option<String>,
    /// How many processes it started (the spawn-budget test reads it).
    pub spawned: std::rc::Rc<Cell<usize>>,
}

/// One project's declared test files (`graph.tests` in its `paddock.ci.json`), globs over repository paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestDecl {
    /// The project id (`app:dyno`, `conformance`).
    pub project: String,
    /// The project folder.
    pub dir: String,
    /// Runner (`vitest`, `playwright`, `node-test`) to the globs of its test files.
    pub runners: BTreeMap<String, Vec<String>>,
    /// Helper globs: a matching file that a test file imports is test code.
    pub helpers: Vec<String>,
}

/// What [`CodeExtractor`] measured beside its edges (the fallback counts of §10).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeStats {
    /// Rust sources read for `reads`.
    pub rust_sources: usize,
    /// Of them, sources `syn` could not parse (the text scan answered).
    pub syn_fallbacks: usize,
    /// JS/TS files parsed for imports.
    pub parsed: usize,
    /// Projects whose helpers stay code because an import of their tests did not resolve.
    pub unprovable: BTreeSet<String>,
}

/// The string literals of a line (`"..."`, no escapes beyond `\"`).
fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut esc = false;
    for c in line.chars() {
        match (&mut cur, c) {
            (Some(s), _) if esc => {
                s.push(c);
                esc = false;
            }
            (Some(_), '\\') => esc = true,
            (Some(s), '"') => {
                out.push(std::mem::take(s));
                cur = None;
            }
            (Some(s), c) => s.push(c),
            (None, '"') => cur = Some(String::new()),
            (None, _) => {}
        }
    }
    out
}

/// The paths a Rust source reads: (path relative to the repository, a folder or a file), from `include_str!`/`include_bytes!`
/// (relative to the file's folder) and from lines with `CARGO_MANIFEST_DIR` (relative to `crate_dir`).
pub fn read_paths(file: &str, text: &str, crate_dir: &str, tracked: &BTreeSet<String>) -> BTreeSet<String> {
    let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let exists = |p: &str| {
        tracked.contains(p)
            || tracked
                .range(format!("{p}/")..)
                .next()
                .is_some_and(|f| f.starts_with(&format!("{p}/")))
    };
    let mut out = BTreeSet::new();
    let mut manifest_context = false;
    for line in text.lines() {
        let l = line.trim_start();
        if l.starts_with("//") {
            continue;
        }
        let include = l.contains("include_str!") || l.contains("include_bytes!");
        // A `CARGO_MANIFEST_DIR` statement often spans lines (`Path::new(env!(..))\n.join("tests/x")`): the next lines count too.
        if l.contains("CARGO_MANIFEST_DIR") {
            manifest_context = true;
        } else if l.ends_with(';') && !l.contains(".join(") {
            manifest_context = false;
        }
        for lit in literals(l) {
            if lit == "CARGO_MANIFEST_DIR" || lit.len() < 3 || lit.contains("{") {
                continue;
            }
            let rel = lit.trim_start_matches('/');
            let mut tries = Vec::new();
            if include {
                tries.push(crate::gates::shell::join(dir, &lit));
            }
            if manifest_context || include {
                tries.push(crate::gates::shell::join(crate_dir, rel));
            }
            for p in tries.into_iter().flatten() {
                if exists(&p) {
                    out.insert(p);
                }
            }
        }
        if l.ends_with(';') {
            manifest_context = false;
        }
    }
    out
}

/// The files `git grep -l` named. It accepts exit 0, or exit 1 with empty output (nothing matched), and nothing else: any other
/// status (128 is git's own error), or output cut at the cap, is an error and the plan answers everything (review P1-5). With a
/// revision git prefixes each name with `<rev>:`.
pub fn grep_files(code: Option<i32>, stdout: &str, stderr: &str, truncated: bool, rev: Option<&str>) -> Result<Vec<String>, String> {
    let ok = match code {
        Some(0) => true,
        Some(1) => stdout.is_empty() && !truncated,
        _ => false,
    };
    if !ok || truncated {
        let why = if truncated {
            "its output was cut at the cap".to_string()
        } else {
            format!("exit status {code:?}")
        };
        return Err(format!("git grep failed ({why}): {}", stderr.trim()));
    }
    let prefix = rev.map(|r| format!("{r}:"));
    stdout
        .lines()
        .map(|l| match &prefix {
            None => Ok(l.to_string()),
            Some(p) => l
                .strip_prefix(p.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("git grep named {l:?}, which does not start with {p:?}")),
        })
        .collect()
}

/// What `git grep -E` looks for in a Rust source: every construct [`rust_reads`] resolves or marks blind.
pub const READ_PATTERN: &str =
    r"include(_str|_bytes)?!|CARGO_MANIFEST_DIR|CARGO_MANIFEST_PATH|#\[path|rerun-if-changed|ancestors\(|current_dir|ls-files|fs::|File::open|read_to_string|read_dir|\.(exists|is_file|is_dir|try_exists)\(";

/// Text that makes an unparseable source a blind one.
pub const BLIND_TRIGGERS: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    ".ancestors(",
    "current_dir",
    "ls-files",
    "fs::",
    "File::open",
    "read_to_string",
    "read_dir",
    ".exists(",
    ".is_file(",
    ".is_dir(",
    ".try_exists(",
];

/// What a source's reads cannot resolve, one line each (the reference text of a blind read): the [`How::Blind`] reads of a parse, or, for
/// a source `syn` could not parse, one line when its text names a trigger ([`BLIND_TRIGGERS`]). The one definition for the model
/// ([`CodeExtractor::reads_of`]) and for the declared-reads check (C179).
pub fn blind_of(parsed: &Parsed, text: &str) -> Vec<String> {
    match parsed {
        Parsed::Reads(r) => r.iter().filter(|r| r.how == How::Blind).map(|r| r.path.clone()).collect(),
        Parsed::Fallback(e) if BLIND_TRIGGERS.iter().any(|t| text.contains(t)) => {
            vec![format!("source that does not parse ({e}) and reads the crate folder or the tree")]
        }
        Parsed::Fallback(_) => Vec::new(),
    }
}

impl CodeExtractor {
    /// The `.rs` files that may read a path: one `git grep -l` over the tracked tree at `rev` (`None`: the working tree, files
    /// mode).
    fn read_candidates(&self, rev: Option<&str>) -> Result<Vec<String>, String> {
        self.spawned.set(self.spawned.get() + 1);
        let mut cmd = crate::git_env::git();
        cmd.arg("-C")
            .arg(&self.root)
            .args(["-c", "core.quotePath=false", "grep", "-l", "-E", READ_PATTERN]);
        if let Some(rev) = rev {
            cmd.arg(rev);
        }
        cmd.args(["--", "*.rs"]);
        let out = paddock_proc::run_bounded(
            cmd,
            paddock_proc::Limits::new(std::time::Duration::from_secs(60)).max_output(1 << 24),
        )
        .map_err(|e| e.to_string())?;
        let code = match out.outcome {
            paddock_proc::Outcome::Exited(s) => s.code(),
            _ => None,
        };
        grep_files(code, &out.stdout_text(), &out.stderr_text(), out.truncated, rev)
    }

    fn crate_of<'a>(&'a self, path: &str) -> Option<&'a str> {
        self.cargo.node_of(path)
    }

    /// The `reads` edges and data nodes, with the fallback count; a required read (`include*!`, `#[path]`) of a path that is not
    /// tracked is an unresolved reference of its source.
    fn reads(&self, tracked: &BTreeSet<String>, read: &dyn BulkRead, x: &mut Extracted, stats: &mut CodeStats) -> Result<(), ExtractError> {
        let candidates: Vec<String> = self
            .read_candidates(self.rev.as_deref())
            .map_err(|e| ExtractError {
                extractor: "code",
                file: "git grep".into(),
                reason: e,
            })?
            .into_iter()
            .filter(|f| tracked.contains(f))
            .collect();
        // A candidate that cannot be read is an error (fail closed); only a file that is not there is skipped.
        let texts = in_order(&candidates, texts_of("code", read, &candidates)?);
        self.reads_of(&candidates, texts, tracked, x, stats, true);
        Ok(())
    }

    /// The reads of `candidates` (their texts in order) against `tracked`; `unresolved` records required reads that miss.
    pub(crate) fn reads_of(
        &self,
        candidates: &[String],
        texts: Vec<Option<String>>,
        tracked: &BTreeSet<String>,
        x: &mut Extracted,
        stats: &mut CodeStats,
        unresolved: bool,
    ) {
        let exists = |p: &str| {
            tracked.contains(p)
                || tracked
                    .range(format!("{p}/")..)
                    .next()
                    .is_some_and(|f| f.starts_with(&format!("{p}/")))
        };
        let mut data: BTreeSet<String> = x
            .nodes
            .iter()
            .filter_map(|n| n.id.strip_prefix("data:").map(str::to_string))
            .collect();
        let sources: Vec<(&String, String, &str)> = candidates
            .iter()
            .zip(texts)
            .filter_map(|(f, text)| Some((f, text?, self.crate_of(f)?)))
            .collect();
        let parsed = parse_all(&sources);
        for ((f, text, reader), rr) in sources.iter().zip(parsed) {
            let (f, reader) = (*f, *reader);
            stats.rust_sources += 1;
            // The source reads something the extractor cannot follow: its crate is blind (ADR 0116 §2).
            if unresolved {
                for reference in blind_of(&rr, text) {
                    x.blind.push(Unresolved {
                        referrer: f.clone(),
                        node: None,
                        reference,
                        why: "a read the extractor cannot resolve (the test cache never reuses this crate unless it declares test-reads)"
                            .into(),
                    });
                }
            }
            let paths: BTreeSet<String> = match rr {
                Parsed::Reads(r) => {
                    let mut found = BTreeSet::new();
                    for r in r.into_iter().filter(|r| r.how != How::Blind) {
                        if exists(&r.path) {
                            found.insert(r.path);
                        } else if unresolved && r.how.required() {
                            x.unresolved.push(Unresolved {
                                referrer: f.clone(),
                                node: None,
                                reference: r.path,
                                why: "a required read (include*!, #[path]) of a path that is not tracked".into(),
                            });
                        }
                    }
                    found
                }
                Parsed::Fallback(_) => {
                    stats.syn_fallbacks += 1;
                    read_paths(f, text, reader, tracked)
                }
            };
            for p in paths {
                // A path of the reader's own crate is its own code already; any other path gets a data node owning exactly it (a
                // folder: every file under it), so the node that owns the file itself (a test file, a package) keeps its own
                // reach and the reader is reached too (review P1-3).
                if self.crate_of(&p) == Some(reader) {
                    continue;
                }
                let id = data_id(&p);
                if data.insert(p.clone()) {
                    let own = if tracked.contains(&p) { p.clone() } else { format!("{p}/") };
                    x.nodes.push(ModelNode::new(&id, NodeKind::Project, &[own.as_str()]));
                }
                x.edges.push(Edge::new(format!("crate:{reader}"), id, EdgeKind::Reads));
            }
        }
    }

    /// The reads at the base revision, for the changed files the head no longer has (review P1-3: a deleted read target keeps its
    /// reader). One `git grep` and one bulk read at the base, only when something was deleted; a failed grep or read is an error
    /// (fail closed: the plan answers everything).
    pub fn base_reads(
        &self,
        base: &str,
        base_files: &BTreeSet<String>,
        read: &dyn BulkRead,
        deleted: &BTreeSet<String>,
    ) -> Result<Extracted, ExtractError> {
        let mut x = Extracted::default();
        if deleted.is_empty() {
            return Ok(x);
        }
        let candidates = self.read_candidates(Some(base)).map_err(|e| ExtractError {
            extractor: "code",
            file: format!("git grep at {base}"),
            reason: e,
        })?;
        let candidates: Vec<String> = candidates.into_iter().filter(|f| base_files.contains(f)).collect();
        let texts = in_order(&candidates, texts_of("code", read, &candidates)?);
        let mut stats = CodeStats::default();
        self.reads_of(&candidates, texts, base_files, &mut x, &mut stats, false);
        // Keep only the data nodes that name a deleted file (or a folder holding one), owning exactly the deleted files.
        let keep: BTreeSet<String> = x
            .nodes
            .iter()
            .filter(|n| {
                n.files.iter().any(|f| {
                    deleted
                        .iter()
                        .any(|d| d == f || f.strip_suffix('/').is_some_and(|dir| d.starts_with(&format!("{dir}/"))))
                })
            })
            .map(|n| n.id.clone())
            .collect();
        x.nodes.retain(|n| keep.contains(&n.id));
        for n in &mut x.nodes {
            let owned: Vec<String> = deleted
                .iter()
                .filter(|d| {
                    n.files
                        .iter()
                        .any(|f| f == *d || f.strip_suffix('/').is_some_and(|dir| d.starts_with(&format!("{dir}/"))))
                })
                .cloned()
                .collect();
            n.files = owned;
        }
        x.edges.retain(|e| keep.contains(&e.to));
        Ok(x)
    }
}

/// How many threads parse the Rust sources at once. `syn` parses the about 430 candidate sources (2.5 MB) in about 0.7 s of one
/// core in a debug build (2026-11-24, docs/benchmarks.md); the parse is pure, so a few workers cut the wait, and more than four would
/// crowd the other tests of a run (ER-48), as the replay's [`super::replay::WORKERS`].
pub const PARSE_WORKERS: usize = 4;

/// [`rust_reads`] of each `(file, text, crate folder)`, in order, in up to [`PARSE_WORKERS`] threads (results do not depend on the
/// thread count). A worker that panicked is parsed again on this thread, so its panic surfaces here as it did before.
fn parse_all(sources: &[(&String, String, &str)]) -> Vec<Parsed> {
    let workers = PARSE_WORKERS.min(sources.len()).max(1);
    let mut out: Vec<Option<Parsed>> = vec![None; sources.len()];
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|w| {
                s.spawn(move || {
                    sources
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| i % workers == w)
                        .map(|(i, (f, text, dir))| (i, rust_reads(f, text, dir)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for h in handles {
            for (i, p) in h.join().unwrap_or_default() {
                out[i] = Some(p);
            }
        }
    });
    out.into_iter()
        .zip(sources)
        .map(|(p, (f, text, dir))| p.unwrap_or_else(|| rust_reads(f, text, dir)))
        .collect()
}

/// The texts of `paths` in their order (absent files `None`), from the map [`texts_of`] answered.
fn in_order(paths: &[String], mut texts: BTreeMap<String, String>) -> Vec<Option<String>> {
    paths.iter().map(|p| texts.remove(p)).collect()
}

/// The id of a data node: a file or folder read at build or test time from outside every crate.
pub fn data_id(path: &str) -> String {
    format!("data:{path}")
}

/// The test runner and target of a file, or `None` when it is no test file: a declared runner glob first, else the naming rule
/// (undeclared projects only).
fn test_target(f: &str, decls: &[TestDecl], npm: &Graph) -> Option<(String, String)> {
    let home = |runner: &str, project_dir: &str| -> (String, String) {
        match npm.node_of(f).filter(|_| runner == "vitest" || runner == "playwright") {
            Some(pkg) => (format!("{runner}:{pkg}"), pkg.to_string()),
            None => (format!("{runner}:{project_dir}"), project_dir.to_string()),
        }
    };
    let declared = decls
        .iter()
        .filter(|d| f.strip_prefix(d.dir.as_str()).is_some_and(|r| r.starts_with('/')))
        .max_by_key(|d| d.dir.len());
    // The runner comes from the contract, never from the path (review P1-2: paddock-e2e's selftest specs are Playwright); an
    // undeclared test file is no test file, so its change runs its project in full (never less than declared, §5).
    let d = declared?;
    d.runners
        .iter()
        .find(|(_, globs)| globs.iter().any(|g| glob::matches(g, f)))
        .map(|(runner, _)| home(runner, &d.dir))
}

/// The unresolved imports and unreadable files of an import walk, as unresolved references of their files (ADR 0117 §4, §14). A
/// computed `import()` of a file whose contract declares what it loads (`graph.reads`) becomes edges to those files instead.
fn unresolved_of(g: &ts::ImportGraph, x: &mut Extracted, dynamic: &BTreeMap<String, Vec<String>>, tracked: &BTreeSet<String>) {
    for (f, specs) in &g.unresolved {
        for s in specs {
            if s.contains("dynamic import")
                && let Some(globs) = dynamic.get(f)
            {
                let hits: Vec<&String> = tracked.iter().filter(|t| globs.iter().any(|gl| glob::matches(gl, t))).collect();
                if !hits.is_empty() {
                    for t in hits {
                        let id = data_id(t);
                        if !x.nodes.iter().any(|n| n.id == id) {
                            x.nodes.push(ModelNode::new(&id, NodeKind::Project, &[t.as_str()]));
                        }
                        for from in [format!("script:{f}"), format!("test:{f}")] {
                            x.edges.push(Edge::new(from, &id, EdgeKind::Imports));
                        }
                    }
                    continue;
                }
            }
            x.unresolved.push(Unresolved {
                referrer: f.clone(),
                node: None,
                reference: s.clone(),
                why: "an import that resolves to no tracked file".into(),
            });
        }
    }
    for (f, e) in &g.errors {
        x.unresolved.push(Unresolved {
            referrer: f.clone(),
            node: None,
            reference: "(the whole file)".into(),
            why: e.clone(),
        });
    }
}

impl Extractor for CodeExtractor {
    fn name(&self) -> &'static str {
        "code"
    }

    fn claims(&self) -> &[&'static str] {
        &[""]
    }

    fn extract(&self, files: &[String], read: &dyn BulkRead) -> Result<Extracted, ExtractError> {
        self.extract_with_stats(files, read).map(|(x, _)| x)
    }

    fn spawn_budget(&self, _: &[String]) -> usize {
        1
    }
}

impl CodeExtractor {
    /// [`Extractor::extract`] with what it measured.
    pub fn extract_with_stats(&self, files: &[String], read: &dyn BulkRead) -> Result<(Extracted, CodeStats), ExtractError> {
        let tracked: BTreeSet<String> = files.iter().cloned().collect();
        let mut x = Extracted::default();
        let mut stats = CodeStats::default();
        self.reads(&tracked, read, &mut x, &mut stats)?;

        // The resolver over the tracked files and the texts it reads itself (package manifests, TypeScript configs): one bulk read.
        let config_files: Vec<String> = files
            .iter()
            .filter(|f| ts::resolver_reads(f) && !f.contains("/node_modules/"))
            .cloned()
            .collect();
        let texts: BTreeMap<String, String> = texts_of("code", read, &config_files)?;
        let packages: BTreeMap<String, String> = self
            .npm
            .iter()
            .filter(|(_, n)| !n.name.is_empty())
            .map(|(d, n)| (n.name.clone(), d.to_string()))
            .collect();
        let resolver = ts::Resolver::new(&tracked, texts, packages);
        // The import walks read through this: a failed read is remembered and fails the extractor (fail closed) after the walk;
        // an absent file is `None` (an unresolved import).
        let failed: RefCell<Option<ExtractError>> = RefCell::new(None);
        let bulk = |paths: &[String]| -> Vec<Option<String>> {
            paths
                .iter()
                .zip(read.read_all(paths))
                .map(|(p, r)| match r {
                    Ok(t) => t,
                    Err(reason) => {
                        failed.borrow_mut().get_or_insert(ExtractError {
                            extractor: "code",
                            file: p.clone(),
                            reason,
                        });
                        None
                    }
                })
                .collect()
        };

        // imports between JS scripts, transitively from the CI adapter's scripts
        let seeds: Vec<String> = self
            .scripts
            .iter()
            .filter(|s| ts::is_source(s) && tracked.contains(*s))
            .cloned()
            .collect();
        for s in &seeds {
            x.nodes.push(ModelNode::new(format!("script:{s}"), NodeKind::Script, &[s.as_str()]));
        }
        let g = ts::import_graph(&seeds, &resolver, &bulk, &|p| !p.contains("/node_modules/"));
        if let Some(e) = failed.borrow_mut().take() {
            return Err(e);
        }
        stats.parsed += g.edges.len() + g.errors.len();
        unresolved_of(&g, &mut x, &self.dynamic, &tracked);
        for (from, tos) in &g.edges {
            for t in tos {
                match t {
                    Target::File(to) => {
                        if !self.scripts.contains(to) {
                            x.nodes
                                .push(ModelNode::new(format!("script:{to}"), NodeKind::Script, &[to.as_str()]));
                        }
                        x.edges
                            .push(Edge::new(format!("script:{from}"), format!("script:{to}"), EdgeKind::Imports));
                    }
                    Target::Package(dir) => {
                        x.edges
                            .push(Edge::new(format!("script:{from}"), format!("npm:{dir}"), EdgeKind::Imports));
                    }
                    Target::External(_) | Target::Builtin => {}
                }
            }
        }

        // test files and their targets
        let mut specs: BTreeMap<String, (String, String)> = BTreeMap::new();
        for f in files.iter().filter(|f| !f.contains("/node_modules/")) {
            if let Some(t) = test_target(f, &self.tests, &self.npm) {
                specs.insert(f.clone(), t);
            }
        }
        for (f, (target, home)) in &specs {
            if !x.nodes.iter().any(|n| &n.id == target) {
                x.nodes.push(ModelNode::new(target, NodeKind::TestTarget, &[]).with_home(home));
            }
            x.nodes
                .push(ModelNode::new(format!("test:{f}"), NodeKind::TestFile, &[f.as_str()]).with_home(home));
            x.edges.push(Edge::new(target, format!("test:{f}"), EdgeKind::Tests));
        }
        // imports among a project's test files: spec to spec (review P1-2) and spec to its declared helpers
        for d in &self.tests {
            let mine: Vec<String> = specs
                .keys()
                .filter(|f| f.strip_prefix(d.dir.as_str()).is_some_and(|r| r.starts_with('/')) && ts::is_source(f))
                .cloned()
                .collect();
            let helper = |p: &str| d.helpers.iter().any(|h| glob::matches(h, p)) && !specs.contains_key(p);
            let g = ts::import_graph(&mine, &resolver, &bulk, &|p| helper(p));
            if let Some(e) = failed.borrow_mut().take() {
                return Err(e);
            }
            stats.parsed += g.edges.len() + g.errors.len();
            if !g.unresolved.is_empty() || !g.errors.is_empty() {
                stats.unprovable.insert(d.project.clone());
                unresolved_of(&g, &mut x, &self.dynamic, &tracked);
                continue;
            }
            for (from, tos) in &g.edges {
                for to in tos.iter().filter_map(|t| match t {
                    Target::File(p) if helper(p) || specs.contains_key(p) => Some(p),
                    _ => None,
                }) {
                    if !specs.contains_key(to) {
                        x.nodes
                            .push(ModelNode::new(format!("test:{to}"), NodeKind::TestFile, &[to.as_str()]).with_home(&d.dir));
                    }
                    x.edges
                        .push(Edge::new(format!("test:{from}"), format!("test:{to}"), EdgeKind::Imports));
                }
            }
        }
        x.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        x.nodes.dedup_by(|a, b| a.id == b.id);
        Ok((x, stats))
    }
}

#[cfg(test)]
mod tests;
