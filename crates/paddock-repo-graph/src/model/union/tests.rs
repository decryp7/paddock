//! Review of 59e5f7ec3 (2026-11-23): selection is a union per project (P1-1), only declared setup-only actions are plumbing (the
//! coordinator's runner-actions decision), a file owned by several nodes seeds each of them (P1-3), and an unresolved reference
//! puts its referrer's projects on full in one place (the structural rule of ADR 0117 §4/§14). Each with its positive control.

use super::nodes::{Edge, EdgeKind, Model, ModelNode, NodeKind};
use super::*;
use crate::Node;

fn node(name: &str, ws: &str, deps: &[&str]) -> Node {
    Node {
        name: name.into(),
        workspace: ws.into(),
        deps: deps.iter().map(|d| d.to_string()).collect(),
    }
}

struct World {
    cargo: Graph,
    npm: Graph,
    projects: Vec<(String, String)>,
}

fn world() -> World {
    let mut cargo = Graph::new();
    cargo.insert("apps/dyno/crates/core", node("dyno-core", "apps/dyno", &[]));
    cargo.insert(
        "crates/paddock-dialogs",
        node("paddock-dialogs", "crates", &[]),
    );
    let mut npm = Graph::new();
    npm.insert("apps/dyno/app", node("dyno-app", "", &[]));
    npm.insert("ui", node("@d/paddock-ui", "", &[]));
    let projects = [
        ("crates", "crates"),
        ("ui", "ui"),
        ("app:dyno", "apps/dyno"),
    ]
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .to_vec();
    World {
        cargo,
        npm,
        projects,
    }
}

/// A CI model: `job:ci.yml#app` runs app:dyno and uses `action` (setup-only or not); the release job of dyno-release.yml uses it
/// too; a spec of dyno's Vitest target.
fn ci(setup_only: bool) -> Model {
    let mut m = Model::new();
    let mut a = ModelNode::new(
        "action:build-folder",
        NodeKind::Action,
        &[".gitea/actions/build-folder/"],
    );
    a.setup_only = setup_only;
    m.add_node(a);
    m.add_node(ModelNode::new("job:ci.yml#app", NodeKind::Job, &[]));
    let mut bundle = ModelNode::new("job:dyno-release.yml#bundle", NodeKind::Job, &[]);
    bundle.release = true;
    m.add_node(bundle);
    m.add_edge(Edge::new(
        "job:ci.yml#app",
        "action:build-folder",
        EdgeKind::Uses,
    ));
    m.add_edge(Edge::new("job:ci.yml#app", "app:dyno", EdgeKind::Runs));
    m.add_edge(Edge::new(
        "job:dyno-release.yml#bundle",
        "action:build-folder",
        EdgeKind::Uses,
    ));
    m.add_edge(Edge::new(
        "job:dyno-release.yml#bundle",
        "app:dyno",
        EdgeKind::Runs,
    ));
    m.add_node(
        ModelNode::new("vitest:apps/dyno/app", NodeKind::TestTarget, &[])
            .with_home("apps/dyno/app"),
    );
    m.add_node(
        ModelNode::new(
            "test:apps/dyno/app/src/a.spec.ts",
            NodeKind::TestFile,
            &["apps/dyno/app/src/a.spec.ts"],
        )
        .with_home("apps/dyno/app"),
    );
    m.add_edge(Edge::new(
        "vitest:apps/dyno/app",
        "test:apps/dyno/app/src/a.spec.ts",
        EdgeKind::Tests,
    ));
    m
}

fn run(w: &World, m: &Model, changed: &[&str]) -> Reached {
    let files: Vec<String> = Vec::new();
    let facts = Facts {
        cargo: &w.cargo,
        npm: &w.npm,
        projects: &w.projects,
        files: &files,
        detect: None,
        model: Some(m),
        seeds: &[],
    };
    let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
    reach(&changed, &facts, &super::fixture::inputs(), &|_| {
        (None, None)
    })
}

fn smoke() -> BTreeSet<String> {
    BTreeSet::from(["app:dyno".to_string()])
}

fn sel(full: bool, smoke: bool, affected: &[&str], installers: bool) -> Selection {
    Selection {
        full,
        smoke,
        affected: affected.iter().map(|s| s.to_string()).collect(),
        installers,
        phases: BTreeSet::new(),
    }
}

