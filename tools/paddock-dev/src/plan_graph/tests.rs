//! The graph's selection mapped into the plan job's outputs (ADR 0117 §5, the wiring), with the controls of the brief: a plumbing
//! change plans the smoke set only, a test-only change only the affected targets and spec files, a foundation change no app, an
//! app code change that app in full, an unowned file everything.
#![allow(clippy::unwrap_used)]

use super::*;
use crate::plan_ci::{MatrixOpts, matrix};
use crate::repo_model::code::TestDecl;
use crate::repo_model::decl::{ProjectDecl, Smoke};
use paddock_repo_graph::Graph;
use paddock_repo_graph::model::nodes::Model;
use paddock_repo_graph::model::{NodeReach, Reach};

fn app_contract() -> Value {
    json!({"dir": "apps/x", "cargo": "app/src-tauri", "os": ["ubuntu-latest", "macos-latest"],
           "prepare": ["npm run x:prepare"], "checks": ["node scripts/x-check.mjs"], "rustChecks": ["cargo deny check"],
           "ui": ["cd apps/x/app", "npx ng test --watch=false --coverage"], "angularDir": "app",
           "e2e": ["cd apps/x/app", "npx playwright test --shard=$E2E_SHARD"], "e2eShards": 2,
           "e2eWebkit": ["cd apps/x/app", "npx playwright test --project=webkit"]})
}

fn graph() -> RepoGraph {
    let mut host = BTreeMap::new();
    host.insert(
        "apps/x".to_string(),
        crate::host_tests::Derived {
            packages: ["x-host".to_string()].into(),
            ..Default::default()
        },
    );
    RepoGraph {
        cargo: Graph::new(),
        npm: Graph::new(),
        apps: ["x".to_string(), "y".to_string()].into_iter().collect(),
        configs: [("x".to_string(), app_contract()), ("y".to_string(), app_contract())].into(),
        host,
        ..Default::default()
    }
}

fn world() -> World {
    let mut decls = BTreeMap::new();
    let mut tests = TestDecl {
        project: "app:x".into(),
        dir: "apps/x".into(),
        ..TestDecl::default()
    };
    tests.runners.insert("vitest".into(), vec!["apps/x/app/src/**/*.spec.ts".into()]);
    tests
        .runners
        .insert("playwright".into(), vec!["apps/x/app/e2e/**/*.spec.ts".into()]);
    decls.insert(
        "app:x".to_string(),
        ProjectDecl {
            tests,
            smoke: Some(Smoke {
                phases: vec!["prepare".into(), "checks".into()],
                nextest: "package(=x-core)".into(),
            }),
            reads: BTreeMap::new(),
        },
    );
    World {
        inputs: Default::default(),
        files: Vec::new(),
        model: Model::new(),
        smoke: ["app:x".to_string()].into(),
        decls,
        errors: Vec::new(),
    }
}

fn hit(reach: Reach, rust: bool, ui: bool) -> NodeReach {
    NodeReach {
        reach,
        kinds: [reach].into_iter().collect(),
        rust,
        ui,
        why: vec!["a reason".into()],
    }
}

fn rules() -> Value {
    json!({"docs": false})
}

