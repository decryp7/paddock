//! The code adapter with its positive controls (ADR 0117 §8).
#![allow(clippy::unwrap_used)]

use super::*;

fn tracked(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn read_paths_finds_include_and_manifest_dir_reads_and_the_control_finds_none_without_them() {
    let t = tracked(&[
        "crates/a/tests/recorded/x.txt",
        "crates/a/src/words.txt",
        "crates/b/tests/fixtures/f.evtx",
        "tools/paddock-dev/tests/ci-runs/timings/a.json",
    ]);
    let src = r#"
const WORDS: &str = include_str!("words.txt");
fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/recorded")
}
const F: &[u8] = include_bytes!("../../b/tests/fixtures/f.evtx");
// a comment naming "tests/recorded" is no read
let label = "tests/recorded";
"#;
    let got = read_paths("crates/a/src/lib.rs", src, "crates/a", &t);
    assert_eq!(
        got,
        tracked(&[
            "crates/a/src/words.txt",
            "crates/a/tests/recorded",
            "crates/b/tests/fixtures/f.evtx"
        ])
    );
    let concat = r#"const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ci-runs/timings");"#;
    assert_eq!(
        read_paths("tools/paddock-dev/src/ci_runs/timings.rs", concat, "tools/paddock-dev", &t),
        tracked(&["tools/paddock-dev/tests/ci-runs/timings"])
    );
    // Control: the same literal outside an include or a manifest-dir statement reads nothing.
    assert!(read_paths("crates/a/src/lib.rs", "let x = \"tests/recorded\";\n", "crates/a", &t).is_empty());
}

