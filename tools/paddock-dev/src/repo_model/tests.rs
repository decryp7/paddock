//! `--compare` and the batched reader, with their positive controls (ADR 0117 §8).
#![allow(clippy::unwrap_used)]

use super::*;
use paddock_repo_graph::Graph;
use paddock_repo_graph::model::NodeReach;

fn graph() -> RepoGraph {
    RepoGraph {
        cargo: Graph::new(),
        npm: Graph::new(),
        apps: ["dyno".to_string()].into_iter().collect(),
        configs: BTreeMap::new(),
        ..Default::default()
    }
}

fn hit(reach: Reach, rust: bool, ui: bool) -> NodeReach {
    NodeReach {
        reach,
        kinds: [reach].into_iter().collect(),
        rust,
        ui,
        why: vec!["model reason".into()],
    }
}

fn plan() -> Value {
    json!({"all": false, "crates": true, "ui": false, "eslint": false, "conformance": true, "c99": false, "tools": false,
           "icons": false, "ciImage": false, "paddockDev": false, "docs": false, "apps": ["dyno"],
           "kinds": {"dyno": ["rust"]}, "why": {"crates": ["crates/a/src/lib.rs"]}})
}

#[test]
fn the_same_answer_is_same() {
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r.nodes.insert("app:dyno".into(), hit(Reach::Code, true, false));
    let c = compare(&plan(), &r, &graph(), &BTreeSet::new());
    assert_eq!(c["same"], true, "{c}");
}

#[test]
fn compare_reports_a_rule_the_model_lacks_and_names_what_it_drops() {
    // Positive control (§8 "--compare"): take the app's edge out of the model's answer; --compare names the difference and the drop.
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    let c = compare(&plan(), &r, &graph(), &BTreeSet::new());
    assert_eq!(c["same"], false);
    assert_eq!(c["dropped"], json!(["app:dyno"]));
    assert_eq!(c["differences"][0]["project"], "app:dyno");
    assert_eq!(c["differences"][0]["rules"], "rust");
    assert_eq!(c["differences"][0]["model"], Value::Null);
}

