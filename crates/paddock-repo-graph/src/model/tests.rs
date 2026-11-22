//! The project level of the model against the audit's gaps (ADR 0117 §8): each case names the gap and what today's planner
//! marked for it, so the difference is the documented one.

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
    files: Vec<String>,
}

fn world() -> World {
    let mut cargo = Graph::new();
    cargo.insert("crates/paddock-proc", node("paddock-proc", "crates", &[]));
    cargo.insert(
        "crates/paddock-watch",
        node("paddock-watch", "crates", &["crates/paddock-proc"]),
    );
    cargo.insert(
        "apps/dyno/app/src-tauri",
        node("dyno", "apps/dyno", &["crates/paddock-watch"]),
    );
    cargo.insert(
        "apps/shell-demo/src-tauri",
        node("shell-demo", "apps/shell-demo/src-tauri", &[]),
    );
    cargo.insert(
        "tools/paddock-dev",
        node("paddock-dev", "tools/paddock-dev", &["crates/paddock-proc"]),
    );
    let mut npm = Graph::new();
    npm.insert("ui", node("@d/paddock-ui", "", &[]));
    npm.insert("apps/dyno/app", node("dyno-app", "", &["ui"]));
    npm.insert("apps/shell-demo", node("shell-demo-ui", "", &["ui"]));
    npm.insert(
        "apps/shell-demo/src-tauri",
        node("shell-demo-shell", "", &["apps/shell-demo"]),
    );
    let projects = [
        ("crates", "crates"),
        ("ui", "ui"),
        ("conformance", "conformance"),
        ("tools", "tools"),
        ("paddock-dev", "tools/paddock-dev"),
        ("app:dyno", "apps/dyno"),
        ("app:shell-demo", "apps/shell-demo"),
    ]
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .to_vec();
    let files = [
        "apps/dyno/scripts/run.ps1",
        "crates/paddock-proc/src/lib.rs",
        "ui/src/a.ts",
        "docs/lessons.md",
    ]
    .map(String::from)
    .to_vec();
    World {
        cargo,
        npm,
        projects,
        files,
    }
}

fn run(w: &World, changed: &[&str], text: TextOf) -> Reached {
    let facts = Facts {
        cargo: &w.cargo,
        npm: &w.npm,
        projects: &w.projects,
        files: &w.files,
    };
    let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
    reach(&changed, &facts, &Inputs::defaults(), text)
}

fn none(_: &str) -> (Option<String>, Option<String>) {
    (None, None)
}

fn ids(r: &Reached) -> Vec<&str> {
    r.nodes.keys().map(String::as_str).collect()
}

#[test]
fn a_crate_change_reaches_its_dependents_rust_half_only() {
    let w = world();
    let r = run(&w, &["crates/paddock-proc/src/lib.rs"], &none);
    assert_eq!(ids(&r), ["app:dyno", "crates", "paddock-dev"]);
    let dyno = &r.nodes["app:dyno"];
    assert_eq!((dyno.reach, dyno.rust, dyno.ui), (Reach::Code, true, false));
}

#[test]
fn g9_an_npm_edge_into_a_tauri_shell_reaches_the_ui_half_only() {
    // Today: `npm apps/shell-demo/src-tauri` gives kind `all` (Rust and UI) to the demo.
    let w = world();
    let r = run(&w, &["ui/src/a.ts"], &none);
    let demo = &r.nodes["app:shell-demo"];
    assert_eq!((demo.rust, demo.ui), (false, true));
    assert_eq!(
        (r.nodes["app:dyno"].rust, r.nodes["app:dyno"].ui),
        (false, true)
    );
}

#[test]
fn g3_a_spike_reaches_nothing_and_an_unknown_root_file_makes_the_plan_answer_everything() {
    // Today: `unknown root file spikes/dialog-hidden/Cargo.lock` marks all 18 projects. Now a spike is "not built", and a file
    // nothing owns fails closed: every project, full, with the file named (ADR 0117 §2, review P1-B).
    let w = world();
    let spike = run(&w, &["spikes/dialog-hidden/Cargo.lock"], &none);
    assert!(
        spike.nodes.is_empty() && !spike.everything,
        "{:?}",
        spike.nodes
    );
    assert_eq!(spike.not_built, ["spikes/dialog-hidden/Cargo.lock"]);
    let r = run(&w, &["mystery.cfg"], &none);
    assert_eq!(r.unowned, ["mystery.cfg"]);
    assert!(r.everything);
    assert_eq!(r.nodes.len(), w.projects.len());
    assert!(
        r.nodes
            .values()
            .all(|n| n.reach == Reach::Code && n.rust && n.ui)
    );
}

