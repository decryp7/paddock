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

fn lint() -> Vec<String> {
    vec!["fmt".into(), "lint".into()]
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
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
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
    assert_eq!(linux["graph_filter"], "true", "the graph's filterset fails on no test (P2-2)");
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
    let p2 = plan(&rules(), &r, &w2, &g, &root(), &lint());
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
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
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
    // The gates (fmt, clippy) still run: no rerun semantics (review round 2 P2-1); the filterset goes through its own channel (P2-2).
    assert!(legs[0].get("tests_only").is_none(), "{:?}", legs[0]);
    assert_eq!(
        (
            legs[0]["graph_filter"].as_str(),
            legs[0]["filter"].as_str(),
            legs[0]["rust"].as_bool()
        ),
        (Some("true"), Some("binary_id(=x-core::it)"), Some(true))
    );
    assert_eq!(legs[0]["checks"], "", "the checks phase has no affected file");
    // Control: a reached target in a package with OS-specific code keeps the host leg.
    r.targets.get_mut("app:x").unwrap().insert("nextest:x-host".into());
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
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
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
    assert_eq!(p["apps"], json!([]));
    assert_eq!(
        (p["paddockDev"].as_bool(), p["crates"].as_bool(), p["ui"].as_bool()),
        (Some(true), Some(false), Some(false))
    );
    assert!(legs_of(&p, &g).is_empty(), "no app leg");
    // An app's code: that app in full, its halves as reached; the other app does not run.
    r.nodes.insert("app:x".into(), hit(Reach::Code, true, false));
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
    assert_eq!((p["apps"].clone(), p["kinds"]["x"].clone()), (json!(["x"]), json!(["rust"])));
    assert!(p["graph"].get("app:x").is_none(), "full: not narrowed");
    let legs = legs_of(&p, &g);
    assert!(legs.iter().all(|l| l["filter"] == "" && l.get("tests_only").is_none()));
    assert!(selection_output(&p).as_object().unwrap().is_empty() && partial_projects(&p) == Default::default());
}

#[test]
fn an_unowned_file_plans_everything_in_full() {
    let mut r = Reached {
        everything: true,
        unowned: vec!["Mystery".into()],
        ..Reached::default()
    };
    for id in ["app:x", "app:y", "crates", "ui", "tools", "paddock-dev"] {
        r.nodes.insert(id.into(), NodeReach::full("fail closed"));
    }
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
    assert_eq!(p["all"], true);
    assert_eq!(p["apps"], json!(["x", "y"]));
    assert!(p["graph"].as_object().unwrap().is_empty(), "nothing narrowed");
    assert_eq!(p["kinds"]["x"], json!(["rust", "ui"]));
}

#[test]
fn the_workspace_legs_run_the_selected_tests_and_a_selection_without_rust_tests_keeps_one_leg() {
    let legs: Vec<Value> = ["ubuntu-latest", "windows-latest", "macos-latest"]
        .iter()
        .map(|os| json!({"os": os, "test_packages": "", "test_filter": "", "test_skip": ""}))
        .collect();
    let mut g = graph();
    g.host.insert(
        "crates".into(),
        crate::host_tests::Derived {
            packages: ["paddock-proc".to_string()].into(),
            ..Default::default()
        },
    );
    // A smoke set: the Linux leg, its filterset in `test_filter` (the member channel: no test found fails, P2-2).
    let p = json!({"graph": {"crates": {"nextest": "package(=paddock-proc)", "affectedOnly": false, "targets": []}}});
    let out = narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates");
    assert_eq!(out.len(), 1);
    assert_eq!(
        (out[0]["test_filter"].as_str(), out[0].get("filter")),
        (Some("package(=paddock-proc)"), None)
    );
    // Affected targets: a host leg only for a package with OS-specific code, testing that package.
    let p =
        json!({"graph": {"crates": {"nextest": "binary_id(=paddock-proc)", "affectedOnly": true, "targets": ["nextest:paddock-proc"]}}});
    let out = narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates");
    assert_eq!(out.len(), 3, "paddock-proc holds OS code: every OS");
    assert_eq!(out[2]["test_packages"], "paddock-proc");
    let p = json!({"graph": {"crates": {"nextest": "binary_id(=paddock-a)", "affectedOnly": true, "targets": ["nextest:paddock-a"]}}});
    assert_eq!(
        narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates").len(),
        1,
        "no host code: Linux only"
    );
    // No Rust test in the selection: the Linux leg keeps its gates and skips its tests, never a filter that selects none.
    let p = json!({"graph": {"crates": {"nextest": "", "affectedOnly": false, "targets": []}}});
    let out = narrow_workspace_legs(legs.clone(), &p, &g, "crates", "crates");
    assert_eq!(
        (out.len(), out[0]["test_skip"].as_str(), out[0]["test_filter"].as_str()),
        (1, Some("true"), Some(""))
    );
    // Control: a project in full keeps its legs as they are.
    assert_eq!(
        narrow_workspace_legs(legs.clone(), &json!({"graph": {}}), &g, "crates", "crates"),
        legs
    );
}

