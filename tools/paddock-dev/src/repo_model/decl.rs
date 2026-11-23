//! What only a project knows, declared in its contract (ADR 0117 §3): `graph.tests` (its test files per runner and its helper
//! globs) and `graph.smoke` (the phases and the nextest filterset a plumbing-only change runs), read from every `paddock.ci.json`:
//! the apps', the root projects' (`crates`, `ui`, `conformance`, `tools`, `icons`, `eslint`) and paddock-dev's
//! (`tools/paddock-dev/paddock.ci.json`, which holds only this block). Globs are relative to the project folder.
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
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The contract file of paddock-dev (no phases: the `paddock-dev` job runs its own steps).
pub const PADDOCK_DEV_CONTRACT: &str = "tools/paddock-dev/paddock.ci.json";

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
    for k in g.keys().filter(|k| !["tests", "smoke"].contains(&k.as_str())) {
        return Err(format!("{file}: unknown field `graph.{k}` (tests, smoke)"));
    }
    Ok(d)
}

/// Every project's declarations: project id to them.
pub fn load(root: &Path, graph: &RepoGraph) -> Result<BTreeMap<String, ProjectDecl>, String> {
    let mut out = BTreeMap::new();
    for (app, v) in &graph.configs {
        let dir = format!("apps/{app}");
        out.insert(
            format!("app:{app}"),
            parse(&format!("app:{app}"), &dir, &format!("{dir}/paddock.ci.json"), v)?,
        );
    }
    for p in crate::ci_run::contract::ROOT_PROJECTS {
        let file = crate::ci_run::contract::repo_path_of(p);
        if let Ok(v) = crate::ci_run::contract::load(root, p) {
            out.insert(p.to_string(), parse(p, p, &file, &v)?);
        }
    }
    if let Ok(text) = std::fs::read_to_string(root.join(PADDOCK_DEV_CONTRACT)) {
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("{PADDOCK_DEV_CONTRACT} is not valid JSON: {e}"))?;
        out.insert(
            "paddock-dev".to_string(),
            parse("paddock-dev", "tools/paddock-dev", PADDOCK_DEV_CONTRACT, &v)?,
        );
    }
    Ok(out)
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