#[test]
fn literals_split_quoted_strings() {
    assert_eq!(literals(r#"a("x", "y\"z")"#), ["x", "y\"z"]);
}

/// The ui project's declared tests: Vitest specs under src, Playwright specs under e2e, helpers under e2e.
fn ui_decl() -> TestDecl {
    TestDecl {
        project: "ui".into(),
        dir: "ui".into(),
        runners: [
            ("vitest".to_string(), vec!["ui/src/**/*.spec.ts".to_string()]),
            ("playwright".to_string(), vec!["ui/e2e/**/*.spec.ts".to_string()]),
        ]
        .into_iter()
        .collect(),
        helpers: vec!["ui/e2e/**".to_string()],
    }
}

fn code_model(cli: &str) -> paddock_repo_graph::model::nodes::Model {
    use paddock_repo_graph::model::extract::{MemoryRead, build};
    let m = MemoryRead::new()
        .with("x/bin/cli.mjs", cli)
        .with("x/src/run.mjs", "import { a } from './util/a.mjs';\n")
        .with("x/src/util/a.mjs", "export const a = 1;\n")
        .with("ui/package.json", "{}")
        .with("ui/src/a.spec.ts", "")
        .with("ui/e2e/b.spec.ts", "");
    let mut npm = Graph::new();
    npm.insert(
        "ui",
        paddock_repo_graph::Node {
            name: "ui".into(),
            workspace: String::new(),
            deps: vec![],
        },
    );
    let adapter = CodeExtractor {
        root: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        cargo: Graph::new(),
        npm,
        scripts: ["x/bin/cli.mjs".to_string()].into_iter().collect(),
        tests: vec![ui_decl()],
    };
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(adapter)];
    let b = build(&m.paths(), &m, &adapters);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    b.model
}

#[test]
fn imports_follow_a_scripts_modules_transitively_and_the_control_drops_them() {
    let m = code_model("import { run } from '../src/run.mjs';\n");
    assert!(
        m.targets("script:x/bin/cli.mjs", EdgeKind::Imports)
            .contains(&"script:x/src/run.mjs")
    );
    assert!(
        m.targets("script:x/src/run.mjs", EdgeKind::Imports)
            .contains(&"script:x/src/util/a.mjs")
    );
    let (hits, _) = m.reach(&["x/src/util/a.mjs".to_string()]);
    assert!(hits.contains_key("script:x/bin/cli.mjs"), "{hits:?}");
    // Positive control: without the import line the module is no script node and reaches nothing.
    let c = code_model("console.log('no import');\n");
    assert!(c.node("script:x/src/run.mjs").is_none());
}

#[test]
fn specs_are_test_files_of_their_packages_vitest_or_playwright_target() {
    let m = code_model("");
    assert_eq!(m.targets("vitest:ui", EdgeKind::Tests), ["test:ui/src/a.spec.ts"]);
    assert_eq!(m.targets("playwright:ui", EdgeKind::Tests), ["test:ui/e2e/b.spec.ts"]);
    assert_eq!(m.node("test:ui/src/a.spec.ts").map(|n| n.kind), Some(NodeKind::TestFile));
}

/// A code model over `m` with the given crates and test declarations (no CI scripts).
fn model_of(
    m: &paddock_repo_graph::model::extract::MemoryRead,
    cargo: Graph,
    npm: Graph,
    tests: Vec<TestDecl>,
) -> paddock_repo_graph::model::nodes::Model {
    use paddock_repo_graph::model::extract::build;
    let adapter = CodeExtractor {
        root: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        cargo,
        npm,
        scripts: BTreeSet::new(),
        tests,
    };
    let adapters: Vec<Box<dyn Extractor>> = vec![Box::new(adapter)];
    let b = build(&m.paths(), m, &adapters);
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    b.model
}

fn pkg(name: &str) -> paddock_repo_graph::Node {
    paddock_repo_graph::Node {
        name: name.into(),
        workspace: String::new(),
        deps: vec![],
    }
}

fn e2e_decl(runner: &str) -> TestDecl {
    TestDecl {
        project: "ui".into(),
        dir: "ui".into(),
        runners: [(runner.to_string(), vec!["ui/projects/e2e/src/**/*.spec.ts".to_string()])]
            .into_iter()
            .collect(),
        helpers: vec!["ui/projects/e2e/src/support/**".to_string()],
    }
}

#[test]
fn p1_2_the_runner_comes_from_the_contract_not_the_path() {
    // paddock-e2e's selftest specs sit under no e2e/ folder and are Playwright specs.
    use paddock_repo_graph::model::extract::MemoryRead;
    let m = MemoryRead::new()
        .with("ui/projects/e2e/src/selftest.spec.ts", "")
        .with("ui/package.json", "{}");
    let mut npm = Graph::new();
    npm.insert("ui", pkg("ui"));
    let got = model_of(&m, Graph::new(), npm.clone(), vec![e2e_decl("playwright")]);
    assert_eq!(
        got.targets("playwright:ui", EdgeKind::Tests),
        ["test:ui/projects/e2e/src/selftest.spec.ts"]
    );
    // Control: undeclared, the same file is no test file (its change runs the project in full).
    let none = model_of(&m, Graph::new(), npm, vec![]);
    assert!(none.node("test:ui/projects/e2e/src/selftest.spec.ts").is_none());
}

#[test]
fn p1_2_a_spec_another_spec_imports_and_a_helper_reach_the_importing_spec() {
    use paddock_repo_graph::model::extract::MemoryRead;
    let m = MemoryRead::new()
        .with("ui/package.json", "{}")
        .with(
            "ui/projects/e2e/src/a.spec.ts",
            "import { shared } from './b.spec';\nimport { h } from './support/h';\n",
        )
        .with("ui/projects/e2e/src/b.spec.ts", "export const shared = 1;\n")
        .with("ui/projects/e2e/src/support/h.ts", "export const h = 1;\n");
    let mut npm = Graph::new();
    npm.insert("ui", pkg("ui"));
    let got = model_of(&m, Graph::new(), npm.clone(), vec![e2e_decl("playwright")]);
    assert!(
        got.targets("test:ui/projects/e2e/src/a.spec.ts", EdgeKind::Imports)
            .contains(&"test:ui/projects/e2e/src/b.spec.ts")
    );
    let (hits, _) = got.reach(&["ui/projects/e2e/src/support/h.ts".to_string()]);
    assert!(hits.contains_key("test:ui/projects/e2e/src/a.spec.ts"), "{hits:?}");
    // Control: without the import line the helper is no test file.
    let m2 = MemoryRead::new()
        .with("ui/package.json", "{}")
        .with("ui/projects/e2e/src/a.spec.ts", "const x = 1;\n")
        .with("ui/projects/e2e/src/support/h.ts", "export const h = 1;\n");
    assert!(
        model_of(&m2, Graph::new(), npm, vec![e2e_decl("playwright")])
            .node("test:ui/projects/e2e/src/support/h.ts")
            .is_none()
    );
}

#[test]
fn an_unresolved_import_of_a_test_file_is_an_unresolved_reference_and_its_helpers_stay_code() {
    use paddock_repo_graph::model::extract::MemoryRead;
    let m = MemoryRead::new()
        .with("ui/package.json", "{}")
        .with(
            "ui/projects/e2e/src/a.spec.ts",
            "import { h } from './support/h';\nimport { g } from './gone';\n",
        )
        .with("ui/projects/e2e/src/support/h.ts", "export const h = 1;\n");
    let mut npm = Graph::new();
    npm.insert("ui", pkg("ui"));
    let got = model_of(&m, Graph::new(), npm, vec![e2e_decl("playwright")]);
    assert!(
        got.unresolved()
            .iter()
            .any(|u| u.referrer == "ui/projects/e2e/src/a.spec.ts" && u.reference == "./gone")
    );
    assert!(
        got.node("test:ui/projects/e2e/src/support/h.ts").is_none(),
        "an unprovable project's helpers stay code"
    );
}

#[test]
fn p1_3_a_read_outside_the_readers_crate_is_a_data_node_owning_exactly_that_path_and_a_missing_include_is_unresolved() {
    use paddock_repo_graph::model::extract::MemoryRead;
    let mut cargo = Graph::new();
    let c = |n: &str| paddock_repo_graph::Node {
        name: n.into(),
        workspace: "apps/debrief".into(),
        deps: vec![],
    };
    cargo.insert("apps/debrief/crates/debrief-plugins", c("debrief-plugins"));
    cargo.insert("apps/debrief/crates/debrief-plugin-windows", c("debrief-plugin-windows"));
    let reader = "#[path = \"../../../debrief-plugin-windows/tests/it/support/synth_minidump.rs\"]\nmod synth;\n\
                  const T: &str = include_str!(\"../../../../../../ui/projects/paddock-ui/scss/_tokens.scss\");\n\
                  const G: &str = include_str!(\"gone.txt\");\n";
    let m = MemoryRead::new()
        .with("apps/debrief/crates/debrief-plugins/tests/it/main.rs", reader)
        .with(
            "apps/debrief/crates/debrief-plugin-windows/tests/it/support/synth_minidump.rs",
            "pub fn x() {}\n",
        )
        .with("ui/projects/paddock-ui/scss/_tokens.scss", ":root {}\n");
    // The git grep runs over the real tree, so the reader here is found through the fallback of a tracked candidate: give the
    // adapter the candidate list directly.
    let adapter = CodeExtractor {
        root: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        cargo,
        npm: Graph::new(),
        scripts: BTreeSet::new(),
        tests: vec![],
    };
    let files: BTreeSet<String> = m.paths().into_iter().collect();
    let mut x = Extracted::default();
    let mut stats = CodeStats::default();
    let cand = vec!["apps/debrief/crates/debrief-plugins/tests/it/main.rs".to_string()];
    let texts = paddock_repo_graph::model::extract::BulkRead::read_all(&m, &cand);
    adapter.reads_of(&cand, texts, &files, &mut x, &mut stats, true);
    let ids: BTreeSet<&str> = x.nodes.iter().map(|n| n.id.as_str()).collect();
    assert!(
        ids.contains("data:apps/debrief/crates/debrief-plugin-windows/tests/it/support/synth_minidump.rs"),
        "{ids:?}"
    );
    assert!(ids.contains("data:ui/projects/paddock-ui/scss/_tokens.scss"), "{ids:?}");
    assert!(
        x.edges
            .iter()
            .all(|e| e.from == "crate:apps/debrief/crates/debrief-plugins" && e.kind == EdgeKind::Reads)
    );
    assert_eq!(x.unresolved.len(), 1, "{:?}", x.unresolved);
    assert!(x.unresolved[0].reference.ends_with("gone.txt"));
    // Control: a reader with no read emits nothing.
    let mut y = Extracted::default();
    adapter.reads_of(&cand, vec![Some("fn main() {}\n".into())], &files, &mut y, &mut stats, true);
    assert!(y.nodes.is_empty() && y.edges.is_empty() && y.unresolved.is_empty());
}

/// The measurement of §10 (`cargo nextest run --run-ignored only -E 'test(code_adapter_cost)' --no-capture`): the code adapter over
/// the real tree, with the `syn` fallbacks counted beside the reads and the files parsed for imports.
#[test]
#[ignore = "a measurement, run by hand (rule 48)"]
fn code_adapter_cost_on_the_real_tree() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let history = paddock_repo_graph::GitHistory::new(&root, std::time::Duration::from_secs(120));
    let graph = crate::plan::read_graph(&root, &history).unwrap();
    let files = paddock_repo_graph::History::tracked_files(&history).unwrap();
    let decls = crate::repo_model::decl::load(&root, &graph).unwrap();
    let read = crate::repo_model::GitBulk::new(&root, None);
    let a = CodeExtractor {
        root: root.clone(),
        cargo: graph.cargo.clone(),
        npm: graph.npm.clone(),
        scripts: files
            .iter()
            .filter(|f| f.starts_with("conformance/bin/") || f.starts_with("scripts/"))
            .cloned()
            .collect(),
        tests: decls.values().map(|d| d.tests.clone()).collect(),
    };
    let t = std::time::Instant::now();
    let (x, stats) = a.extract_with_stats(&files, &read).unwrap();
    eprintln!(
        "code adapter: {:?}; nodes {}, edges {}, unresolved {}; rust sources {}, syn fallbacks {}, parsed {}, unprovable {:?}",
        t.elapsed(),
        x.nodes.len(),
        x.edges.len(),
        x.unresolved.len(),
        stats.rust_sources,
        stats.syn_fallbacks,
        stats.parsed,
        stats.unprovable
    );
    for u in &x.unresolved {
        eprintln!("unresolved: {} in {} ({})", u.reference, u.referrer, u.why);
    }
    let t = std::time::Instant::now();
    let tracked: BTreeSet<String> = files.iter().cloned().collect();
    let cand: Vec<String> = a
        .read_candidates(None)
        .unwrap()
        .into_iter()
        .filter(|f| tracked.contains(f))
        .collect();
    let texts = paddock_repo_graph::model::extract::BulkRead::read_all(&read, &cand);
    let mut scan = 0;
    for (f, t) in cand.iter().zip(&texts) {
        if let (Some(t), Some(c)) = (t, graph.cargo.node_of(f)) {
            scan += read_paths(f, t, c, &tracked).len();
        }
    }
    eprintln!("text scan: {:?}; {} reads over {} candidates", t.elapsed(), scan, cand.len());
}
