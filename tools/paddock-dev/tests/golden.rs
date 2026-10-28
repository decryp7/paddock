//! The graph crate plus Paddock's rules reproduce `node scripts/affected.mjs --files <f> --explain` for sample change sets.
//! The old script is the oracle until R4 replaces it: both are run on this working tree.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{Request, plan_for};
use paddock_repo_graph::GitHistory;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn oracle(files: &[&str]) -> Value {
    let out = Command::new("node")
        .args(["scripts/affected.mjs", "--files", &files.join(","), "--explain"])
        .current_dir(repo_root())
        .output()
        .expect("node is required: the golden test runs scripts/affected.mjs");
    assert!(
        out.status.success(),
        "affected.mjs failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn ours(files: &[&str]) -> Value {
    let root = repo_root();
    let req = Request {
        files: files.iter().map(|s| s.to_string()).collect(),
        all: false,
        explain: true,
        root: None,
        base: None,
        head: "HEAD".into(),
    };
    plan_for(&root, &GitHistory::new(&root, Duration::from_secs(60)), &req).unwrap()
}

/// `why` lists reasons in the order the script visited its sets; the closure here is sorted, so compare them as sets.
fn canonical(mut v: Value) -> Value {
    if let Some(why) = v.get_mut("why").and_then(Value::as_object_mut) {
        for reasons in why.values_mut() {
            if let Some(list) = reasons.as_array_mut() {
                list.sort_by_key(|r| r.as_str().unwrap_or_default().to_string());
            }
        }
    }
    v
}

const SAMPLES: &[&[&str]] = &[
    &["crates/paddock-cli/src/lib.rs"],
    &["crates/paddock-proc/src/run.rs", "crates/paddock-slots/src/lib.rs"],
    &["crates/Cargo.toml"],
    &["ui/projects/paddock-ui/src/public-api.ts"],
    &["ui/package.json"],
    &["eslint/src/index.cjs"],
    &["apps/dyno/app/src-tauri/src/main.rs"],
    &["apps/debrief/app/src/app/app.component.ts"],
    &[".gitea/workflows/ci.yml"],
    &[".gitea/workflows/debrief.yml"],
    &["docs/adr/0022-rust-developer-and-ci-tool.md", "README.md"],
    &["docs/rules.md", "CODEOWNERS", "scripts/publish.mjs"],
    &["icons/tools/install.mjs"],
    &["conformance/src/index.mjs"],
    &["package-lock.json"],
    &["tools/paddock-dev/src/lib.rs", "tools/test/x.test.mjs"],
    &["totally-unknown-root-file"],
    &[".gitignore", "crates/paddock-cli/CHANGELOG.md"],
];

fn oracle_between(base: &str, head: &str) -> Value {
    let out = Command::new("node")
        .args(["scripts/affected.mjs", base, head, "--explain"])
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "affected.mjs failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn ours_between(base: &str, head: &str) -> Value {
    let root = repo_root();
    let req = Request {
        files: vec![],
        all: false,
        explain: true,
        root: None,
        base: Some(base.into()),
        head: head.into(),
    };
    plan_for(&root, &GitHistory::new(&root, Duration::from_secs(60)), &req).unwrap()
}

fn git_out(args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(repo_root()).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_range_of_commits_gives_the_same_plan_including_lockfile_changes() {
    let head = git_out(&["rev-parse", "HEAD"]);
    assert_eq!(
        canonical(ours_between(&head, &head)),
        canonical(oracle_between(&head, &head)),
        "an empty range"
    );
    let recent = git_out(&["rev-parse", "HEAD~25"]);
    assert_eq!(
        canonical(ours_between(&recent, "HEAD")),
        canonical(oracle_between(&recent, "HEAD")),
        "the last 25 commits"
    );
    let lock = git_out(&["log", "-1", "--format=%H", "--", "package-lock.json"]);
    assert!(!lock.is_empty(), "the repository has a lockfile history");
    let before = format!("{lock}~1");
    assert_eq!(
        canonical(ours_between(&before, &lock)),
        canonical(oracle_between(&before, &lock)),
        "a commit that changed the lock"
    );
}

#[test]
fn the_plan_matches_affected_mjs_on_sample_change_sets() {
    for files in SAMPLES {
        assert_eq!(canonical(ours(files)), canonical(oracle(files)), "different plan for {files:?}");
    }
}
