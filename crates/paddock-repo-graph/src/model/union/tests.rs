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
    }
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
