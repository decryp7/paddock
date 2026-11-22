//! The repository model of ADR 0117: typed nodes, declared inputs and owners, and bulk queries over a change. This module is
//! the project level (WP-1): owners, named inputs ([`inputs`]), the attribute-aware `.gitattributes` input ([`attributes`]) and
//! [`reach`], which answers, for a whole changed-file set at once, every project the change reaches, how (`Reach`) and which
//! half (Rust, UI). CI, test-file and OS nodes join in later work packages through the same query.
//!
//! Pure: the caller supplies the graphs, the tracked files and the texts it needs (one bulk read, ER-20).

pub mod attributes;
pub mod glob;
pub mod inputs;

use crate::Graph;
use inputs::{Inputs, Owner, Target};
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
#[derive(Debug, Clone, Copy)]
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
}

/// How one project is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeReach {
    /// The strongest kind.
    pub reach: Reach,
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
}

impl Reached {
    fn mark(&mut self, id: &str, reach: Reach, rust: bool, ui: bool, why: String) {
        let n = self.nodes.entry(id.to_string()).or_insert(NodeReach {
            reach,
            rust: false,
            ui: false,
            why: Vec::new(),
        });
        n.reach = n.reach.max(reach);
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
        .filter(|(_, d)| {
            path == d
                || path
                    .strip_prefix(d.as_str())
                    .is_some_and(|r| r.starts_with('/'))
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

/// Prose: read by people, not by a build or a test unless an input or a `reads` edge says so (review round 2, P1-A).
pub fn is_prose(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.starts_with("LICENSE") || [".md", ".txt"].iter().any(|e| name.ends_with(e))
}

/// Images that only documentation shows: prose-like under a doc owner, code inside a project folder (an icon is built).
pub fn is_doc_asset(path: &str) -> bool {
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"]
        .iter()
        .any(|e| path.ends_with(e))
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
    for f in changed {
        let named = inputs.inputs_of(f);
        if !named.is_empty() {
            for i in named {
                let why = format!("input {} ({f})", i.name);
                if i.attributes {
                    let (before, after) = text(f);
                    for pattern in attributes::changed_patterns(before.as_deref(), after.as_deref())
                    {
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
                for t in &i.reaches {
                    match t {
                        Target::All => {
                            for (id, _) in facts.projects {
                                out.mark(id, i.reach, true, true, why.clone());
                            }
                        }
                        Target::Cargo => cargo_projects
                            .iter()
                            .for_each(|p| out.mark(p, i.reach, true, false, why.clone())),
                        Target::Npm => npm_projects
                            .iter()
                            .for_each(|p| out.mark(p, i.reach, false, true, why.clone())),
                        Target::Apps => {
                            for (id, _) in facts
                                .projects
                                .iter()
                                .filter(|(id, _)| id.starts_with("app:"))
                            {
                                out.mark(id, i.reach, true, true, why.clone());
                            }
                        }
                        Target::Project(p) => out.mark(p, i.reach, true, true, why.clone()),
                        Target::Owner => match project_of(facts, f) {
                            Some(p) => out.mark(p, i.reach, true, true, why.clone()),
                            None => out.unowned.push(f.clone()),
                        },
                    }
                }
            }
            continue;
        }
        // A project folder comes before the owner rules (§2): its prose is docs for the project, everything else is code.
        if let Some(p) = project_of(facts, f) {
            if is_prose(f) {
                out.mark(p, Reach::Docs, false, false, f.clone());
                continue;
            }
            let (rust, ui) = halves(facts, f);
            out.mark(p, Reach::Code, rust, ui, f.clone());
            if let Some(c) = facts.cargo.node_of(f) {
                crates.insert(c.to_string());
            }
            if let Some(n) = facts.npm.node_of(f) {
                packages.insert(n.to_string());
            }
            continue;
        }
        match inputs.owner_of(f) {
            Some(Owner::None) => out.not_built.push(f.clone()),
            Some(Owner::Doc) if is_prose(f) || is_doc_asset(f) => {
                out.mark("docs", Reach::Docs, false, false, f.clone())
            }
            Some(Owner::Project(p)) => out.mark(p, Reach::Code, true, true, f.clone()),
            // Data under a doc owner that no input names, or a path nothing claims.
            Some(Owner::Doc) | None => out.unowned.push(f.clone()),
        }
    }
    if !out.unowned.is_empty() {
        out.everything = true;
        let why = format!(
            "unowned {}: the plan answers everything (fail closed; declare an owner or an input)",
            out.unowned.join(", ")
        );
        for (id, _) in facts.projects {
            out.mark(id, Reach::Code, true, true, why.clone());
        }
    }
    for d in facts.cargo.dependents(&crates).difference(&crates) {
        if let Some(p) = project_of(facts, d) {
            out.mark(p, Reach::Code, true, false, format!("cargo {d}"));
        }
    }
    for d in facts.npm.dependents(&packages).difference(&packages) {
        if let Some(p) = project_of(facts, d) {
            out.mark(p, Reach::Code, false, true, format!("npm {d}"));
        }
    }
    out
}

#[cfg(test)]
mod tests;