#[test]
fn a_release_reach_builds_the_installers_and_still_runs_the_apps_smoke_set_or_its_suite() {
    // The WP-5 train replay's release-installers-only defect (39 cases): a release reach never drops the app's own gates.
    let w = world();
    let mut m = Model::new();
    let mut a = ModelNode::new(
        "action:sccache-stats",
        NodeKind::Action,
        &[".gitea/actions/sccache-stats/"],
    );
    a.setup_only = false;
    m.add_node(a);
    let mut bundle = ModelNode::new("job:dyno-release.yml#bundle", NodeKind::Job, &[]);
    bundle.release = true;
    m.add_node(bundle);
    m.add_edge(Edge::new(
        "job:dyno-release.yml#bundle",
        "action:sccache-stats",
        EdgeKind::Uses,
    ));
    m.add_edge(Edge::new(
        "job:dyno-release.yml#bundle",
        "app:dyno",
        EdgeKind::Runs,
    ));
    let r = run(&w, &m, &[".gitea/actions/sccache-stats/action.yml"]);
    assert_eq!(r.nodes["app:dyno"].kinds, BTreeSet::from([Reach::Release]));
    assert_eq!(
        selection(&r, &smoke())["app:dyno"],
        sel(false, true, &[], true),
        "declared: installers and smoke"
    );
    assert_eq!(
        selection(&r, &BTreeSet::new())["app:dyno"],
        sel(true, false, &[], true),
        "undeclared: installers and full"
    );
}

#[test]
fn a_contract_phase_a_change_reaches_runs_that_phase_not_the_suite() {
    // Difference 4 of §8: a conformance change runs each app's conformance step (its `checks` phase), not its tests.
    let w = world();
    let mut m = Model::new();
    m.add_node(ModelNode::new(
        "script:conformance/bin/cli.mjs",
        NodeKind::Script,
        &["conformance/bin/cli.mjs"],
    ));
    m.add_node(
        ModelNode::new("step:apps/dyno/paddock.ci.json#checks", NodeKind::Step, &[])
            .with_home("apps/dyno"),
    );
    m.add_edge(Edge::new(
        "step:apps/dyno/paddock.ci.json#checks",
        "script:conformance/bin/cli.mjs",
        EdgeKind::Runs,
    ));
    let r = run(&w, &m, &["conformance/bin/cli.mjs"]);
    let s = &selection(&r, &smoke())["app:dyno"];
    assert_eq!(s.phases, BTreeSet::from(["checks".to_string()]));
    assert!(!s.full && !s.smoke && s.affected.is_empty(), "{s:?}");
    // Control: without the step's runs edge the app is not reached.
    let mut c = Model::new();
    c.add_node(ModelNode::new(
        "script:conformance/bin/cli.mjs",
        NodeKind::Script,
        &["conformance/bin/cli.mjs"],
    ));
    c.add_node(
        ModelNode::new("step:apps/dyno/paddock.ci.json#checks", NodeKind::Step, &[])
            .with_home("apps/dyno"),
    );
    assert!(
        !run(&w, &c, &["conformance/bin/cli.mjs"])
            .nodes
            .contains_key("app:dyno")
    );
}

#[test]
fn p1_1_code_and_release_select_full_and_installers() {
    let w = world();
    let r = run(
        &w,
        &ci(false),
        &[
            "apps/dyno/crates/core/src/lib.rs",
            ".gitea/actions/build-folder/action.yml",
        ],
    );
    assert_eq!(
        selection(&r, &smoke())["app:dyno"],
        sel(true, false, &[], true)
    );
    // Control: the code change alone runs no installers.
    let c = run(&w, &ci(false), &["apps/dyno/crates/core/src/lib.rs"]);
    assert_eq!(
        selection(&c, &smoke())["app:dyno"],
        sel(true, false, &[], false)
    );
}

#[test]
fn p1_1_an_action_a_release_job_and_a_test_job_use_keeps_both_installers_and_the_jobs_run() {
    // build-folder (not setup-only): installers for the release job and the app leg in full; today's max kept only installers.
    let w = world();
    let r = run(&w, &ci(false), &[".gitea/actions/build-folder/run.sh"]);
    assert_eq!(
        selection(&r, &smoke())["app:dyno"],
        sel(true, false, &[], true)
    );
}

#[test]
fn p1_1_a_spec_and_a_setup_only_action_select_the_spec_and_the_smoke_set() {
    let w = world();
    let r = run(
        &w,
        &ci(true),
        &[
            "apps/dyno/app/src/a.spec.ts",
            ".gitea/actions/build-folder/action.yml",
        ],
    );
    assert_eq!(
        selection(&r, &smoke())["app:dyno"],
        sel(false, true, &["vitest:apps/dyno/app"], false)
    );
    // Control: the spec alone has no smoke set.
    let c = run(&w, &ci(true), &["apps/dyno/app/src/a.spec.ts"]);
    assert_eq!(
        selection(&c, &smoke())["app:dyno"],
        sel(false, false, &["vitest:apps/dyno/app"], false)
    );
}