#[test]
fn a_docs_reach_runs_nothing_and_a_half_is_a_narrowing() {
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r.nodes.insert("app:dyno".into(), hit(Reach::Docs, false, false));
    assert_eq!(compare(&plan(), &r, &graph(), &BTreeSet::new())["dropped"], json!(["app:dyno"]));
    let mut p = plan();
    p["kinds"]["dyno"] = json!(["all"]);
    let mut r2 = Reached::default();
    r2.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r2.nodes.insert("app:dyno".into(), hit(Reach::Code, false, true));
    let c = compare(&p, &r2, &graph(), &BTreeSet::new());
    assert_eq!(c["dropped"], json!(["app:dyno"]), "{c}");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn the_bulk_reader_starts_at_most_one_git_for_many_files_and_the_control_counts_one_per_reader() {
    // ER-20, ADR 0117 §9: one `git cat-file --batch-command` for every text. The control: a reader per file starts one git each,
    // so the counter sees spawns.
    let root = repo_root();
    let files: Vec<String> = ["Cargo.toml", "Cargo.lock", "src/lib.rs", "src/plan.rs", "src/repo_model.rs"]
        .iter()
        .map(|f| format!("tools/paddock-dev/{f}"))
        .collect();
    let one = GitBulk::through_git(&root, Some("HEAD"));
    let texts = one.read_all(&files);
    assert!(texts.iter().filter(|t| t.is_some()).count() >= 4, "the files are read");
    assert_eq!(one.starts(), 1, "one process for every read");
    let per_file: usize = files
        .iter()
        .map(|f| {
            let r = GitBulk::through_git(&root, Some("HEAD"));
            r.read_all(std::slice::from_ref(f));
            r.starts()
        })
        .sum();
    assert_eq!(per_file, files.len(), "the control: one reader per file starts one git each");
}

/// The repository's own declarations (docs/repo-graph.toml through the one accessor) on a small world: the behaviours the crate's
/// tests pin on fixture data, here on the real file.
mod declared {
    use super::*;
    use paddock_repo_graph::Node;
    use paddock_repo_graph::model::inputs::Inputs;
    use paddock_repo_graph::model::{Facts, reach};

    fn node(name: &str, ws: &str, deps: &[&str]) -> Node {
        Node {
            name: name.into(),
            workspace: ws.into(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    fn inputs() -> Inputs {
        crate::repo_inputs::load(&repo_root()).unwrap()
    }

    fn run(changed: &[&str], inputs: &Inputs, text: paddock_repo_graph::model::TextOf) -> Reached {
        let mut cargo = Graph::new();
        cargo.insert("crates/paddock-proc", node("paddock-proc", "crates", &[]));
        cargo.insert("apps/dyno/app/src-tauri", node("dyno", "apps/dyno", &["crates/paddock-proc"]));
        let mut npm = Graph::new();
        npm.insert("ui", node("ui", "", &[]));
        npm.insert("apps/dyno/app", node("dyno-app", "", &["ui"]));
        let projects: Vec<(String, String)> = ROOT_PROJECTS
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .chain([("app:dyno".to_string(), "apps/dyno".to_string())])
            .collect();
        let files: Vec<String> = ["apps/dyno/scripts/run.ps1", "crates/paddock-proc/src/lib.rs"]
            .map(String::from)
            .to_vec();
        let facts = Facts {
            cargo: &cargo,
            npm: &npm,
            projects: &projects,
            files: &files,
            detect: None,
            model: None,
            seeds: &[],
        };
        let changed: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
        reach(&changed, &facts, inputs, text)
    }

    fn none(_: &str) -> (Option<String>, Option<String>) {
        (None, None)
    }

    #[test]
    fn the_gaps_close_on_the_declared_inputs() {
        let i = inputs();
        // G3: a spike is not built, an unknown root file fails closed.
        let s = run(&["spikes/dialog-hidden/Cargo.lock"], &i, &none);
        assert!(s.nodes.is_empty() && !s.everything);
        assert!(run(&["mystery.cfg"], &i, &none).everything);
        // G7 and G8: a README under .gitea and a check-id reservation reach no project's tests.
        let g = run(&[".gitea/actions/README.md", "conformance/reserved/C999-x"], &i, &none);
        assert!(
            !g.everything && !g.nodes.keys().any(|k| k.starts_with("app:")),
            "{:?}",
            g.nodes.keys()
        );
        // G13: the lessons file is code for its readers.
        assert_eq!(run(&["docs/lessons.md"], &i, &none).nodes["conformance"].reach, Reach::Code);
        // G2: a merge-driver change reaches conformance only; an eol change reaches the owners of what it matches.
        let merge = |_: &str| {
            (
                Some("/docs/lessons.md merge=union\n".to_string()),
                Some("/docs/lessons.md merge=x\n".to_string()),
            )
        };
        assert_eq!(
            run(&[".gitattributes"], &i, &merge).nodes.keys().collect::<Vec<_>>(),
            ["conformance"]
        );
        let eol = |_: &str| (Some(String::new()), Some("*.ps1 eol=crlf\n".to_string()));
        assert!(run(&[".gitattributes"], &i, &eol).nodes.contains_key("app:dyno"));
        // Round 3 P1-A: test data in a project folder is code; a README is prose.
        assert_eq!(
            run(&["crates/paddock-proc/tests/recorded/x.txt"], &i, &none).nodes["crates"].reach,
            Reach::Code
        );
        let readme = format!("crates/paddock-proc/{}", "README.md");
        assert_eq!(run(&[readme.as_str()], &i, &none).nodes["crates"].reach, Reach::Docs);
        // The true globals reach every project.
        assert!(run(&["rust-toolchain.toml"], &i, &none).nodes.len() >= ROOT_PROJECTS.len());
        assert!(
            !run(&["ui/package.json"], &i, &none).nodes.contains_key("tools"),
            "a leading slash anchors package.json at the root"
        );
    }

    #[test]
    fn each_additive_input_adds_its_reader_and_the_control_removes_it() {
        let cases = [
            ("c99-reads", "crates/paddock-proc/src/lib.rs", "c99"),
            ("cause-code-scan", "crates/paddock-proc/src/lib.rs", "ui"),
            ("c100-reads", "apps/x/AGENTS.md", "conformance"),
            ("conformance-parity", "conformance/src/checks/c1.mjs", "paddock-dev"),
            ("hooks-parity", "tools/paddock-dev/src/places.rs", "tools"),
            (
                "app-capabilities",
                "apps/dyno/app/src-tauri/capabilities/default.json",
                "conformance",
            ),
            ("per-machine", "apps/dyno/app/src-tauri/windows/hooks.nsh", "conformance"),
            ("e2e-guards", "ui/projects/paddock-e2e/src/env.ts", "app:dyno"),
            ("notices", "scripts/notices.sh", "app:dyno"),
            ("generated-bindings", "ui/projects/paddock-ui/src/generated/health.ts", "crates"),
            ("templates", "templates/brief-template.toml", "paddock-dev"),
            ("gitea-labels", ".gitea/labels.toml", "paddock-dev"),
        ];
        for (name, file, reader) in cases {
            let all = inputs();
            let by = |r: &Reached| {
                r.nodes
                    .get(reader)
                    .is_some_and(|n| n.why.iter().any(|y| y.starts_with(&format!("input {name} "))))
            };
            assert!(by(&run(&[file], &all, &none)), "{name}: {file} must reach {reader}");
            let mut without = inputs();
            without.inputs.retain(|i| i.name != name);
            assert!(!by(&run(&[file], &without, &none)), "{name}: the control still reaches {reader}");
        }
    }
}

/// The structural rule (ADR 0117 §4, §14; the coordinator's ruling on the review of 59e5f7ec3): every extractor reports a reference
/// it cannot resolve, and the one rule in `reach` runs the referrer's projects in full whatever the change. Each extractor is fed
/// an unresolvable reference; the plan for an unrelated change widens to the referrer's project, and the resolved control does not.
#[test]
fn every_extractor_reports_an_unresolvable_reference_and_the_plan_widens_to_its_referrer() {
    use paddock_repo_graph::model::extract::{Extractor, MemoryRead, build};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inputs = crate::repo_inputs::load(&root).unwrap();
    let projects: Vec<(String, String)> = [("conformance", "conformance"), ("tools", "tools"), ("app:dyno", "apps/dyno")]
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .to_vec();
    let plan_for = |m: &paddock_repo_graph::model::nodes::Model| {
        let (cargo, npm) = (Graph::new(), Graph::new());
        let facts = Facts {
            cargo: &cargo,
            npm: &npm,
            projects: &projects,
            files: &[],
            detect: None,
            model: Some(m),
            seeds: &[],
        };
        reach(&["tools/notes.txt".to_string()], &facts, &inputs, &|_| (None, None))
    };
    let ci_with = |script: &str| {
        let r = MemoryRead::new()
            .with(".gitea/workflows/ci.yml", &format!("on: [push]\njobs:\n  app:\n    if: ${{{{ needs.plan.outputs.any_app == 'true' }}}}\n    steps:\n      - run: bash {script}\n"))
            .with("conformance/ci/kit.sh", "echo kit\n");
        let a = ci::CiExtractor {
            apps: ["dyno".to_string()].into_iter().collect(),
            projects: ["conformance".to_string(), "tools".to_string()].into_iter().collect(),
            decl: ci::CiDecl::default(),
        };
        let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(a)];
        build(&r.paths(), &r, &adapters).model
    };
    let code_with = |import: &str| {
        let r = MemoryRead::new()
            .with("conformance/bin/cli.mjs", &format!("import x from '{import}';\n"))
            .with("conformance/src/x.mjs", "export default 1;\n");
        let a = code::CodeExtractor {
            root: root.clone(),
            cargo: Graph::new(),
            npm: Graph::new(),
            scripts: ["conformance/bin/cli.mjs".to_string()].into_iter().collect(),
            tests: vec![],
        };
        let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(a)];
        build(&r.paths(), &r, &adapters).model
    };
    // ci: a step runs a script that is not tracked.
    let r = plan_for(&ci_with("conformance/ci/deleted.sh"));
    assert!(
        r.nodes.get("app:dyno").is_some_and(|n| n.kinds.contains(&Reach::Code)),
        "{:?}",
        r.nodes
    );
    assert!(
        !plan_for(&ci_with("conformance/ci/kit.sh")).nodes.contains_key("app:dyno"),
        "control: resolved"
    );
    // code: a script imports a module that is not tracked.
    let r = plan_for(&code_with("../src/gone.mjs"));
    assert!(
        r.nodes.get("conformance").is_some_and(|n| n.kinds.contains(&Reach::Code)),
        "{:?}",
        r.nodes
    );
    assert!(
        !plan_for(&code_with("../src/x.mjs")).nodes.contains_key("conformance"),
        "control: resolved"
    );
}

/// Every declared smoke filterset names packages of its project's own Cargo workspace, and every declared smoke phase is a phase
/// of its contract (a smoke set that names nothing would run nothing, ER-33). The control: a made-up package is found.
#[test]
fn every_declared_smoke_set_names_real_packages_and_phases() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let history = paddock_repo_graph::GitHistory::new(&root, std::time::Duration::from_secs(120));
    let graph = crate::plan::read_graph(&root, &history).unwrap();
    let decls = decl::load(&root, &graph).unwrap();
    let packages = |id: &str| -> BTreeSet<String> {
        let dir = project_dir_of(&graph, id).unwrap_or_default();
        graph
            .cargo
            .iter()
            .filter(|(d, _)| d.starts_with(&format!("{dir}/")) || *d == dir)
            .map(|(_, n)| n.name.clone())
            .collect()
    };
    let missing = |id: &str, filter: &str| -> Vec<String> {
        let have = packages(id);
        filter
            .split("package(=")
            .skip(1)
            .filter_map(|r| r.split(')').next())
            .filter(|p| !have.contains(*p))
            .map(str::to_string)
            .collect()
    };
    let mut checked = 0;
    for (id, d) in &decls {
        let Some(s) = &d.smoke else { continue };
        assert!(
            missing(id, &s.nextest).is_empty(),
            "{id}: smoke names {:?}, not a package of its workspace",
            missing(id, &s.nextest)
        );
        if !s.phases.is_empty() {
            let contract = if let Some(app) = id.strip_prefix("app:") {
                graph.config(app).clone()
            } else {
                crate::ci_run::contract::load(&root, id).unwrap()
            };
            let phases = crate::ci_run::contract::phases(&contract);
            for p in &s.phases {
                assert!(
                    phases.contains(&p.as_str()),
                    "{id}: smoke phase {p} is not a phase of its contract ({phases:?})"
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 8, "only {checked} smoke sets checked");
    // Positive control: a package that does not exist is found.
    assert_eq!(missing("crates", "package(=paddock-no-such-crate)"), ["paddock-no-such-crate"]);
}
