//! C123 (a Rust test starts ./dev, cargo or node only through TestCommand): the Rust host against the answers the JavaScript check gave
//! (recorded, `tests/fixtures/check/recorded`; the JS module is retired, pair P3) on a tree of every shape the check knows and on this tree.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::check::{RunOptions, registry, run_checks};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn full(root: &Path) -> Value {
    let opts = RunOptions {
        root: root.to_path_buf(),
        config: paddock_dev::check::config::Config::bare("paddock"),
        scope: None,
        only: Some(vec!["C123".into()]),
        files: None,
        today: paddock_dev::check::today_utc(),
        base_ref: None,
        ci: Some(false),
    };
    run_checks(&opts, &registry()).unwrap().json()
}

fn rust(root: &Path) -> Value {
    let r = full(root);
    json!({"findings": r["findings"], "errors": r["errors"], "warnings": r["warnings"]})
}

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (p, t) in files {
        let path = d.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, t).unwrap();
    }
    d
}

#[test]
fn every_shape_gives_the_same_findings_in_both_hosts() {
    let d = tree(&[
        (
            "crates/a/tests/it/bad.rs",
            "fn t() {\n    Command::new(\"node\").output();\n    paddock_proc::command(\n        env!(\"CARGO\"),\n    );\n    quiet_command(std::env::var(\"CARGO\").unwrap());\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    run(bin);\n}\n",
        ),
        (
            "crates/a/tests/it/good.rs",
            "fn t() {\n    TestCommand::new(\"node\").output();\n    Command::new(\"git\").output();\n    // Command::new(\"cargo\")\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    TestCommand::new(\"sh\").arg(bin);\n}\n",
        ),
        (
            "crates/a/src/lib.rs",
            "fn run() {\n    Command::new(\"cargo\");\n}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        Command::new(\"cargo\");\n    }\n}\n",
        ),
        (
            "crates/a/tests/it/bound.rs",
            "const BIN: &str = env!(\"CARGO_BIN_EXE_heavy\");\nfn t() {\n    Command::new(env!(\"CARGO_BIN_EXE_other\")).output();\n    let mut c = command(BIN);\n    let p = std::env::var(\"PATH\").unwrap();\n    TestCommand::new(\"x\").env(\"PATH\", p);\n    let s = SpawnSpec::new(env!(\"CARGO_BIN_EXE_child\"), \"x\");\n}\n",
        ),
        ("crates/a/src/plain.rs", "fn run() {\n    Command::new(\"cargo\");\n}\n"),
        (
            "crates/b/tests/it/common/mod.rs",
            "pub fn child_program() -> PathBuf {\n    PathBuf::from(env!(\"CARGO_BIN_EXE_x\"))\n}\n",
        ),
        (
            "crates/b/tests/it/gaps.rs",
            "fn t() {\n    paddock_proc::command(child_program());\n    Command::new(\"rustc\");\n    Command::new(\"npx.cmd\");\n    Command::new(env!(\"RUSTC\"));\n    Command::new(root.join(\"dev\"));\n    // TestCommand only in a comment\n    let bin = env!(\"CARGO_BIN_EXE_paddock-dev\");\n    run(bin);\n}\n",
        ),
        (
            "crates/b/src/lib.rs",
            "#[cfg(not(test))]\nfn run() {\n    Command::new(\"cargo\");\n}\n#[cfg(all(test, unix))]\nmod tests {\n    fn t() {\n        Command::new(\"node\");\n    }\n}\n",
        ),
        (
            "crates/c/tests/it/other.rs",
            "fn t() {\n    paddock_proc::command(child_program());\n}\n",
        ),
        (
            "crates/a/tests/it/waived.rs",
            "fn t() {\n    // conformance: test-env \u{2014} a probe of the shell\n    Command::new(\"cargo\");\n}\nfn u() {\n    // conformance: test-env\n    Command::new(\"cargo\");\n}\n",
        ),
    ]);
    let b = rust(d.path());
    let a = crate::recorded::expect("c123-every-shape", &b);
    assert_eq!(a, b);
    assert_eq!(
        a["errors"], 16,
        "bad.rs 4, bound.rs 3, lib.rs 1, the bare waiver 1, gaps.rs 6, b's lib.rs 1 (c's other.rs 0: another crate): {a}"
    );
}

#[test]
fn a_tree_without_rust_tests_is_check_broken_in_both_never_a_pass() {
    let d = tree(&[("README.md", "x")]);
    let b = rust(d.path());
    let a = crate::recorded::expect("c123-no-rust-tests", &b);
    assert!(a["errors"].as_u64().unwrap() >= 1 && b["errors"].as_u64().unwrap() >= 1, "{a} {b}");
}

#[test]
fn tree_this_working_tree_run_is_sound() {
    let root = repo();
    crate::recorded::assert_live_tree_sound("c123", &full(&root));
}
