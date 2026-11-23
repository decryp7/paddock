//! The repository graph decides the CI plan (ADR 0117 §5 and §8, the wiring): `./dev plan --ci` asks the graph's `selection` and
//! maps it into the outputs the jobs already read, so the jobs change by one passed-through value each.
//!
//! | Selection of a project | What the plan job writes |
//! |---|---|
//! | `full` | the project's flag (an app: its legs, both halves the change reaches), exactly as the rule tables' plan did |
//! | `smoke` | the flag; the declared smoke phases in `selection` (`./dev ci run` skips the others); the smoke filterset as the leg `filter` (apps: member semantics, the gates run) or the `crate`/`paddock-dev` leg `filter` |
//! | `affected` | the flag; a nextest filterset of the reached targets (`binary_id(=..)`) through the graph's own channel (`graph_filter`, `test_filter`: no test found fails), lint and format phases kept; the affected spec files per runner in `selection` (Vitest `--include`, Playwright and `node --test` file arguments); only the phases whose test runners have an affected file |
//! | `phases` | the flag; those contract phases in `selection` |
//! | `installers` | `--app <app>` answers `true` (rule 54); the app's smoke set or suite runs beside it as above |
//! | per OS | an app's or the crates' host legs run for `full` and `smoke`; for `affected` only when a reached target's package holds OS-specific code (the derived placement, `select_os`) |
//! | per package (G11) | a `full` project keeps the existing package filters (`package_filter`, a member run's reached packages) |
//!
//! A narrowed project is named in the `selection` output and in the run's declared scope (`ran.smoke`, `ran.affected`, ADR 0113
//! v2), so a smoke or affected run is never counted complete; `partial=true` tells the old report-only line the same.
//!
//! The switch is data: `docs/repo-graph.toml` `[planner]` ([`crate::repo_inputs::planner`]); `rules` plans from the rule tables
//! exactly as before (no added output).

use crate::ci_run::select::{Narrow, narrow_lines};
use crate::plan::RepoGraph;
use crate::plan_ci::LINUX;
use crate::repo_model::World;
use paddock_repo_graph::model::glob;
use paddock_repo_graph::model::{Reached, Selection, selection};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The outputs the plan job writes only under the graph planner: the narrowed projects and whether the run is partial.
pub const OUTPUTS: [&str; 2] = ["selection", "partial"];

/// The plan's project keys and the project ids they stand for.
pub const PROJECT_KEYS: [(&str, &str); 9] = [
    ("crates", "crates"),
    ("ui", "ui"),
    ("eslint", "eslint"),
    ("conformance", "conformance"),
    ("c99", "c99"),
    ("tools", "tools"),
    ("icons", "icons"),
    ("ciImage", "ci-image"),
    ("paddockDev", "paddock-dev"),
];

/// The contract fields of an app leg each phase fills (the leg key it blanks when the phase is not kept).
const APP_PHASES: [(&str, &str); 5] = [
    ("prepare", "prepare"),
    ("checks", "checks"),
    ("rustChecks", "rust_checks"),
    ("hostRustChecks", "host_rust_checks"),
    ("ui", "ui_checks"),
];

/// The phases of a contract: its fields that hold a list of command lines.
fn phases_of(contract: &Value) -> Vec<(String, Vec<String>)> {
    contract
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| {
            let lines: Vec<String> = v
                .as_array()?
                .iter()
                .map(|l| l.as_str().map(str::to_string))
                .collect::<Option<_>>()?;
            (!lines.is_empty()).then(|| (k.clone(), lines))
        })
        .collect()
}

/// The nextest filterset of reached test targets (`nextest:<binary id>`); empty when none is a nextest target.
pub fn targets_filter(targets: &BTreeSet<String>) -> String {
    let parts: Vec<String> = targets
        .iter()
        .filter_map(|t| t.strip_prefix("nextest:"))
        .map(|b| format!("binary_id(={b})"))
        .collect();
    parts.join(" | ")
}

/// Two filtersets joined by `|` (an empty one adds nothing).
fn either(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("({a}) | ({b})"),
    }
}

