//! What only a project knows, declared in its contract (ADR 0117 §3): `graph.tests` (its test files per runner and its helper
//! globs) and `graph.smoke` (the phases and the nextest filterset a plumbing-only change runs), read from every `paddock.ci.json`:
//! the apps', the root projects' (`crates`, `ui`, `conformance`, `tools`, `icons`, `eslint`) and paddock-dev's (the contract in
//! its declared project folder, which holds only this block; `docs/repo-graph.toml` `[[project]]` and `[paths]` name the folders). `tests` and `smoke` globs are relative to the project folder;
//! `graph.reads` (what a file loads through a computed `import()`) names repository paths from the root.
//!
//! ```json
//! "graph": {
//!   "tests": { "vitest": ["app/src/**/*.spec.ts"], "playwright": ["app/e2e/**/*.spec.ts"], "helpers": ["app/e2e/**"] },
//!   "smoke": { "phases": ["prepare", "checks"], "nextest": "package(=dyno-core)" }
//! }
//! ```
//!
//! A project without `tests` has no test files (a change to one runs the project in full); without `smoke`, a plumbing reach runs
//! it in full (never less than declared, §5).

use super::code::TestDecl;
use crate::plan::RepoGraph;
use paddock_repo_graph::model::inputs::Inputs;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The project id of paddock-dev, whose contract holds only the `graph` block (no phases: the `paddock-dev` job runs its own steps).
pub const PADDOCK_DEV: &str = "paddock-dev";

/// The smoke set of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Smoke {
    /// The contract phases a smoke run keeps (the others are skipped, ER-33: reported as not run).
    pub phases: Vec<String>,
    /// The nextest filterset of a smoke run (empty: no Rust tests).
    pub nextest: String,
}

/// One project's declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectDecl {
    /// Its test files.
    pub tests: TestDecl,
    /// Its smoke set, when declared.
    pub smoke: Option<Smoke>,
    /// What a file loads at run time through a computed `import()` (`graph.reads`: a repository path to the globs, from the
    /// repository root, of the files it may load): those files become its edges instead of an unresolved reference (ADR 0117 §4,
    /// "declared runtime reads").
    pub reads: BTreeMap<String, Vec<String>>,
}

fn strings(v: &Value, what: &str) -> Result<Vec<String>, String> {
    v.as_array()
        .ok_or_else(|| format!("{what} is not an array of strings"))?
        .iter()
        .map(|s| {
            s.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{what} holds a value that is not a string"))
        })
        .collect()
}

/// The declarations of one contract (`v`) of project `id` in `dir`; `file` names it in errors.
pub fn parse(id: &str, dir: &str, file: &str, v: &Value) -> Result<ProjectDecl, String> {
    let mut d = ProjectDecl {
        tests: TestDecl {
            project: id.to_string(),
            dir: dir.to_string(),
            ..TestDecl::default()
        },
        smoke: None,
        reads: BTreeMap::new(),
    };
    let Some(g) = v.get("graph") else {
        return Ok(d);
    };
    let g = g.as_object().ok_or_else(|| format!("{file}: `graph` is not an object"))?;
    let rooted = |globs: Vec<String>| globs.into_iter().map(|p| format!("{dir}/{p}")).collect::<Vec<_>>();
    if let Some(t) = g.get("tests") {
        let t = t.as_object().ok_or_else(|| format!("{file}: `graph.tests` is not an object"))?;
        for (runner, globs) in t {
            let globs = rooted(strings(globs, &format!("{file}: `graph.tests.{runner}`"))?);
            if runner == "helpers" {
                d.tests.helpers = globs;
            } else {
                d.tests.runners.insert(runner.clone(), globs);
            }
        }
    }
    if let Some(s) = g.get("smoke") {
        let phases = match s.get("phases") {
            Some(p) => strings(p, &format!("{file}: `graph.smoke.phases`"))?,
            None => Vec::new(),
        };
        let nextest = s.get("nextest").and_then(Value::as_str).unwrap_or("").to_string();
        if phases.is_empty() && nextest.is_empty() {
            return Err(format!(
                "{file}: `graph.smoke` names no phase and no nextest filterset (a smoke set that runs nothing, ER-33)"
            ));
        }
        d.smoke = Some(Smoke { phases, nextest });
    }
    if let Some(r) = g.get("reads") {
        let r = r.as_object().ok_or_else(|| format!("{file}: `graph.reads` is not an object"))?;
        for (f, globs) in r {
            d.reads.insert(f.clone(), strings(globs, &format!("{file}: `graph.reads.{f}`"))?);
        }
    }
    if let Some(k) = g.keys().find(|k| !["tests", "smoke", "reads"].contains(&k.as_str())) {
        return Err(format!("{file}: unknown field `graph.{k}` (tests, smoke, reads)"));
    }
    Ok(d)
}

