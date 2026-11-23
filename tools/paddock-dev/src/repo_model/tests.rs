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
    let c = compare(&plan(), &r, &graph());
    assert_eq!(c["same"], true, "{c}");
}

#[test]
fn compare_reports_a_rule_the_model_lacks_and_names_what_it_drops() {
    // Positive control (§8 "--compare"): take the app's edge out of the model's answer; --compare names the difference and the drop.
    let mut r = Reached::default();
    r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    let c = compare(&plan(), &r, &graph());
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
    assert_eq!(compare(&plan(), &r, &graph())["dropped"], json!(["app:dyno"]));
    let mut p = plan();
    p["kinds"]["dyno"] = json!(["all"]);
    let mut r2 = Reached::default();
    r2.nodes.insert("crates".into(), hit(Reach::Code, true, true));
    r2.nodes.insert("app:dyno".into(), hit(Reach::Code, false, true));
    let c = compare(&p, &r2, &graph());
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

mod dropped {
    use super::*;
    use paddock_repo_graph::model::nodes::{Model, ModelNode, NodeKind};

    fn world() -> (RepoGraph, Model) {
        let mut m = Model::new();
        m.add_node(ModelNode::new("nextest:paddock-dyno::it", NodeKind::TestTarget, &[]).with_home("apps/dyno/crates/agent"));
        m.add_node(ModelNode::new("vitest:apps/dyno/app", NodeKind::TestTarget, &[]).with_home("apps/dyno/app"));
        (graph(), m)
    }

    #[test]
    fn a_narrowing_names_the_targets_it_no_longer_runs() {
        // The rule tables run all of dyno; the model reaches only its UI half: the nextest target is dropped, the vitest one is not.
        let (g, m) = world();
        let mut p = plan();
        p["kinds"]["dyno"] = json!(["all"]);
        let mut r = Reached::default();
        r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
        r.nodes.insert("app:dyno".into(), hit(Reach::Code, false, true));
        let c = compare_targets(&p, &r, &g, &m);
        assert_eq!(c["droppedTargets"]["app:dyno"], json!(["nextest:paddock-dyno::it"]), "{c}");
    }

    #[test]
    fn a_project_without_a_known_target_names_itself_so_no_narrowing_is_silent() {
        let (g, _) = world();
        let mut r = Reached::default();
        r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
        let c = compare_targets(&plan(), &r, &g, &Model::new());
        assert_eq!(
            c["droppedTargets"]["app:dyno"],
            json!(["app:dyno: no test target known to the model"]),
            "{c}"
        );
    }

    #[test]
    fn control_a_widening_drops_nothing() {
        // The model runs more than the rule tables (the app's whole suite where they ran its Rust half): nothing is dropped.
        let (g, m) = world();
        let mut r = Reached::default();
        r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
        r.nodes.insert("app:dyno".into(), hit(Reach::Code, true, true));
        let c = compare_targets(&plan(), &r, &g, &m);
        assert_eq!(c["droppedTargets"], json!({}), "{c}");
        assert_eq!(c["dropped"], json!([]));
    }

    #[test]
    fn a_plumbing_reach_without_a_declared_smoke_set_runs_the_whole_suite_and_drops_nothing() {
        // ADR 0117 §5: never less than declared. The old compare called this `smoke` and a narrowing.
        let (g, m) = world();
        let mut r = Reached::default();
        r.nodes.insert("crates".into(), hit(Reach::Code, true, true));
        r.nodes.insert("app:dyno".into(), hit(Reach::Plumbing, true, true));
        let c = compare_targets(&plan(), &r, &g, &m);
        assert_eq!(c["dropped"], json!([]), "{c}");
        // control: the same reach with a smoke set declared is the smoke set, a narrowing that drops the targets
        let mut g2 = graph();
        g2.configs.insert("dyno".into(), json!({"smoke": {}}));
        let c2 = compare_targets(&plan(), &r, &g2, &m);
        assert_eq!(c2["dropped"], json!(["app:dyno"]), "{c2}");
    }
}

mod categories {
    use super::super::replay::{Category, diffs_of, is_test_file};
    use super::*;

    fn diff(project: &str, rules: &str, model: Option<&str>, why: &[&str]) -> Value {
        json!({"differences": [{"project": project, "rules": rules, "model": model, "whyRules": why, "whyModel": []}],
               "dropped": if model.is_none() || model != Some(rules) { json!([project]) } else { json!([]) },
               "droppedTargets": {}})
    }

    fn category(c: &Value, changed: &[&str]) -> Category {
        diffs_of(c, &changed.iter().map(|s| s.to_string()).collect::<Vec<_>>())[0].category
    }

    #[test]
    fn each_listed_reason_has_its_category_and_an_unlisted_one_stays_unlisted() {
        let c = diff("app:dyno", "all", None, &["conformance conformance/src/x.mjs"]);
        assert_eq!(category(&c, &["conformance/src/x.mjs"]), Category::ConformanceSplit);
        let c = diff("ci-image", "all", None, &["global .gitea/actions/README.md"]);
        assert_eq!(category(&c, &[".gitea/actions/README.md"]), Category::GiteaDocs);
        let c = diff("ci-image", "all", None, &["global .gitea/workflows/ci.yml"]);
        assert_eq!(category(&c, &[".gitea/workflows/ci.yml"]), Category::Workflow);
        let c = diff("crates", "all", None, &["global .gitattributes"]);
        assert_eq!(category(&c, &[".gitattributes"]), Category::AttributesG2);
        let c = diff("app:dyno", "all", None, &["unknown root file spikes/x/Cargo.lock"]);
        assert_eq!(category(&c, &["spikes/x/Cargo.lock"]), Category::RootFolderG3);
        let c = diff("app:dyno", "ui", None, &["npm apps/dyno/app"]);
        assert_eq!(category(&c, &["ui/e2e/data.spec.ts"]), Category::TestOnly);
        let c = diff("app:dyno", "all", Some("ui"), &["npm apps/dyno/src-tauri"]);
        assert_eq!(category(&c, &["ui/projects/paddock-ui/src/a.ts"]), Category::HalfSplit);
        // controls: the same shapes with a changed source file, or a reason nothing lists, are not explained
        let c = diff("app:dyno", "ui", None, &["npm apps/dyno/app"]);
        assert_eq!(
            category(&c, &["ui/projects/paddock-ui/src/a.ts"]),
            Category::Unlisted,
            "a source change reaches its dependents"
        );
        let c = diff("app:dyno", "all", None, &["tools/paddock-dev/src/plan.rs"]);
        assert_eq!(category(&c, &["tools/paddock-dev/src/plan.rs"]), Category::Unlisted);
        let c = diff("app:dyno", "all", None, &[]);
        assert_eq!(category(&c, &["x"]), Category::Unlisted, "no reason is no explanation");
    }

    #[test]
    fn a_release_reach_that_selects_installers_only_is_the_pinned_defect_and_a_widening_is_wider() {
        let c = diff("app:dyno", "all", Some("release"), &["conformance/ci/x.sh used by ci.yml, job app"]);
        assert_eq!(category(&c, &["conformance/ci/x.sh"]), Category::ReleaseInstallersOnly);
        let c = json!({"differences": [{"project": "ui", "rules": null, "model": "all", "whyRules": [], "whyModel": []}], "dropped": [], "droppedTargets": {}});
        assert_eq!(category(&c, &["ui/a.ts"]), Category::Wider);
    }

    #[test]
    fn test_files_are_specs_test_folders_and_stand_ins_and_not_sources() {
        for f in [
            "ui/e2e/data.spec.ts",
            "crates/paddock-testkit/tests/it/stand_in.rs",
            "conformance/test/c1.test.mjs",
            "apps/a/app/e2e/x.e2e.ts",
        ] {
            assert!(is_test_file(f), "{f}");
        }
        for f in [
            "crates/paddock-proc/src/lib.rs",
            "ui/projects/paddock-ui/src/public-api.ts",
            "conformance/src/a.mjs",
        ] {
            assert!(!is_test_file(f), "{f}");
        }
    }
}
