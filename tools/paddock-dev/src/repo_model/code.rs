//! The code adapter of the repository model (ADR 0117 §4, WP-3): `reads` edges from Rust sources, `imports` edges between JS/TS
//! scripts, and the spec runners' test targets and test files.
//!
//! - **reads**: a Rust file that names a path with `include_str!`/`include_bytes!` (relative to the file), or with
//!   `CARGO_MANIFEST_DIR` (`concat!(env!("CARGO_MANIFEST_DIR"), "/tests/x")`, `Path::new(env!(..)).join("tests/x")`, relative to its
//!   crate), reads that file or folder. When the path lies in another crate, the reader's crate gets a `reads` edge to that crate,
//!   so a change there selects the reader (ADR 0117 §4; `debrief-plugins` reading `debrief-plugin-windows` fixtures). The candidate
//!   files come from one `git grep -l` (one spawn); only they are read. A text scan, the fallback §10 names, not `syn`.
//! - **imports**: a script node (from the CI adapter) written in JS imports other files by relative specifiers; each imported file
//!   becomes a script node with an `imports` edge, transitively, so `conformance/src/*.mjs` reaches the CLI a contract's check line
//!   runs (difference 4 of §8). The scanner is `paddock_repo_graph::imports` (relative specifiers; no `node_modules`).
//! - **spec targets**: `*.spec.ts`, `*.test.ts`, `*.spec.mjs` and `*.test.mjs` in an npm package are test files of that package's
//!   Vitest target, or of its Playwright target under an `e2e/` folder; `conformance/test/*.test.mjs` of the conformance node
//!   tests. A helper a spec imports stays code (a helper no spec proves narrows nothing, D6).

use paddock_repo_graph::Graph;
use paddock_repo_graph::imports::{relative_specifiers, resolve};
use paddock_repo_graph::model::extract::{BulkRead, ExtractError, Extracted, Extractor};
use paddock_repo_graph::model::nodes::{Edge, EdgeKind, ModelNode, NodeKind};
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
}

const JS: [&str; 5] = [".mjs", ".cjs", ".js", ".ts", ".mts"];

fn is_spec(f: &str) -> bool {
    [".spec.ts", ".test.ts", ".spec.mjs", ".test.mjs"].iter().any(|e| f.ends_with(e)) && !f.contains("/node_modules/")
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

impl CodeExtractor {
    /// The `.rs` files that may read a path: one `git grep -l` over the tracked tree.
    fn read_candidates(&self) -> Result<Vec<String>, String> {
        let mut cmd = paddock_proc::command("git");
        cmd.arg("-C").arg(&self.root).args([
            "-c",
            "core.quotePath=false",
            "grep",
            "-l",
            "-E",
            "include_(str|bytes)!|CARGO_MANIFEST_DIR",
            "--",
            "*.rs",
        ]);
        let out = paddock_proc::run_bounded(
            cmd,
            paddock_proc::Limits::new(std::time::Duration::from_secs(60)).max_output(1 << 24),
        )
        .map_err(|e| e.to_string())?;
        // `git grep` exits 1 when nothing matches.
        if !out.success() && !out.stdout_text().is_empty() {
            return Err(out.stderr_text());
        }
        Ok(out.stdout_text().lines().map(str::to_string).collect())
    }

    fn crate_of<'a>(&'a self, path: &str) -> Option<&'a str> {
        self.cargo.node_of(path)
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
        let tracked: BTreeSet<String> = files.iter().cloned().collect();
        let bad = |file: &str, reason: String| ExtractError {
            extractor: "code",
            file: file.to_string(),
            reason,
        };
        let mut x = Extracted::default();
        // reads
        let candidates: Vec<String> = self
            .read_candidates()
            .map_err(|e| bad("git grep", e))?
            .into_iter()
            .filter(|f| tracked.contains(f))
            .collect();
        for (f, text) in candidates.iter().zip(read.read_all(&candidates)) {
            let (Some(text), Some(reader)) = (text, self.crate_of(f)) else {
                continue;
            };
            for p in read_paths(f, &text, reader, &tracked) {
                if let Some(owner) = self.crate_of(&p).filter(|o| *o != reader) {
                    x.edges
                        .push(Edge::new(format!("crate:{reader}"), format!("crate:{owner}"), EdgeKind::Reads));
                }
            }
        }
        // imports between JS scripts, transitively from the CI adapter's scripts
        let js: BTreeSet<String> = files
            .iter()
            .filter(|f| JS.iter().any(|e| f.ends_with(e)) && !f.contains("/node_modules/"))
            .cloned()
            .collect();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut layer: Vec<String> = self.scripts.iter().filter(|s| js.contains(*s)).cloned().collect();
        // The seed scripts' own nodes (the CI adapter has them too; the merge keeps its node).
        for s in &layer {
            x.nodes.push(ModelNode::new(format!("script:{s}"), NodeKind::Script, &[s.as_str()]));
        }
        let mut rounds = 0;
        while !layer.is_empty() && rounds < 64 {
            rounds += 1;
            let texts: BTreeMap<String, String> = layer
                .iter()
                .cloned()
                .zip(read.read_all(&layer))
                .filter_map(|(p, t)| t.map(|t| (p, t)))
                .collect();
            let set: BTreeSet<String> = layer.iter().cloned().collect();
            seen.extend(set.iter().cloned());
            let edges: BTreeMap<String, BTreeSet<String>> = set
                .iter()
                .map(|f| {
                    let to = relative_specifiers(texts.get(f).map(String::as_str).unwrap_or(""))
                        .iter()
                        .filter_map(|s| resolve(f, s, &js))
                        .collect();
                    (f.clone(), to)
                })
                .collect();
            let mut next = Vec::new();
            for (from, tos) in edges {
                for to in tos {
                    if !self.scripts.contains(&to) && !seen.contains(&to) {
                        x.nodes
                            .push(ModelNode::new(format!("script:{to}"), NodeKind::Script, &[to.as_str()]));
                    }
                    x.edges
                        .push(Edge::new(format!("script:{from}"), format!("script:{to}"), EdgeKind::Imports));
                    if !seen.contains(&to) && !next.contains(&to) {
                        next.push(to);
                    }
                }
            }
            layer = next;
        }
        // spec targets
        for f in files.iter().filter(|f| is_spec(f)) {
            let (target, home) = if f.starts_with("conformance/test/") {
                ("node-test:conformance".to_string(), "conformance".to_string())
            } else if let Some(pkg) = self.npm.node_of(f) {
                let kind = if f.contains("/e2e/") { "playwright" } else { "vitest" };
                (format!("{kind}:{pkg}"), pkg.to_string())
            } else {
                continue;
            };
            if !x.nodes.iter().any(|n| n.id == target) {
                x.nodes.push(ModelNode::new(&target, NodeKind::TestTarget, &[]).with_home(&home));
            }
            x.nodes
                .push(ModelNode::new(format!("test:{f}"), NodeKind::TestFile, &[f.as_str()]).with_home(&home));
            x.edges.push(Edge::new(&target, format!("test:{f}"), EdgeKind::Tests));
        }
        Ok(x)
    }

    fn spawn_budget(&self, _: &[String]) -> usize {
        1
    }
}

#[cfg(test)]
mod tests;
