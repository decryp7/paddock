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