/// The affected test files of a project by runner (its contract's `graph.tests` globs); Rust files are selected by target, not file.
fn files_by_runner(reached: &Reached, world: &World, id: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let Some(decl) = world.decls.get(id) else { return out };
    for f in reached.test_files.get(id).into_iter().flatten() {
        if let Some((runner, _)) = decl
            .tests
            .runners
            .iter()
            .find(|(_, globs)| globs.iter().any(|g| glob::matches(g, f)))
        {
            out.entry(runner.clone()).or_default().push(f.clone());
        }
    }
    out
}

/// What one narrowed project runs (`plan["graph"][id]`): the [`Narrow`] `./dev ci run` applies, the nextest filterset, whether only
/// tests run (no gates), and the modes the scope line names. `None`: it runs in full (an affected file no test runner line of its
/// contract can be given).
fn narrowed(id: &str, s: &Selection, reached: &Reached, world: &World, contract: &Value, lint: &[String]) -> Option<Value> {
    let smoke = world.decls.get(id).and_then(|d| d.smoke.clone()).filter(|_| s.smoke);
    let mut phases: BTreeSet<String> = s.phases.clone();
    if let Some(sm) = &smoke {
        phases.extend(sm.phases.iter().cloned());
    }
    let files = if s.affected.is_empty() {
        BTreeMap::new()
    } else {
        files_by_runner(reached, world, id)
    };
    // An affected run keeps the phases whose test runners have an affected file (the same narrowing `./dev ci run` applies), and
    // the app's preparation with them.
    if !files.is_empty() {
        let mut placed: BTreeSet<&str> = BTreeSet::new();
        for (phase, lines) in phases_of(contract) {
            let Ok(lines) = crate::ci_run::expand(&lines) else { continue };
            for (runner, list) in &files {
                let one = BTreeMap::from([(runner.clone(), list.clone())]);
                if narrow_lines(&lines, &one).1 > 0 {
                    phases.insert(phase.clone());
                    placed.insert(runner.as_str());
                }
            }
        }
        // Never less than declared (§5): a runner whose files no line of the contract can be given runs the project whole.
        if files.keys().any(|r| !placed.contains(r.as_str())) {
            return None;
        }
        if contract.get("prepare").is_some() {
            phases.insert("prepare".into());
        }
    }
    // A test-only change keeps the project's lint and format phases (`[planner] lint_phases`, review round 2 P2-1): only the tests
    // narrow.
    if !s.affected.is_empty() {
        let declared: BTreeSet<String> = phases_of(contract).into_iter().map(|(p, _)| p).collect();
        phases.extend(lint.iter().filter(|p| declared.contains(*p)).cloned());
    }
    let nextest = either(
        smoke.as_ref().map(|s| s.nextest.as_str()).unwrap_or(""),
        &targets_filter(&s.affected),
    );
    let mut modes = Vec::new();
    if s.smoke {
        modes.push("smoke");
    }
    if !s.affected.is_empty() {
        modes.push("affected");
    }
    if !s.phases.is_empty() {
        modes.push("phases");
    }
    if s.installers {
        modes.push("installers");
    }
    let narrow = Narrow {
        phases: Some(phases),
        files,
    };
    Some(json!({
        "modes": modes,
        "narrow": narrow.json(),
        "nextest": nextest,
        "affectedOnly": !s.smoke && s.phases.is_empty() && !s.affected.is_empty(),
        "targets": s.affected,
    }))
}