#[test]
fn a_test_only_change_keeps_the_lint_and_format_phases() {
    // Review round 2, P2-1: the reached project's lint and format phases run beside its affected tests.
    let (g, mut w) = (graph(), world());
    w.decls.insert("crates".into(), Default::default());
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Test, true, false));
    r.targets.insert("crates".into(), ["nextest:paddock-proc".to_string()].into());
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
    assert_eq!(
        p["graph"]["crates"]["narrow"]["phases"],
        json!(["fmt"]),
        "the crates contract's lint phase: {}",
        p["graph"]
    );
    // Control: without declared lint phases nothing of them is kept.
    let p = plan(&rules(), &r, &w, &g, &root(), &[]);
    assert_eq!(p["graph"]["crates"]["narrow"]["phases"], json!([]));
}

#[test]
fn an_app_workflows_affected_run_carries_its_lint_its_tests_and_its_host_tests() {
    // Review round 2, P2-1 and P2-3: debrief.yml runs its Rust lint, its Rust tests with the graph's filterset, and its macOS tests
    // when a reached target holds OS-specific code; a spec runner it has no job for runs it in full.
    let g = graph();
    let p = json!({"graph": {"app:x": {"affectedOnly": true, "nextest": "binary_id(=x-host)", "targets": ["nextest:x-host"],
                                       "narrow": {"files": {"vitest": ["apps/x/app/src/a.spec.ts"]}}}}});
    let out: BTreeMap<String, String> = app_workflow_outputs(&p, "x", &g, "apps/x").unwrap().into_iter().collect();
    assert_eq!(out["rerun_jobs"], r#"["rust-lint","rust-test","rust-test-macos","frontend"]"#);
    assert_eq!(
        (out["rust_test_filter"].as_str(), out["rust_test_macos_filter"].as_str()),
        ("binary_id(=x-host)", "binary_id(=x-host)")
    );
    assert!(
        !out.contains_key("frontend_include"),
        "the frontend job runs whole, its lint with it"
    );
    // No host code: no macOS tests.
    let p2 =
        json!({"graph": {"app:x": {"affectedOnly": true, "nextest": "binary_id(=x-core)", "targets": ["nextest:x-core"], "narrow": {}}}});
    let out: BTreeMap<String, String> = app_workflow_outputs(&p2, "x", &g, "apps/x").unwrap().into_iter().collect();
    assert_eq!(out["rerun_jobs"], r#"["rust-lint","rust-test"]"#);
    // A node test: in full (never less than declared); a smoke selection: in full.
    let p3 = json!({"graph": {"app:x": {"affectedOnly": true, "nextest": "", "targets": [], "narrow": {"files": {"node-test": ["apps/x/s.test.mjs"]}}}}});
    assert_eq!(app_workflow_outputs(&p3, "x", &g, "apps/x"), None);
    let p4 = json!({"graph": {"app:x": {"affectedOnly": false, "nextest": "package(=x-core)", "narrow": {}}}});
    assert_eq!(app_workflow_outputs(&p4, "x", &g, "apps/x"), None);
}

#[test]
fn the_escape_record_names_what_the_graph_runs_not_the_rule_tables() {
    // WP-9 records what CI ran: since the wiring, the graph's selection.
    let mut r = Reached::default();
    r.nodes.insert("app:x".into(), hit(Reach::Test, true, true));
    r.targets.insert("app:x".into(), ["nextest:x-core::it".to_string()].into());
    let (g, w) = (graph(), world());
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
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
    let p = plan(&rules(), &r, &w, &g, &root(), &lint());
    let sel = escape_selected(&p, &legs_of(&p, &g), &[]);
    assert_eq!((sel["app:x"].runs.as_str(), sel["app:x"].targets.is_none()), ("all", true));
}

/// The declared scope of a landing (or push) whose need is its own diff, through the planner's own call
/// ([`declare_selected_legs`], as `plan_cmd` makes it) over the OS of the legs [`narrow_workspace_legs`] kept.
fn landing_declared(plan: &Value, ran_oses: &[&str]) -> crate::run_scope::Declared {
    use crate::run_scope::{Scope, ci_scope};
    let legs: Vec<Value> = ["ubuntu-latest", "windows-latest", "macos-latest"]
        .iter()
        .map(|os| json!({"os": os, "test_packages": "", "test_filter": "", "test_skip": ""}))
        .collect();
    let kept = narrow_workspace_legs(legs, plan, &graph(), "crates", "crates");
    let oses: BTreeMap<&str, BTreeSet<String>> = [("crate", legs_oses(&kept))].into();
    let all: BTreeSet<String> = ["crates", "crate@ubuntu-latest", "crate@windows-latest", "crate@macos-latest"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (smoke, _) = partial_projects(plan);
    let ran_jobs: BTreeSet<String> = all
        .iter()
        .filter(|j| j.split_once('@').is_none_or(|(_, os)| ran_oses.contains(&os)))
        .cloned()
        .collect();
    let mut ran = Scope {
        jobs: ran_jobs,
        whole: true,
        smoke,
        ..Scope::default()
    };
    let mut need = Scope {
        jobs: all,
        whole: true,
        ..Scope::default()
    };
    declare_selected_legs(&mut ran, &mut need, true, true, &oses);
    ci_scope(ran, false, need, false)
}

#[test]
fn a_landing_that_smoke_tests_crates_needs_only_the_legs_its_selection_runs_and_one_that_changes_crates_needs_every_os() {
    // Train y (run 9243): the train did not touch crates, the landing plan gave crates a smoke set, its `crate` job ran on Linux
    // only, but the need asked for crate@macos-latest and crate@windows-latest too, so `lands()` could never hold.
    let smoke =
        json!({"graph": {"crates": {"nextest": "package(=paddock-proc)", "affectedOnly": false, "targets": [], "modes": ["smoke"]}}});
    let d = landing_declared(&smoke, &["ubuntu-latest"]);
    assert_eq!(d.lands(), Ok(()), "the smoke selection's own legs are what the landing needs");
    assert!(d.complete().is_err(), "a smoke run lands but is never complete");
    // Control: a train that changes crates runs them in full, so the landing needs every OS, and a run without the macOS leg fails.
    let full = json!({"graph": {}});
    assert_eq!(
        landing_declared(&full, &["ubuntu-latest", "windows-latest", "macos-latest"]).lands(),
        Ok(())
    );
    let missing = landing_declared(&full, &["ubuntu-latest", "windows-latest"]).lands();
    assert!(missing.as_ref().is_err_and(|e| e.contains("crate@macos-latest")), "{missing:?}");
    // Control: the whole tree's need (a member run, a tag, a whole dispatch) is never narrowed, whatever the selection kept.
    let oses: BTreeMap<&str, BTreeSet<String>> = [("crate", BTreeSet::from(["ubuntu-latest".to_string()]))].into();
    let mut need = crate::run_scope::Scope {
        jobs: BTreeSet::from(["crate@ubuntu-latest".to_string(), "crate@macos-latest".to_string()]),
        ..crate::run_scope::Scope::default()
    };
    let mut ran = need.clone();
    declare_selected_legs(&mut ran, &mut need, true, false, &oses);
    assert!(need.jobs.contains("crate@macos-latest") && !ran.jobs.contains("crate@macos-latest"));
}
