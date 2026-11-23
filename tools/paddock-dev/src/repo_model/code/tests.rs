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