/// The graph's plan in the rule tables' JSON shape (`plan.rs` `affected`): the project flags, the apps and their halves, the reached
/// packages and the reasons, plus `graph` (each narrowed project's [`narrowed`] record) and `planner: "graph"`. `rules` gives the
/// facts the graph does not decide (`docs`); `root` holds the root projects' contracts.
pub fn plan(rules: &Value, reached: &Reached, world: &World, graph: &RepoGraph, root: &Path, lint: &[String]) -> Value {
    let sel = selection(reached, &world.smoke);
    let runs = |id: &str| sel.get(id).is_some_and(|s| !s.skip());
    let mut out = Map::new();
    out.insert("all".into(), json!(reached.everything));
    for (key, id) in PROJECT_KEYS {
        // The conformance job is in every plan, whatever changed (user decision 2026-11-12).
        out.insert(key.into(), json!(runs(id) || id == "conformance"));
    }
    out.insert("docs".into(), rules["docs"].clone());
    let apps: Vec<&String> = graph.apps.iter().filter(|a| runs(&format!("app:{a}"))).collect();
    out.insert("apps".into(), json!(apps));
    let mut kinds = Map::new();
    let mut detail = Map::new();
    for a in &apps {
        let id = format!("app:{a}");
        let s = &sel[&id];
        let n = reached.nodes.get(&id);
        let d = if s.full {
            None
        } else {
            narrowed(&id, s, reached, world, graph.config(a), lint)
        };
        let (rust, ui) = match d {
            None => match n {
                Some(n) if !reached.everything && (n.rust || n.ui) => (n.rust, n.ui),
                _ => (true, true),
            },
            Some(d) => {
                let kept = |p: &str| d["narrow"]["phases"].as_array().is_some_and(|l| l.iter().any(|x| x == p));
                let rust = !d["nextest"].as_str().unwrap_or("").is_empty() || kept("rustChecks") || kept("hostRustChecks");
                let ui = kept("ui") || kept("e2e") || kept("e2eWebkit");
                detail.insert(id.clone(), d);
                (rust, ui)
            }
        };
        let mut k = Vec::new();
        if rust {
            k.push("rust");
        }
        if ui {
            k.push("ui");
        }
        kinds.insert(a.to_string(), json!(k));
    }
    for (_, id) in PROJECT_KEYS.iter().filter(|(_, id)| *id != "conformance") {
        if let Some(s) = sel.get(*id).filter(|s| !s.skip() && !s.full) {
            let contract = crate::ci_run::contract::load(root, id).unwrap_or(Value::Null);
            if let Some(d) = narrowed(id, s, reached, world, &contract, lint) {
                detail.insert(id.to_string(), d);
            }
        }
    }
    out.insert("kinds".into(), Value::Object(kinds));
    // G11: the reached packages of each workspace (a member run narrows a full project to them through `package_filter`).
    let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in &reached.crates {
        if let Some(n) = graph.cargo.get(c) {
            packages.entry(n.workspace.clone()).or_default().push(n.name.clone());
        }
    }
    packages.values_mut().for_each(|v| {
        v.sort();
        v.dedup();
    });
    out.insert("packages".into(), json!(packages));
    // Only a project reached through its crates alone may run a subset of its packages.
    let cargo_only: Vec<&String> = sel
        .iter()
        .filter(|(id, s)| s.full && !reached.whole.contains(*id) && !reached.everything)
        .map(|(id, _)| id)
        .collect();
    out.insert("cargoOnly".into(), json!(cargo_only));
    let why: Map<String, Value> = reached.nodes.iter().map(|(id, n)| (id.clone(), json!(n.why))).collect();
    out.insert("why".into(), Value::Object(why));
    out.insert("graph".into(), Value::Object(detail));
    out.insert("planner".into(), json!("graph"));
    Value::Object(out)
}

/// The narrowed record of project `id` in a graph plan.
fn detail<'a>(plan: &'a Value, id: &str) -> Option<&'a Value> {
    plan["graph"].get(id)
}