fn legs_of(plan: &Value, g: &RepoGraph) -> Vec<Value> {
    let legs = matrix(
        plan,
        g,
        MatrixOpts {
            narrow: false,
            webkit: true,
            full_os: false,
        },
    )
    .unwrap();
    narrow_app_legs(legs, plan, g)
}

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn a_plumbing_change_plans_the_smoke_set_only() {
    let mut r = Reached::default();
    r.nodes.insert("app:x".into(), hit(Reach::Plumbing, true, true));
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root());
    assert_eq!(p["apps"], json!(["x"]));
    assert_eq!(p["graph"]["app:x"]["modes"], json!(["smoke"]));
    let legs = legs_of(&p, &g);
    let linux = legs.iter().find(|l| l["os"] == LINUX && l["e2e_leg"] == "").unwrap();
    assert_eq!(linux["filter"], "package(=x-core)");
    assert_eq!(
        (linux["checks"].as_str().unwrap().is_empty(), linux["ui_checks"].as_str().unwrap()),
        (false, "")
    );
    assert!(linux.get("tests_only").is_none(), "a smoke run keeps the gates");
    assert!(
        legs.iter().all(|l| l["e2e_leg"] == ""),
        "no e2e leg: the smoke set keeps no e2e phase"
    );
    assert!(
        legs.iter().any(|l| l["os"] == "macos-latest"),
        "a plumbing change runs on every OS the app declares"
    );
    assert_eq!(selection_output(&p)["app:x"]["phases"], json!(["checks", "prepare"]));
    assert_eq!(partial_projects(&p).0, BTreeSet::from(["app:x".to_string()]));
    // Control: without a declared smoke set the same reach runs the app in full.
    let mut w2 = world();
    w2.smoke.clear();
    let p2 = plan(&rules(), &r, &w2, &g, &root());
    assert!(p2["graph"].get("app:x").is_none() && p2["apps"] == json!(["x"]));
}

#[test]
fn a_test_only_change_passes_only_the_affected_targets_and_spec_files() {
    let mut r = Reached::default();
    r.nodes.insert("app:x".into(), hit(Reach::Test, true, true));
    r.targets.insert(
        "app:x".into(),
        ["nextest:x-core::it".to_string(), "vitest:apps/x/app".to_string()].into(),
    );
    r.test_files.insert(
        "app:x".into(),
        [
            "apps/x/app/src/a.spec.ts".to_string(),
            "apps/x/crates/core/tests/it/t.rs".to_string(),
        ]
        .into(),
    );
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root());
    let d = &p["graph"]["app:x"];
    assert_eq!(d["modes"], json!(["affected"]));
    assert_eq!(d["nextest"], "binary_id(=x-core::it)");
    assert_eq!(
        d["narrow"]["files"],
        json!({"vitest": ["apps/x/app/src/a.spec.ts"]}),
        "Rust files go by target"
    );
    assert_eq!(
        d["narrow"]["phases"],
        json!(["prepare", "ui"]),
        "the phase whose runner has the file, and the preparation"
    );
    let legs = legs_of(&p, &g);
    assert_eq!(
        legs.len(),
        1,
        "no e2e legs (no Playwright file), no macOS leg (x-core holds no host code): {legs:?}"
    );
    assert_eq!(
        (legs[0]["tests_only"].as_str(), legs[0]["filter"].as_str()),
        (Some("true"), Some("binary_id(=x-core::it)"))
    );
    assert_eq!(legs[0]["checks"], "", "the checks phase has no affected file");
    // Control: a reached target in a package with OS-specific code keeps the host leg.
    r.targets.get_mut("app:x").unwrap().insert("nextest:x-host".into());
    let p = plan(&rules(), &r, &w, &g, &root());
    assert!(legs_of(&p, &g).iter().any(|l| l["os"] == "macos-latest"));
    assert_eq!(partial_projects(&p).1, BTreeSet::from(["app:x".to_string()]));
}

#[test]
fn a_foundation_change_plans_no_app_and_an_app_change_plans_it_in_full() {
    let mut r = Reached::default();
    r.nodes.insert("paddock-dev".into(), hit(Reach::Code, true, false));
    r.nodes.insert("conformance".into(), hit(Reach::Code, true, true));
    r.nodes.insert("docs".into(), hit(Reach::Docs, false, false));
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root());
    assert_eq!(p["apps"], json!([]));
    assert_eq!(
        (p["paddockDev"].as_bool(), p["crates"].as_bool(), p["ui"].as_bool()),
        (Some(true), Some(false), Some(false))
    );
    assert!(legs_of(&p, &g).is_empty(), "no app leg");
    // An app's code: that app in full, its halves as reached; the other app does not run.
    r.nodes.insert("app:x".into(), hit(Reach::Code, true, false));
    let p = plan(&rules(), &r, &w, &g, &root());
    assert_eq!((p["apps"].clone(), p["kinds"]["x"].clone()), (json!(["x"]), json!(["rust"])));
    assert!(p["graph"].get("app:x").is_none(), "full: not narrowed");
    let legs = legs_of(&p, &g);
    assert!(legs.iter().all(|l| l["filter"] == "" && l.get("tests_only").is_none()));
    assert!(selection_output(&p).as_object().unwrap().is_empty() && partial_projects(&p) == Default::default());
}