#[test]
fn g7_g8_docs_and_reservations_reach_no_project_and_conformance_code_reaches_conformance_only() {
    // Today: `.gitea/actions/README.md` marks everything (G7); any conformance/ file marks every app, kind all (G8).
    let w = world();
    let r = run(
        &w,
        &[
            ".gitea/actions/README.md",
            "conformance/reserved/C156-branch",
        ],
        &none,
    );
    assert_eq!(ids(&r), ["docs"]);
    assert!(!r.everything);
    let c = run(
        &w,
        &["conformance/src/checks/C156-axe-scan-held-still.mjs"],
        &none,
    );
    assert_eq!(
        ids(&c),
        ["conformance"],
        "foundation changes plan no app tests"
    );
}

#[test]
fn g13_the_lessons_file_selects_its_readers_as_code() {
    // C145 and the merge driver's test read it: a docs reach would select nothing (review P1-B).
    let w = world();
    let r = run(&w, &["docs/lessons.md"], &none);
    assert_eq!(ids(&r), ["conformance", "tools"]);
    assert_eq!(r.nodes["conformance"].reach, Reach::Code);
}

#[test]
fn p1b_a_project_folder_beats_the_docs_rules_and_prose_in_it_is_docs_for_that_project() {
    let w = world();
    // A changelog is compiled in (`include_str!` in dyno's lib.rs): the declared changelog input reaches its owner as code.
    let cl = run(&w, &["apps/dyno/CHANGELOG.md"], &none);
    assert_eq!(ids(&cl), ["app:dyno"]);
    assert_eq!(cl.nodes["app:dyno"].reach, Reach::Code);
    // A README in the app is prose: docs for the app, nothing to run.
    let rd = run(&w, &["apps/dyno/README.md"], &none);
    assert_eq!(ids(&rd), ["app:dyno"]);
    assert_eq!(rd.nodes["app:dyno"].reach, Reach::Docs);
}

#[test]
fn p1a_data_in_a_project_folder_is_code_for_that_project() {
    let w = world();
    // tauri-build reads the capabilities with no literal path: code for the app's Rust half.
    let r = run(
        &w,
        &[
            "apps/dyno/app/src-tauri/capabilities/default.json",
            "apps/dyno/docs/data/table.csv",
        ],
        &none,
    );
    let d = &r.nodes["app:dyno"];
    assert_eq!((d.reach, d.rust), (Reach::Code, true));
    assert!(
        d.why.iter().any(|w| w.ends_with("table.csv")),
        "{:?}",
        d.why
    );
}

#[test]
fn p1a_every_non_prose_file_under_a_doc_owner_is_a_named_input_or_unowned() {
    let w = world();
    let g = run(&w, &["docs/gates.toml"], &none);
    assert_eq!(ids(&g), ["conformance", "paddock-dev"]);
    assert!(g.nodes.values().all(|n| n.reach == Reach::Code));
    let model = run(&w, &["docs/repo-graph.toml"], &none);
    assert_eq!(ids(&model), ["conformance", "paddock-dev"]);
    // Positive control: a register nobody declared fails closed.
    let stray = run(&w, &["docs/new-register.toml"], &none);
    assert_eq!(stray.unowned, ["docs/new-register.toml"]);
    assert!(stray.everything);
    // Prose and images under docs stay docs.
    assert_eq!(
        ids(&run(
            &w,
            &["docs/plans/x.md", "docs/images/shot.png"],
            &none
        )),
        ["docs"]
    );
}

#[test]
fn g2_a_merge_driver_change_reaches_conformance_only_and_an_eol_change_reaches_the_files_owners() {
    // Today: `.gitattributes` is GLOBAL, so train 19's merge-driver edit marked all 18 projects.
    let w = world();
    let merge = |_: &str| {
        (
            Some("/docs/lessons.md merge=union\n".to_string()),
            Some("# driver\n/docs/lessons.md merge=lessons\n".to_string()),
        )
    };
    assert_eq!(ids(&run(&w, &[".gitattributes"], &merge)), ["conformance"]);
    let eol = |_: &str| (Some(String::new()), Some("*.ps1 eol=crlf\n".to_string()));
    assert_eq!(
        ids(&run(&w, &[".gitattributes"], &eol)),
        ["app:dyno", "conformance"]
    );
}

#[test]
fn the_two_true_globals_reach_every_project() {
    let w = world();
    let r = run(&w, &["rust-toolchain.toml"], &none);
    assert_eq!(r.nodes.len(), w.projects.len());
    assert!(
        r.nodes
            .values()
            .all(|n| n.reach == Reach::Code && n.rust && n.ui)
    );
}

#[test]
fn the_strongest_kind_wins_and_reasons_accumulate() {
    let w = world();
    let r = run(&w, &["docs/lessons.md", "tools/bootstrap.sh"], &none);
    let t = &r.nodes["tools"];
    assert_eq!(t.reach, Reach::Code);
    assert_eq!(t.why.len(), 2);
    assert_eq!(Reach::parse("plumbing"), Ok(Reach::Plumbing));
    assert!(Reach::parse("everything").is_err());
}