/// The packages of the reached nextest targets of a narrowed project that hold OS-specific code (the derived placement of
/// `workspace`, `select_os`): its host legs run for them (review round 2 P2-3).
fn host_packages(d: &Value, graph: &RepoGraph, workspace: &str) -> BTreeSet<String> {
    let derived = graph.derived(workspace);
    d["targets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|t| t.strip_prefix("nextest:"))
        .map(|b| b.split("::").next().unwrap_or(b).to_string())
        .filter(|p| derived.packages.contains(p))
        .collect()
}

/// The app legs of `matrix` narrowed by the graph's selection: phases blanked, the graph's filterset on the Rust step through its own
/// channel (`graph_filter`: no test found fails, review round 2 P2-2) with the gates kept (fmt, clippy: P2-1), e2e legs dropped when
/// no e2e phase is kept, a host leg for affected tests only when a reached target holds OS-specific code, and a leg left with
/// nothing to run dropped.
pub fn narrow_app_legs(legs: Vec<Value>, plan: &Value, graph: &RepoGraph) -> Vec<Value> {
    legs.into_iter()
        .filter_map(|mut leg| {
            let app = leg["app"].as_str().unwrap_or("").to_string();
            let Some(d) = detail(plan, &format!("app:{app}")) else {
                return Some(leg);
            };
            let kept = |p: &str| d["narrow"]["phases"].as_array().is_some_and(|l| l.iter().any(|x| x == p));
            let e2e = leg["e2e_leg"].as_str().unwrap_or("").to_string();
            if e2e == "e2e-webkit" && !kept("e2eWebkit") || e2e.starts_with("e2e-") && e2e != "e2e-webkit" && !kept("e2e") {
                return None;
            }
            let affected_only = d["affectedOnly"].as_bool() == Some(true);
            // The app's folder is its leg's `dir` (from its contract), the key of its derived placement.
            let dir = leg["dir"].as_str().unwrap_or("").to_string();
            if leg["os"] != LINUX && affected_only && host_packages(d, graph, &dir).is_empty() {
                return None;
            }
            for (phase, key) in APP_PHASES {
                if !kept(phase) {
                    leg[key] = json!("");
                }
            }
            if !kept("ui") {
                leg["ui"] = json!(false);
            }
            let nextest = d["nextest"].as_str().unwrap_or("");
            if e2e.is_empty() {
                leg["rust"] = json!(!nextest.is_empty());
                leg["filter"] = json!(nextest);
                leg["graph_filter"] = json!(if nextest.is_empty() { "" } else { "true" });
                let idle = ["prepare", "checks", "rust_checks", "host_rust_checks", "ui_checks"]
                    .iter()
                    .all(|k| leg[*k].as_str().unwrap_or("").is_empty());
                if idle && nextest.is_empty() {
                    return None;
                }
            }
            Some(leg)
        })
        .collect()
}

/// The `crate` or `paddock-dev` legs narrowed by the graph's selection of `id` (`crates`, `paddock-dev`): each leg's `test_filter` (the
/// member channel: no test found fails, review round 2 P2-2) also holds the graph's filterset, and the gates (clippy) still run (P2-1).
/// A host leg runs only for affected targets in packages with OS-specific code (`crates`, its packages narrowed to them; paddock-dev's
/// placement is by module, so its narrowed runs stay on Linux). A selection without Rust tests keeps the Linux leg for its gates and
/// skips its tests (`test_skip`), never a filter that selects nothing.
pub fn narrow_workspace_legs(legs: Vec<Value>, plan: &Value, graph: &RepoGraph, id: &str, workspace: &str) -> Vec<Value> {
    let Some(d) = detail(plan, id) else { return legs };
    let nextest = d["nextest"].as_str().unwrap_or("").to_string();
    let hosts = if id == "crates" && d["affectedOnly"].as_bool() == Some(true) {
        host_packages(d, graph, workspace)
    } else {
        BTreeSet::new()
    };
    legs.into_iter()
        .filter(|l| l["os"] == LINUX || !hosts.is_empty())
        .map(|mut l| {
            if nextest.is_empty() {
                l["test_skip"] = json!("true");
                return l;
            }
            let before = l["test_filter"].as_str().unwrap_or("").to_string();
            l["test_filter"] = json!(crate::host_tests::and_filters(&before, &nextest));
            if l["os"] != LINUX {
                l["test_packages"] = json!(hosts.iter().cloned().collect::<Vec<_>>().join(" "));
            }
            l
        })
        .collect()
}

/// The `selection` output: each narrowed project's [`Narrow`] (a project in full is not named).
pub fn selection_output(plan: &Value) -> Value {
    Value::Object(
        plan["graph"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(id, d)| (id.clone(), d["narrow"].clone()))
            .collect(),
    )
}

/// The projects a plan ran less than in full, by mode, for the declared scope: `(smoke, affected)`. A smoke or phases selection
/// is `smoke`; affected tests `affected`.
pub fn partial_projects(plan: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut smoke, mut affected) = (BTreeSet::new(), BTreeSet::new());
    for (id, d) in plan["graph"].as_object().into_iter().flatten() {
        let modes: Vec<&str> = d["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if modes.iter().any(|m| *m == "smoke" || *m == "phases") {
            smoke.insert(id.clone());
        }
        if modes.contains(&"affected") {
            affected.insert(id.clone());
        }
    }
    (smoke, affected)
}

