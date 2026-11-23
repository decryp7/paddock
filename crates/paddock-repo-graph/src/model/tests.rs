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
    run_detecting(w, changed, text, None)
}

fn run_detecting(w: &World, changed: &[&str], text: TextOf, detect: Option<DetectOf>) -> Reached {
    let facts = Facts {
        cargo: &w.cargo,
        npm: &w.npm,
        projects: &w.projects,
        files: &w.files,
        detect,
        model: None,
        seeds: &[],
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
    // c99 and ui read every Rust source (C99, the cause-code scan): additive inputs, as the rule tables had them.
    assert_eq!(ids(&r), ["app:dyno", "c99", "crates", "paddock-dev", "ui"]);
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
    // C100 reads every README (an additive input); the docs node takes the prose.
    assert_eq!(ids(&r), ["conformance", "docs"]);
    assert!(!r.everything);
    let c = run(
        &w,
        &["conformance/src/checks/C156-axe-scan-held-still.mjs"],
        &none,
    );
    // C99 and the parity tests read conformance code too (additive inputs); no app is reached.
    assert_eq!(
        ids(&c),
        ["c99", "conformance", "paddock-dev"],
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
    assert_eq!(
        ids(&rd),
        ["app:dyno", "conformance"],
        "C100 reads every README"
    );
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

#[test]
fn p1a_round3_text_in_a_project_folder_is_code_and_only_readmes_changelogs_licences_and_docs_are_prose()
 {
    // Round 3, P1-A: 237 `.txt` files in project folders are mostly test data (crates/paddock-service/tests/recorded/*.txt read by
    // a folder join, paddock-names/src/words.txt under `include_str!`, conformance/ci/ports.txt). Inside a project folder prose is
    // README*, CHANGELOG*, LICENSE* and the project's own docs/** only.
    let w = world();
    for data in [
        "crates/paddock-proc/tests/recorded/run.txt",
        "crates/paddock-proc/src/words.txt",
        "conformance/ci/ports.txt",
        "crates/paddock-proc/NOTES.md",
    ] {
        let r = run(&w, &[data], &none);
        assert!(
            r.nodes.values().any(|n| n.reach == Reach::Code),
            "{data} must reach code: {:?}",
            r.nodes
        );
    }
    for prose in [
        "crates/paddock-proc/README.md",
        "crates/paddock-proc/CHANGELOG.txt",
        "crates/paddock-proc/LICENSE-MIT",
        "apps/dyno/docs/guide/start.md",
    ] {
        let r = run(&w, &[prose], &none);
        let owner = r
            .nodes
            .iter()
            .find(|(id, _)| id.as_str() != "conformance")
            .map(|(_, n)| n);
        assert!(
            owner.is_some_and(|n| n.reach == Reach::Docs),
            "{prose} is prose: {:?}",
            r.nodes
        );
    }
}

#[test]
fn round3_templates_reach_paddock_dev_which_compiles_one_in() {
    // tools/paddock-dev/src/brief_check.rs:609 `include_str!`s a template (review round 3, P2).
    let w = world();
    let r = run(&w, &["templates/brief-template.toml"], &none);
    assert!(!r.everything, "{:?}", r.unowned);
    assert!(r.nodes.contains_key("paddock-dev"), "{:?}", r.nodes);
    assert_eq!(r.nodes["paddock-dev"].reach, Reach::Code);
}

#[test]
fn d14_detection_classes_binary_under_docs_as_an_asset_and_a_renamed_text_png_as_unowned() {
    let w = world();
    let detect = |p: &str| -> Option<Detected> {
        Some(match p {
            "docs/shots/raw.bin" | "docs/images/a.png" => Detected::Binary,
            "docs/images/fake.png" => Detected::Text,
            "docs/new.toml" => Detected::Toml,
            _ => Detected::Text,
        })
    };
    // A binary file under a doc owner is a documentation asset without being listed.
    let bin = run_detecting(
        &w,
        &["docs/shots/raw.bin", "docs/images/a.png"],
        &none,
        Some(&detect),
    );
    assert_eq!(ids(&bin), ["docs"], "{:?}", bin.unowned);
    // Positive control: a `.png` that holds text is not an asset; the error names its detected kind.
    let fake = run_detecting(
        &w,
        &["docs/images/fake.png", "docs/new.toml"],
        &none,
        Some(&detect),
    );
    assert_eq!(fake.unowned, ["docs/images/fake.png", "docs/new.toml"]);
    assert!(fake.everything);
    assert_eq!(fake.unowned_kind["docs/images/fake.png"], Detected::Text);
    assert_eq!(fake.unowned_kind["docs/new.toml"], Detected::Toml);
    // Detection never narrows inside a project folder: binary data there is still code.
    let data = run_detecting(&w, &["apps/dyno/tests/blob.dat"], &none, Some(&detect));
    assert_eq!(data.nodes["app:dyno"].reach, Reach::Code);
}

#[cfg(feature = "content")]
#[test]
fn d14_detect_reads_the_first_kilobyte() {
    assert_eq!(detect(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Detected::Binary);
    assert_eq!(detect(b"{\"a\": [1, 2]}\n"), Detected::Json);
    assert_eq!(
        detect(b"# c\nversion = 1\n[[input]]\nname = \"x\"\n"),
        Detected::Toml
    );
    assert_eq!(detect(b"just words\nmore words\n"), Detected::Text);
    // A text file with a PNG name is text: detection reads bytes, never the name.
    assert_eq!(detect(b"not an image"), Detected::Text);
}

#[test]
fn npm_lock_reaches_only_the_workspaces_whose_packages_changed() {
    let w = world();
    let lock = |v: &str| {
        format!(
            r#"{{"packages":{{"":{{"workspaces":["ui"]}},"ui":{{"dependencies":{{"react":"1"}}}},"node_modules/react":{{"version":"{v}"}}}}}}"#
        )
    };
    let (a, b) = (lock("1"), lock("2"));
    let changed = move |_: &str| (Some(a.clone()), Some(b.clone()));
    let r = run(&w, &["package-lock.json"], &changed);
    // ui changed, and its npm dependents (dyno's app, the demo and its shell) follow on their UI half.
    assert_eq!(ids(&r), ["app:dyno", "app:shell-demo", "ui"]);
    assert!(r.nodes.values().all(|n| n.ui && !n.rust));
    // Control: the same lock reaches nothing.
    let same = lock("1");
    let unchanged = move |_: &str| (Some(same.clone()), Some(same.clone()));
    assert!(run(&w, &["package-lock.json"], &unchanged).nodes.is_empty());
}

#[test]
fn an_extractor_that_fails_or_is_unknown_fails_closed() {
    let w = world();
    let broken = |_: &str| (Some("{ not json".to_string()), Some("{}".to_string()));
    let r = run(&w, &["package-lock.json"], &broken);
    assert_eq!(
        r.nodes.len(),
        w.projects.len(),
        "a lock that cannot be read reaches every project"
    );
    let mut inputs = Inputs::defaults();
    inputs.inputs.push(inputs::Input {
        name: "odd".into(),
        files: vec!["odd.cfg".into()],
        reaches: vec![inputs::Target::Project("tools".into())],
        reach: Reach::Code,
        extractor: Some("magic".into()),
        adds: false,
        except: Vec::new(),
    });
    let facts = Facts {
        cargo: &w.cargo,
        npm: &w.npm,
        projects: &w.projects,
        files: &w.files,
        detect: None,
        model: None,
        seeds: &[],
    };
    let r = reach(&["odd.cfg".to_string()], &facts, &inputs, &none);
    assert_eq!(r.nodes.len(), w.projects.len());
}

#[test]
fn guards_add_conformance_to_a_test_file_whose_ignores_moved_and_keep_its_owner() {
    let w = world();
    let moved = |_: &str| {
        (
            Some("#[test]\n#[ignore]\nfn one() {}\nfn two() {}\n".to_string()),
            Some("fn one() {}\n#[test]\n#[ignore]\nfn two() {}\n".to_string()),
        )
    };
    let r = run(&w, &["crates/paddock-proc/tests/it/a.rs"], &moved);
    assert!(
        r.nodes.contains_key("conformance") && r.nodes.contains_key("crates"),
        "{:?}",
        r.nodes
    );
    // Control: no guard changed, the owner only.
    let same = |_: &str| {
        (
            Some("fn a() {}\n".to_string()),
            Some("fn a() { 1 }\n".to_string()),
        )
    };
    let r = run(&w, &["crates/paddock-proc/tests/it/a.rs"], &same);
    assert!(!r.nodes.contains_key("conformance"), "{:?}", r.nodes);
}

fn ci_model(runs_dyno: bool) -> nodes::Model {
    use nodes::{Edge, EdgeKind, ModelNode, NodeKind};
    let mut m = nodes::Model::new();
    m.add_node(ModelNode::new(
        "action:cache",
        NodeKind::Action,
        &[".gitea/actions/cache/"],
    ));
    m.add_node(ModelNode::new(
        "wf:ci.yml",
        NodeKind::Workflow,
        &[".gitea/workflows/ci.yml"],
    ));
    m.add_node(ModelNode::new("job:ci.yml#app", NodeKind::Job, &[]));
    m.add_node(ModelNode::new("job:ci.yml#ui", NodeKind::Job, &[]));
    m.add_node(ModelNode::new(
        "script:conformance/ci/x.sh",
        NodeKind::Script,
        &["conformance/ci/x.sh"],
    ));
    m.add_edge(Edge::new("job:ci.yml#app", "action:cache", EdgeKind::Uses));
    m.add_edge(Edge::new(
        "job:ci.yml#ui",
        "script:conformance/ci/x.sh",
        EdgeKind::Runs,
    ));
    m.add_edge(Edge::new("job:ci.yml#ui", "ui", EdgeKind::Runs));
    if runs_dyno {
        m.add_edge(Edge::new("job:ci.yml#app", "app:dyno", EdgeKind::Runs));
    }
    m
}

fn run_model(
    w: &World,
    m: &nodes::Model,
    changed: &[&str],
    seeds: &[(String, String, String)],
) -> Reached {
    let facts = Facts {
        cargo: &w.cargo,
        npm: &w.npm,
        projects: &w.projects,
        files: &w.files,
        detect: None,
        model: Some(m),
        seeds,
    };
    let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
    reach(&changed, &facts, &Inputs::defaults(), &none)
}

#[test]
fn g6_a_shared_action_reaches_the_projects_of_its_jobs_as_plumbing_and_the_control_drops_the_runs_edge()
 {
    let w = world();
    let r = run_model(
        &w,
        &ci_model(true),
        &[".gitea/actions/cache/action.yml"],
        &[],
    );
    assert_eq!(ids(&r), ["app:dyno"]);
    assert_eq!(r.nodes["app:dyno"].reach, Reach::Plumbing);
    assert!(!r.everything);
    // Positive control: without the job's `runs` edge to the app nothing is reached.
    let c = run_model(
        &w,
        &ci_model(false),
        &[".gitea/actions/cache/action.yml"],
        &[],
    );
    assert!(c.nodes.is_empty(), "{:?}", c.nodes);
}

#[test]
fn a_script_in_a_project_folder_is_code_for_its_project_and_plumbing_for_its_jobs() {
    let w = world();
    let r = run_model(&w, &ci_model(true), &["conformance/ci/x.sh"], &[]);
    assert_eq!(r.nodes["conformance"].reach, Reach::Code);
    assert_eq!(r.nodes["ui"].reach, Reach::Plumbing);
}

#[test]
fn a_workflow_edit_seeds_only_the_jobs_whose_text_changed() {
    let w = world();
    let seeds = [(
        ".gitea/workflows/ci.yml".to_string(),
        "job:ci.yml#ui".to_string(),
        "job ui changed".to_string(),
    )];
    let r = run_model(&w, &ci_model(true), &[".gitea/workflows/ci.yml"], &seeds);
    assert_eq!(ids(&r), ["ui"]);
    assert_eq!(r.nodes["ui"].reach, Reach::Code);
    // Control: a .gitea file no node owns fails closed.
    let u = run_model(&w, &ci_model(true), &[".gitea/stray.yml"], &[]);
    assert!(
        u.everything && u.unowned == [".gitea/stray.yml"],
        "{:?}",
        u.unowned
    );
}

#[test]
fn each_migrated_additive_rule_adds_its_reader_and_the_control_removes_it() {
    // One case per former rule table (ADR 0117 §8 "one golden per migrated rule"); the control drops the input and the reader goes.
    let w = world();
    let cases = [
        ("c99-reads", "crates/paddock-proc/src/lib.rs", "c99"),
        ("cause-code-scan", "crates/paddock-proc/src/lib.rs", "ui"),
        ("c100-reads", "apps/dyno/AGENTS.md", "conformance"),
        (
            "conformance-parity",
            "conformance/src/checks/c1.mjs",
            "paddock-dev",
        ),
        ("hooks-parity", "tools/paddock-dev/src/places.rs", "tools"),
        (
            "app-capabilities",
            "apps/dyno/app/src-tauri/capabilities/default.json",
            "conformance",
        ),
        (
            "per-machine",
            "apps/dyno/app/src-tauri/windows/hooks.nsh",
            "conformance",
        ),
        (
            "e2e-guards",
            "ui/projects/paddock-e2e/src/env.ts",
            "app:dyno",
        ),
        ("notices", "scripts/notices.sh", "app:shell-demo"),
        (
            "generated-bindings",
            "ui/projects/paddock-ui/src/generated/health.ts",
            "crates",
        ),
    ];
    for (name, file, reader) in cases {
        let facts = Facts {
            cargo: &w.cargo,
            npm: &w.npm,
            projects: &w.projects,
            files: &w.files,
            detect: None,
            model: None,
            seeds: &[],
        };
        let all = Inputs::defaults();
        let by_input = |r: &Reached| {
            r.nodes.get(reader).is_some_and(|n| {
                n.why
                    .iter()
                    .any(|y| y.starts_with(&format!("input {name} ")))
            })
        };
        let r = reach(&[file.to_string()], &facts, &all, &none);
        assert!(
            by_input(&r),
            "{name}: {file} must reach {reader}: {:?}",
            r.nodes.get(reader)
        );
        let mut without = Inputs::defaults();
        without.inputs.retain(|i| i.name != name);
        let c = reach(&[file.to_string()], &facts, &without, &none);
        assert!(
            !by_input(&c),
            "{name}: the control still reaches {reader} through it"
        );
        assert!(!c.everything, "{name}: the control must not fail closed");
    }
}

#[test]
fn g7_a_readme_in_an_action_folder_is_docs_even_with_the_model() {
    let w = world();
    let r = run_model(
        &w,
        &ci_model(true),
        &[".gitea/actions/cache/README.md"],
        &[],
    );
    assert!(!r.nodes.contains_key("app:dyno"), "{:?}", r.nodes);
    assert!(r.nodes.contains_key("docs"));
}

#[test]
fn a_workspace_config_reaches_every_member_and_a_nested_package_reaches_its_containing_package() {
    // From --compare on the real tree: crates/Cargo.toml reached no member, and ui/projects/paddock-ui/src/x.ts did not reach
    // the apps (they read ui/dist, which the containing package `ui` builds from its projects).
    let mut w = world();
    let r = run(&w, &["crates/Cargo.toml"], &none);
    assert!(
        r.nodes.get("app:dyno").is_some_and(|n| n.rust),
        "{:?}",
        r.nodes.keys()
    );
    assert!(r.nodes.contains_key("paddock-dev"));
    w.npm.insert("ui/projects/lib", node("@d/lib", "", &[]));
    let n = run(&w, &["ui/projects/lib/src/a.ts"], &none);
    assert!(
        n.nodes.get("app:dyno").is_some_and(|n| n.ui),
        "{:?}",
        n.nodes.keys()
    );
    // From --compare: a UI file inside an app's Cargo workspace folder is no workspace config (it reached every Debrief crate).
    w.cargo.insert(
        "apps/dyno/fuzz",
        node("dyno-fuzz", "apps/dyno", &["apps/dyno/app/src-tauri"]),
    );
    let ui = run(&w, &["apps/dyno/app/src/main.ts"], &none);
    assert!(
        ui.nodes.get("app:dyno").is_some_and(|n| !n.rust),
        "{:?}",
        ui.nodes.get("app:dyno")
    );
    // Control: a crate's own file reaches only its dependents, not its workspace siblings.
    let c = run(&w, &["crates/paddock-watch/src/lib.rs"], &none);
    assert!(
        !c.nodes
            .get("crates")
            .is_some_and(|n| n.why.iter().any(|y| y.contains("paddock-proc")))
    );
}

#[test]
fn an_owner_input_is_its_projects_own_code_and_reaches_its_packages_dependents() {
    // From --compare: ui/package.json (the lint-wiring input, target owner) reached ui and eslint but not the apps that use ui.
    let w = world();
    let r = run(&w, &["ui/package.json"], &none);
    assert!(
        r.nodes.get("app:dyno").is_some_and(|n| n.ui),
        "{:?}",
        r.nodes.keys()
    );
}