#[test]
fn only_a_declared_setup_only_action_is_plumbing_any_other_runs_its_jobs_in_full() {
    let w = world();
    let undeclared = run(&w, &ci(false), &[".gitea/actions/build-folder/action.yml"]);
    assert!(
        undeclared.nodes["app:dyno"].kinds.contains(&Reach::Code),
        "{:?}",
        undeclared.nodes
    );
    // Control: declared setup-only, the same change is plumbing (smoke) and no release.
    let declared = run(&w, &ci(true), &[".gitea/actions/build-folder/action.yml"]);
    assert_eq!(
        declared.nodes["app:dyno"].kinds,
        BTreeSet::from([Reach::Plumbing])
    );
    assert_eq!(
        selection(&declared, &smoke())["app:dyno"],
        sel(false, true, &[], false)
    );
}

#[test]
fn p1_3_a_file_two_nodes_own_seeds_both_a_test_file_and_the_data_a_crate_reads() {
    let w = world();
    let mut m = ci(true);
    m.add_node(ModelNode::new(
        "data:apps/dyno/app/src/a.spec.ts",
        NodeKind::Project,
        &["apps/dyno/app/src/a.spec.ts"],
    ));
    m.add_node(ModelNode::new(
        "crate:crates/paddock-dialogs",
        NodeKind::Project,
        &["crates/paddock-dialogs/"],
    ));
    m.add_edge(Edge::new(
        "crate:crates/paddock-dialogs",
        "data:apps/dyno/app/src/a.spec.ts",
        EdgeKind::Reads,
    ));
    let r = run(&w, &m, &["apps/dyno/app/src/a.spec.ts"]);
    assert!(
        r.targets["app:dyno"].contains("vitest:apps/dyno/app"),
        "{r:?}"
    );
    assert_eq!(r.nodes["crates"].reach, Reach::Code, "{:?}", r.nodes);
    // Control: without the reads edge the crate is not reached.
    let r2 = run(&w, &ci(true), &["apps/dyno/app/src/a.spec.ts"]);
    assert!(!r2.nodes.contains_key("crates"));
}

#[test]
fn a_reads_edge_raises_prose_outside_every_project_to_code_for_its_reader() {
    let w = world();
    let mut m = Model::new();
    m.add_node(ModelNode::new(
        "data:docs/notes.md",
        NodeKind::Project,
        &["docs/notes.md"],
    ));
    m.add_node(ModelNode::new(
        "crate:crates/paddock-dialogs",
        NodeKind::Project,
        &["crates/paddock-dialogs/"],
    ));
    m.add_edge(Edge::new(
        "crate:crates/paddock-dialogs",
        "data:docs/notes.md",
        EdgeKind::Reads,
    ));
    let r = run(&w, &m, &["docs/notes.md"]);
    assert_eq!(r.nodes["crates"].reach, Reach::Code, "{:?}", r.nodes);
    // Control: unread, the same file is docs only.
    let c = run(&w, &Model::new(), &["docs/notes.md"]);
    assert!(!c.nodes.contains_key("crates"));
}

#[test]
fn an_unresolved_reference_puts_its_referrers_projects_on_full_in_every_plan() {
    let w = world();
    let mut m = ci(true);
    m.add_node(ModelNode::new(
        "script:conformance/ci/x.mjs",
        NodeKind::Script,
        &["conformance/ci/x.mjs"],
    ));
    m.add_edge(Edge::new(
        "job:ci.yml#app",
        "script:conformance/ci/x.mjs",
        EdgeKind::Runs,
    ));
    m.add_unresolved(Unresolved {
        referrer: "conformance/ci/x.mjs".into(),
        node: None,
        reference: "./gone.mjs".into(),
        why: "resolves to no tracked file".into(),
    });
    // A change elsewhere (a crate of the shared workspace) still runs the job that runs the referrer, in full.
    let r = run(&w, &m, &["crates/paddock-dialogs/src/lib.rs"]);
    assert!(
        r.nodes["app:dyno"].kinds.contains(&Reach::Code),
        "{:?}",
        r.nodes
    );
    assert!(r.whole.contains("app:dyno"));
    assert!(
        r.nodes["app:dyno"]
            .why
            .iter()
            .any(|w| w.contains("./gone.mjs"))
    );
    // Control: resolved, the same change does not reach the app.
    let mut ok = ci(true);
    ok.add_node(ModelNode::new(
        "script:conformance/ci/x.mjs",
        NodeKind::Script,
        &["conformance/ci/x.mjs"],
    ));
    ok.add_edge(Edge::new(
        "job:ci.yml#app",
        "script:conformance/ci/x.mjs",
        EdgeKind::Runs,
    ));
    let c = run(&w, &ok, &["crates/paddock-dialogs/src/lib.rs"]);
    assert!(!c.nodes.contains_key("app:dyno"), "{:?}", c.nodes);
}

