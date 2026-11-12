//! `dev-tool.yml` (ADR 0022 R0b) publishes the tool for every kind of runner CI uses, and its push filter covers the tool's inputs:
//! a runner kind added to a CI workflow without a published binary would build the tool on every run, silently.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::input_hash::parse_inputs;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const LABELS: [&str; 3] = ["ubuntu-latest", "windows-latest", "macos-latest"];

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

/// The runner labels a workflow names on `runs-on:` and `os:` lines (comments are skipped).
fn runner_labels(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        if line.contains("runs-on:") || line.starts_with("os:") || line.starts_with("- os:") {
            let code = line.split('#').next().unwrap();
            found.extend(LABELS.iter().filter(|l| code.contains(*l)).map(|l| l.to_string()));
        }
    }
    found
}

#[test]
fn the_publish_workflow_covers_every_runner_kind_ci_runs_on() {
    let dir = repo_root().join(".gitea/workflows");
    let publish = std::fs::read_to_string(dir.join("dev-tool.yml")).unwrap();
    let published = runner_labels(&publish);
    let mut used = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "yml") && p.file_name().unwrap() != "dev-tool.yml" {
            used.extend(runner_labels(&std::fs::read_to_string(&p).unwrap()));
        }
    }
    assert!(!used.is_empty(), "no runner labels found in .gitea/workflows: the scan is broken");
    assert_eq!(
        published, used,
        "dev-tool.yml must publish for exactly the runner kinds the other workflows use"
    );
}

#[test]
fn the_publish_workflow_runs_when_an_input_of_the_tool_changes() {
    let root = repo_root();
    let publish = std::fs::read_to_string(root.join(".gitea/workflows/dev-tool.yml")).unwrap();
    let paths: Vec<String> = publish
        .lines()
        .skip_while(|l| l.trim() != "paths:")
        .skip(1)
        .map_while(|l| l.trim().strip_prefix("- ").map(|p| p.trim().to_string()))
        .collect();
    assert!(!paths.is_empty(), "no push paths found in dev-tool.yml");
    let inputs = std::fs::read_to_string(root.join("tools/paddock-dev/inputs.txt")).unwrap();
    for input in parse_inputs(&inputs).unwrap() {
        let covered = paths.iter().any(|p| {
            let base = p.trim_end_matches("/**");
            input == base || input.starts_with(&format!("{base}/"))
        });
        assert!(
            covered,
            "{input} is an input of the tool but not a push path of dev-tool.yml ({paths:?})"
        );
    }
    assert!(
        publish.contains("workflow_dispatch:") && publish.contains("schedule:"),
        "manual and nightly runs are part of the contract"
    );
}