/// The outputs of an app with its own workflow (`debrief.yml`) for an affected selection: the jobs that run (`rerun_jobs`: its Rust
/// lint always, P2-1; its Rust tests with the graph's filterset, and its macOS tests when a reached target holds OS-specific code,
/// P2-3; the frontend and e2e jobs whole when a spec of theirs changed, lint included) and the Rust filtersets. `None`: the app runs
/// in full (not narrowed, or a selection this mapping cannot carry: a smoke set or phases).
pub fn app_workflow_outputs(plan: &Value, app: &str, graph: &RepoGraph, dir: &str) -> Option<Vec<(String, String)>> {
    let d = detail(plan, &format!("app:{app}"))?;
    if d["affectedOnly"].as_bool() != Some(true) {
        return None;
    }
    let mut jobs = vec!["rust-lint"];
    let mut out = Vec::new();
    let nextest = d["nextest"].as_str().unwrap_or("");
    if !nextest.is_empty() {
        jobs.push("rust-test");
        out.push(("rust_test_filter".to_string(), nextest.to_string()));
        if !host_packages(d, graph, dir).is_empty() {
            jobs.push("rust-test-macos");
            out.push(("rust_test_macos_filter".to_string(), nextest.to_string()));
        }
    }
    let files = &d["narrow"]["files"];
    let has = |runner: &str| files[runner].as_array().is_some_and(|a| !a.is_empty());
    if has("vitest") {
        jobs.push("frontend");
    }
    if has("playwright") {
        jobs.push("e2e");
    }
    // A runner this workflow has no job for (a node test): the app runs in full, never less (§5).
    if files
        .as_object()
        .is_some_and(|f| f.keys().any(|r| r != "vitest" && r != "playwright"))
    {
        return None;
    }
    out.push(("rerun_jobs".to_string(), json!(jobs).to_string()));
    Some(out)
}

/// What the escape monitor records as selected (ADR 0117 WP-9): what CI runs after the wiring, project by project, with the OS legs
/// that run it and the label of the graph's selection (`all`, `rust`, `ui`, `smoke`, `tests`, `phases`) and, for affected tests,
/// the exact targets.
pub fn escape_selected(plan: &Value, legs: &[Value], os_legs: &[String]) -> BTreeMap<String, crate::escape::Sel> {
    let label = |id: &str, full: &str| -> (String, Option<Vec<String>>) {
        match detail(plan, id) {
            None => (full.to_string(), None),
            Some(d) => {
                let modes: Vec<&str> = d["modes"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let targets: Vec<String> = d["targets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                let l: Vec<&str> = modes.iter().map(|m| if *m == "affected" { "tests" } else { m }).collect();
                (l.join("+"), (!targets.is_empty()).then_some(targets))
            }
        }
    };
    let mut out = BTreeMap::new();
    for (key, id) in PROJECT_KEYS {
        if plan[key].as_bool() != Some(true) {
            continue;
        }
        let os = if id == "crates" || id == "paddock-dev" {
            os_legs.to_vec()
        } else {
            vec![LINUX.to_string()]
        };
        let (runs, targets) = label(id, "all");
        out.insert(id.to_string(), crate::escape::Sel { os, runs, targets });
    }
    for a in plan["apps"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let os: BTreeSet<String> = legs
            .iter()
            .filter(|l| l["app"] == a && l["e2e_leg"].as_str().unwrap_or("").is_empty())
            .filter_map(|l| l["os"].as_str().map(str::to_string))
            .collect();
        let k: Vec<&str> = plan["kinds"][a]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let full = match (k.contains(&"rust"), k.contains(&"ui")) {
            (true, false) => "rust",
            (false, true) => "ui",
            _ => "all",
        };
        let (runs, targets) = label(&format!("app:{a}"), full);
        let os = if os.is_empty() {
            vec![LINUX.to_string()]
        } else {
            os.into_iter().collect()
        };
        out.insert(format!("app:{a}"), crate::escape::Sel { os, runs, targets });
    }
    out
}

#[cfg(test)]
mod tests;