/// A release job of dyno-release.yml: a `run:` step that runs the conformance CLI (the release preflight, which imports a check) and
/// a step that uses the bundle action (a release step); the job runs app:dyno.
fn release_preflight() -> Model {
    let mut m = Model::new();
    for (id, f) in [
        ("script:conformance/bin/cli.mjs", "conformance/bin/cli.mjs"),
        (
            "script:conformance/src/checks/C07.mjs",
            "conformance/src/checks/C07.mjs",
        ),
    ] {
        m.add_node(ModelNode::new(id, NodeKind::Script, &[f]));
    }
    m.add_node(ModelNode::new(
        "action:tauri-bundle",
        NodeKind::Action,
        &[".gitea/actions/tauri-bundle/"],
    ));
    let mut job = ModelNode::new("job:dyno-release.yml#bundle", NodeKind::Job, &[]);
    job.release = true;
    m.add_node(job);
    // The run step is no release step; the step that uses the bundle action is.
    m.add_node(ModelNode::new(
        "step:dyno-release.yml#bundle/3",
        NodeKind::Step,
        &[],
    ));
    let mut bundle_step = ModelNode::new("step:dyno-release.yml#bundle/4", NodeKind::Step, &[]);
    bundle_step.release = true;
    m.add_node(bundle_step);
    for (from, to, k) in [
        (
            "job:dyno-release.yml#bundle",
            "step:dyno-release.yml#bundle/3",
            EdgeKind::Uses,
        ),
        (
            "job:dyno-release.yml#bundle",
            "step:dyno-release.yml#bundle/4",
            EdgeKind::Uses,
        ),
        (
            "step:dyno-release.yml#bundle/3",
            "script:conformance/bin/cli.mjs",
            EdgeKind::Runs,
        ),
        (
            "job:dyno-release.yml#bundle",
            "script:conformance/bin/cli.mjs",
            EdgeKind::Runs,
        ),
        (
            "script:conformance/bin/cli.mjs",
            "script:conformance/src/checks/C07.mjs",
            EdgeKind::Imports,
        ),
        (
            "step:dyno-release.yml#bundle/4",
            "action:tauri-bundle",
            EdgeKind::Uses,
        ),
        (
            "job:dyno-release.yml#bundle",
            "action:tauri-bundle",
            EdgeKind::Uses,
        ),
        ("job:dyno-release.yml#bundle", "app:dyno", EdgeKind::Runs),
    ] {
        m.add_edge(Edge::new(from, to, k));
    }
    m
}

#[test]
fn a_conformance_check_reaching_a_release_preflight_selects_the_smoke_set_not_the_installers() {
    // User decision 2026-11-23 (rule 54 narrowed): a conformance check change that reaches an app's release preflight runs the app's
    // smoke set and its checks phase; the installers stay for the bundle, sign, notarise and publish steps.
    let w = world();
    let m = release_preflight();
    let r = run(&w, &m, &["conformance/src/checks/C07.mjs"]);
    assert_eq!(r.nodes["app:dyno"].kinds, BTreeSet::from([Reach::Gate]));
    let s = &selection(&r, &smoke())["app:dyno"];
    assert!(!s.installers && s.smoke && !s.full, "{s:?}");
    assert_eq!(
        s.phases,
        BTreeSet::from(["checks".to_string()]),
        "the declared gate phases"
    );
    // Without a declared smoke set the gate runs the app's suite (never less than declared), still without installers.
    let s = &selection(&r, &BTreeSet::new())["app:dyno"];
    assert!(s.full && !s.installers, "{s:?}");
    // Control: the bundle action changed is a release step: the installers, beside the smoke set.
    let r = run(&w, &m, &[".gitea/actions/tauri-bundle/action.yml"]);
    assert_eq!(
        selection(&r, &smoke())["app:dyno"],
        sel(false, true, &[], true)
    );
}
