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
        paths: crate::repo_inputs::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .unwrap()
            .paths,
        rev: None,
        spawned: Default::default(),
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

#[test]
fn git_grep_status_is_read_exactly_and_a_git_error_is_not_no_candidates() {
    // Failing first (P1-5): exit 128 with empty stdout was read as "no candidates" (the test was inverted).
    assert_eq!(grep_files(Some(1), "", "", false, None).unwrap(), Vec::<String>::new());
    assert_eq!(grep_files(Some(0), "a.rs\nb.rs\n", "", false, None).unwrap(), ["a.rs", "b.rs"]);
    assert!(
        grep_files(Some(128), "", "fatal: not a git repository", false, None)
            .unwrap_err()
            .contains("fatal")
    );
    assert!(
        grep_files(Some(1), "x.rs\n", "", false, None).is_err(),
        "exit 1 with output is not a clean answer"
    );
    assert!(grep_files(None, "", "", false, None).is_err(), "a killed git is an error");
    assert!(
        grep_files(Some(0), "a.rs\n", "", true, None).is_err(),
        "output cut at the cap is an error"
    );
    // With a revision git prefixes each name; a name without the prefix is an error.
    assert_eq!(grep_files(Some(0), "HEAD:a.rs\n", "", false, Some("HEAD")).unwrap(), ["a.rs"]);
    assert!(grep_files(Some(0), "a.rs\n", "", false, Some("HEAD")).is_err());
}

#[test]
fn the_adapter_fails_when_git_grep_cannot_run() {
    // The same through the adapter: a folder that is no repository makes git exit 128, and the extraction is an error.
    let d = tempfile::tempdir().unwrap();
    let adapter = CodeExtractor {
        root: d.path().to_path_buf(),
        cargo: Graph::new(),
        npm: Graph::new(),
        scripts: BTreeSet::new(),
        paths: crate::repo_inputs::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .unwrap()
            .paths,
        rev: None,
        spawned: Default::default(),
    };
    let e = adapter
        .extract(&[], &paddock_repo_graph::model::extract::MemoryRead::new())
        .unwrap_err();
    assert_eq!((e.extractor, e.file.as_str()), ("code", "git grep"), "{e}");
    assert_eq!(adapter.spawned.get(), 1);
}

#[test]
fn a_candidate_that_cannot_be_read_fails_the_adapter() {
    // Failing first: an unreadable `.rs` candidate (and an unreadable imported script) was skipped with `continue`.
    use paddock_repo_graph::model::extract::MemoryRead;
    let d = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let mut c = crate::git_env::git();
        c.arg("-C")
            .arg(d.path())
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args);
        assert!(c.status().unwrap().success(), "{args:?}");
    };
    run(&["init", "-q"]);
    std::fs::write(d.path().join("a.rs"), "const W: &str = include_str!(\"w.txt\");\n").unwrap();
    run(&["add", "a.rs"]);
    run(&["commit", "-q", "-m", "x"]);
    let adapter = |scripts: &[&str]| CodeExtractor {
        root: d.path().to_path_buf(),
        cargo: Graph::new(),
        npm: Graph::new(),
        scripts: scripts.iter().map(|s| s.to_string()).collect(),
        paths: crate::repo_inputs::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .unwrap()
            .paths,
        rev: None,
        spawned: Default::default(),
    };
    let files = vec!["a.rs".to_string(), "x/cli.mjs".to_string()];
    let bad = MemoryRead::new().failing("a.rs", "I/O error").with("x/cli.mjs", "");
    let e = adapter(&[]).extract(&files, &bad).unwrap_err();
    assert_eq!((e.extractor, e.file.as_str(), e.reason.as_str()), ("code", "a.rs", "I/O error"));
    let bad_script = MemoryRead::new().with("a.rs", "").failing("x/cli.mjs", "not UTF-8");
    assert_eq!(adapter(&["x/cli.mjs"]).extract(&files, &bad_script).unwrap_err().file, "x/cli.mjs");
    // Control: the same files readable extract.
    let good = MemoryRead::new().with("a.rs", "").with("x/cli.mjs", "");
    assert!(adapter(&["x/cli.mjs"]).extract(&files, &good).is_ok());
    // Control: an absent script is skipped, not an error.
    assert!(adapter(&["x/cli.mjs"]).extract(&files, &MemoryRead::new().with("a.rs", "")).is_ok());
}
