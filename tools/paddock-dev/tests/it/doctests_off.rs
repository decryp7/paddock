//! Doctests are off (user 2026-11-24; crates/README.md, "Doctests"): every library target says `doctest = false`, read through
//! `cargo metadata` (the planner's recorded answers, ER-42). A member audit (tools/paddock-dev/.config/nextest.toml, `member-audits`), so
//! a new crate without it is found on the merged tree whatever the member changed. The runnable doc examples are checked on every push by
//! `contracts::doc_examples` (the conformance tests).

use paddock_dev::plan::read_graph_unplaced;
use paddock_repo_graph::{GitHistory, parse_targets};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Folders whose Cargo workspaces and sources are fixtures of tests, not the family's code.
fn fixture(path: &str) -> bool {
    path.starts_with("tools/paddock-dev/tests/") || path.contains("/fixtures/")
}

/// The `package/target` of every target `cargo metadata` says runs doctests, in one workspace's answer.
fn doctested(json: &str, root: &str) -> Vec<String> {
    parse_targets(json, root)
        .unwrap()
        .into_iter()
        .filter(|t| t.doctest && !fixture(&t.dir))
        .map(|t| format!("{} ({}/{})", t.dir, t.package, t.name))
        .collect()
}

#[test]
fn graph_every_library_target_has_doctests_off() {
    let root = root();
    let history = GitHistory::new(&root, Duration::from_secs(120));
    let graph = read_graph_unplaced(&root, &history).unwrap();
    let abs = std::fs::canonicalize(&root).unwrap().to_string_lossy().replace('\\', "/");
    assert!(
        graph.metadata.len() >= 5,
        "the planner read too few workspaces: {:?}",
        graph.metadata.keys()
    );
    let on: Vec<String> = graph.metadata.values().flat_map(|json| doctested(json, &abs)).collect();
    assert!(
        on.is_empty(),
        "these library targets run doctests; add `[lib] doctest = false` to their Cargo.toml (crates/README.md, \"Doctests\"):\n{}",
        on.join("\n")
    );
}

/// Positive control: a target `cargo metadata` lists with `"doctest": true` is found, and one with `false` is not.
#[test]
fn a_target_with_doctests_on_is_found() {
    let json = |on: bool| {
        format!(
            r#"{{"packages": [{{"name": "p", "manifest_path": "/r/crates/p/Cargo.toml",
                "targets": [{{"name": "p", "kind": ["lib"], "src_path": "/r/crates/p/src/lib.rs", "doctest": {on}}}]}}]}}"#
        )
    };
    assert_eq!(doctested(&json(true), "/r"), ["crates/p (p/p)"]);
    assert!(doctested(&json(false), "/r").is_empty());
}