#[test]
fn an_unowned_file_plans_everything_in_full() {
    let mut r = Reached::default();
    r.everything = true;
    r.unowned = vec!["Mystery".into()];
    for id in ["app:x", "app:y", "crates", "ui", "tools", "paddock-dev"] {
        r.nodes.insert(id.into(), NodeReach::full("fail closed"));
    }
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root());
    assert_eq!(p["all"], true);
    assert_eq!(p["apps"], json!(["x", "y"]));
    assert!(p["graph"].as_object().unwrap().is_empty(), "nothing narrowed");
    assert_eq!(p["kinds"]["x"], json!(["rust", "ui"]));
}

#[test]
fn the_workspace_legs_run_the_selected_tests_and_a_selection_without_rust_tests_keeps_one_leg() {
    let legs: Vec<Value> = ["ubuntu-latest", "windows-latest", "macos-latest"]
        .iter()
        .map(|os| json!({"os": os}))
        .collect();
    let g = graph();
    let p = json!({"graph": {"crates": {"nextest": "package(=paddock-proc)", "testsOnly": false, "targets": []}}});
    let out = narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates");
    assert_eq!(out.len(), 3, "a smoke set runs on every OS");
    assert!(out.iter().all(|l| l["filter"] == "package(=paddock-proc)"));
    let p = json!({"graph": {"crates": {"nextest": "binary_id(=paddock-proc)", "testsOnly": true, "targets": ["nextest:paddock-proc"]}}});
    assert_eq!(
        narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates").len(),
        1,
        "no host code: Linux only"
    );
    let p = json!({"graph": {"crates": {"nextest": "", "testsOnly": false, "targets": []}}});
    let out = narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates");
    assert_eq!((out.len(), out[0]["filter"].as_str()), (1, Some("none()")));
    // Control: a project in full keeps its legs as they are.
    assert_eq!(
        narrow_workspace_legs(legs.clone(), &json!({"graph": {}}), &g, "crates", "crates"),
        legs
    );
}

#[test]
fn the_escape_record_names_what_the_graph_runs_not_the_rule_tables() {
    // WP-9 records what CI ran: since the wiring, the graph's selection.
    let mut r = Reached::default();
    r.nodes.insert("app:x".into(), hit(Reach::Test, true, true));
    r.targets.insert("app:x".into(), ["nextest:x-core::it".to_string()].into());
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root());
    let legs = legs_of(&p, &g);
    let sel = escape_selected(&p, &legs, &["ubuntu-latest".to_string()]);
    let x = &sel["app:x"];
    assert_eq!(
        (x.runs.as_str(), x.targets.clone()),
        ("tests", Some(vec!["nextest:x-core::it".to_string()]))
    );
    assert_eq!(x.os, vec![LINUX.to_string()]);
    assert!(
        x.covers(LINUX, "nextest:x-core::it") && !x.covers(LINUX, "nextest:x-core"),
        "only the affected target"
    );
    assert_eq!(sel["conformance"].runs, "all");
    // Control: a full selection records `all` with no target list.
    r.nodes.insert("app:x".into(), hit(Reach::Code, true, true));
    let p = plan(&rules(), &r, &w, &g, &root());
    let sel = escape_selected(&p, &legs_of(&p, &g), &[]);
    assert_eq!((sel["app:x"].runs.as_str(), sel["app:x"].targets.is_none()), ("all", true));
}
