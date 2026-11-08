//! The graph crate plus Paddock's rules reproduce what `scripts/affected.mjs` answered.
//!
//! - `recorded_*`: cases in `tests/golden/cases.json` run on the fixture repository in `tests/golden/fixture` (built in a scratch
//!   folder with two commits, tagged `fixture-base` and `fixture-head`); the answers of the script were recorded once into `tests/golden/expected/`
//!   by `tests/golden/generate.mjs`. These need no node, so they outlive the script (ADR 0022 R4 deletes it).
//! - `live_*`: the script itself as the oracle on this working tree, while the script exists. The `.gitea/` and `conformance/ci`
//!   samples need a clean tree: the script narrows those only for files that differ from HEAD (it answers everything for a clean one). Skipped, with the reason printed,
//!   when node or the script is missing.
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
    &["diagrams/paddock.xml", "README.md"],
    &["docs/rules.md", "CODEOWNERS", "scripts/publish.mjs"],
    &["icons/tools/install.mjs"],
    &["conformance/src/index.mjs"],
    &["package-lock.json"],
    &["tools/paddock-dev/src/lib.rs", "tools/test/x.test.mjs"],
    &["totally-unknown-root-file"],
    &["scripts/affected.mjs", "scripts/affected.test.mjs"],
    &[".cargo/config.toml"],
    &["lint/clippy.toml"],
    &["rust-toolchain.toml", "tsconfig/base.json"],
    &["conformance/ci/build-folder.sh"],
    &["conformance/src/util/is-main.mjs"],
    &[".gitignore", "crates/paddock-cli/CHANGELOG.md"],
];

#[test]
fn live_plan_matches_affected_mjs_on_sample_change_sets() {
    if !repo_root().join("scripts/affected.mjs").exists() || Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: scripts/affected.mjs or node is not available (the recorded cases still run)");
        return;
    }
    for files in SAMPLES {
        assert_eq!(canonical(ours(files)), canonical(oracle(files)), "different plan for {files:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Recorded cases

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// A lockfile stored in the fixture as `package-lock.fixture.json`, so the repository's own "one lockfile" check ignores it.
fn placed_name(name: &str) -> &str {
    if name == "package-lock.fixture.json" {
        "package-lock.json"
    } else {
        name
    }
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(placed_name(&entry.file_name().to_string_lossy()));
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn files_below(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_below(&path, base, out);
        } else {
            out.push(path.strip_prefix(base).unwrap().to_path_buf());
        }
    }
}

pub(crate) fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// The fixture repository: the overlay over the fixture is commit `fixture-base`, the fixture alone (overlay-only files removed) is `fixture-head`.
pub(crate) fn fixture_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    copy_tree(&golden_dir().join("fixture"), &root);
    copy_tree(&golden_dir().join("base-overlay"), &root);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["tag", "fixture-base"]);
    copy_tree(&golden_dir().join("fixture"), &root);
    let mut overlay = Vec::new();
    files_below(&golden_dir().join("base-overlay"), &golden_dir().join("base-overlay"), &mut overlay);
    for rel in overlay.into_iter().filter(|r| !golden_dir().join("fixture").join(r).exists()) {
        std::fs::remove_file(root.join(rel)).unwrap(); // overlay-only files carry no renamed names
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "head"]);
    git(&root, &["tag", "fixture-head"]);
    (dir, root)
}

#[test]
fn recorded_plans_match_what_affected_mjs_answered_on_the_fixture_repository() {
    let (_keep, root) = fixture_repo();
    let cases: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(golden_dir().join("cases.json")).unwrap()).unwrap();
    assert!(cases.len() >= 20, "the recorded cases");
    let history = GitHistory::new(&root, Duration::from_secs(60));
    let mut failures = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let req = Request {
            files: case["files"]
                .as_array()
                .map(|f| f.iter().map(|x| x.as_str().unwrap().to_string()).collect())
                .unwrap_or_default(),
            all: case["all"].as_bool().unwrap_or(false),
            explain: true,
            root: None,
            base: case["base"].as_str().map(String::from),
            head: case["head"].as_str().unwrap_or("HEAD").to_string(),
        };
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(golden_dir().join("expected").join(format!("{name}.json"))).unwrap()).unwrap();
        let got = plan_for(&root, &history, &req).map_err(|e| e.to_string());
        let ok = match (&got, expected.get("error").is_some()) {
            (Err(_), true) => true,
            (Ok(plan), false) => canonical(plan.clone()) == canonical(expected.clone()),
            _ => false,
        };
        if !ok {
            failures.push(format!("{name}: expected {expected}, got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_recorded_cases_cover_what_the_review_asked_for() {
    let text = std::fs::read_to_string(golden_dir().join("cases.json")).unwrap();
    for needle in [
        "tsconfig",
        "angular-json",
        "frontend-dist",
        "app-cargo-lock",
        "deletion",
        "unknown-base",
        "everything",
    ] {
        assert!(text.contains(needle), "no recorded case for {needle}");
    }
}