/// What a contract read gives: its text, `None` when the file is not there, or why it cannot be read.
pub type ReadText<'a> = &'a dyn Fn(&str) -> Result<Option<String>, String>;

/// Every project's declarations: project id to them, every contract read through `read` (the tree's revision, or the working tree
/// in files mode: integrator item 4c, a plan of another revision never takes today's contracts). A contract that is not there
/// declares nothing; one that cannot be read or parsed is an error (fail closed: the plan answers everything).
pub fn load(read: ReadText, graph: &RepoGraph, inputs: &Inputs) -> Result<BTreeMap<String, ProjectDecl>, String> {
    let paths = &inputs.paths;
    let mut out = BTreeMap::new();
    let mut one = |id: &str, dir: &str| -> Result<(), String> {
        let file = format!("{dir}/{}", paths.contract);
        if let Some(text) = read(&file)? {
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("{file} is not valid JSON: {e}"))?;
            out.insert(id.to_string(), parse(id, dir, &file, &v)?);
        }
        Ok(())
    };
    for app in &graph.apps {
        one(&format!("app:{app}"), &format!("{}{app}", paths.apps))?;
    }
    for p in crate::ci_run::contract::ROOT_PROJECTS {
        one(p, p)?;
    }
    if let Some(dir) = inputs.projects.iter().find(|d| d.id == PADDOCK_DEV).map(|d| d.folder.clone()) {
        one(PADDOCK_DEV, &dir)?;
    }
    Ok(out)
}

/// A [`ReadText`] over the working tree under `root`.
pub fn working_tree(root: &Path) -> impl Fn(&str) -> Result<Option<String>, String> + '_ {
    move |file: &str| match std::fs::read_to_string(root.join(file)) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{file} cannot be read: {e}")),
    }
}

/// The projects with a declared smoke set.
pub fn smoke_declared(decls: &BTreeMap<String, ProjectDecl>) -> BTreeSet<String> {
    decls.iter().filter(|(_, d)| d.smoke.is_some()).map(|(id, _)| id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_contract_declares_runners_helpers_and_a_smoke_set_relative_to_its_folder() {
        let v = json!({"graph": {"tests": {"vitest": ["app/src/**/*.spec.ts"], "helpers": ["app/e2e/**"]},
                                 "smoke": {"phases": ["checks"], "nextest": "package(=x-core)"}}});
        let d = parse("app:x", "apps/x", "apps/x/paddock.ci.json", &v).unwrap_or_default();
        assert_eq!(d.tests.runners["vitest"], ["apps/x/app/src/**/*.spec.ts"]);
        assert_eq!(d.tests.helpers, ["apps/x/app/e2e/**"]);
        assert_eq!(d.smoke.map(|s| s.nextest), Some("package(=x-core)".to_string()));
    }

    #[test]
    fn an_empty_smoke_set_or_an_unknown_field_is_refused_and_no_block_declares_nothing() {
        assert!(parse("a", "a", "f", &json!({"graph": {"smoke": {}}})).is_err());
        assert!(parse("a", "a", "f", &json!({"graph": {"smok": {}}})).is_err());
        let none = parse("a", "a", "f", &json!({})).unwrap_or_default();
        assert!(none.smoke.is_none() && none.tests.runners.is_empty());
    }
}
