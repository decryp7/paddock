//! The graph crate plus Paddock's rules reproduce what `scripts/affected.mjs` answered.
//!
//! - `recorded_*`: cases in `tests/golden/cases.json` run on the fixture repository in `tests/golden/fixture` (built in a scratch
//!   folder with two commits, tagged `fixture-base` and `fixture-head`); the answers of the script were recorded once into `tests/golden/expected/`
//!   by `tests/golden/generate.mjs`. These need no node, so they outlive the script (ADR 0022 R4 deletes it).
//! - `recorded_command_line_*`: the same for the command line (`--ci`, `--commit-range`, the argument forms) in
//!   `tests/golden/ci-cases.json`, recorded into `tests/golden/expected/cli/`.
//! - `live_*`: the script itself as the oracle on this working tree, while the script exists. The `.gitea/` and `conformance/ci`
//!   samples need a clean tree: the script narrows those only for files that differ from HEAD (it answers everything for a clean one). Skipped, with the reason printed,
//!   when node or the script is missing.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{Request, plan_for};
use paddock_repo_graph::GitHistory;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// A live comparison that cannot run: a note on a workstation, a failure in CI, where a missing oracle or a shallow clone must not
/// let the comparison pass unseen.
fn skip(reason: &str) {
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "true"));
    assert!(!ci, "the live comparison cannot run in CI: {reason}");
    eprintln!("skipped: {reason}");
}

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
        skip("scripts/affected.mjs or node is not available (the recorded cases still run)");
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

fn git(dir: &Path, args: &[&str]) {
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
fn fixture_repo() -> (tempfile::TempDir, PathBuf) {
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

// ---------------------------------------------------------------------------------------------------------------------
// The command line: `dev plan` against what `affected.mjs` answered (recorded) and against the script itself (live)

/// What a caller of the planner sees of one run, with commit ids masked (they differ per run). Mirrors `generate.mjs`.
fn run_cli(program: &mut Command, root: &Path, case: &Value) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let head = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "fixture-head"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    program.current_dir(root).env_clear();
    for k in ["PATH", "HOME"] {
        if let Ok(v) = std::env::var(k) {
            program.env(k, v);
        }
    }
    program
        .env("GITHUB_OUTPUT", dir.path().join("out"))
        .env("GITHUB_STEP_SUMMARY", dir.path().join("summary"));
    for (k, v) in case["env"].as_object().unwrap() {
        let v = v.as_str().unwrap();
        program.env(k, if v == "__HEAD__" { head.as_str() } else { v });
    }
    let args: Vec<&str> = case["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    let out = program.args(&args).output().unwrap();
    let mask = |t: String| -> String {
        let re = regex::Regex::new(r"\b[0-9a-f]{40}\b").unwrap();
        re.replace_all(&t, "<sha>").to_string()
    };
    let read = |f: &str| {
        std::fs::read_to_string(dir.path().join(f))
            .ok()
            .map(|t| Value::String(mask(t)))
            .unwrap_or(Value::Null)
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let logs_to_stdout = args.contains(&"--ci") && !args.contains(&"--app");
    let answer = if stdout.trim_start().starts_with('{') {
        canonical(serde_json::from_str(&stdout).unwrap())
    } else {
        Value::String(mask(stdout.trim().to_string()))
    };
    let first_error_line = String::from_utf8_lossy(&out.stderr)
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches("error: ")
        .to_string();
    json!({
        "code": out.status.code(),
        "stdout": if logs_to_stdout { Value::Null } else { answer },
        "outputs": read("out"),
        "summary": read("summary"),
        "error": if out.status.success() { Value::Null } else { Value::String(first_error_line) },
    })
}

fn cli_cases() -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(golden_dir().join("ci-cases.json")).unwrap()).unwrap()
}

/// The fixture repository with `origin/main` at `fixture-base`, as `generate.mjs` leaves it.
fn fixture_with_origin() -> (tempfile::TempDir, PathBuf) {
    let (keep, root) = fixture_repo();
    git(&root, &["update-ref", "refs/remotes/origin/main", "fixture-base"]);
    (keep, root)
}

#[test]
fn recorded_command_line_cases_match_what_affected_mjs_answered_on_the_fixture_repository() {
    let (_keep, root) = fixture_with_origin();
    let mut failures = Vec::new();
    let cases = cli_cases();
    assert!(cases.len() >= 25, "the recorded command line cases");
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(golden_dir().join("expected/cli").join(format!("{name}.json"))).unwrap())
                .unwrap();
        let mut program = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
        program.arg("plan");
        let got = run_cli(&mut program, &root, case);
        if got != expected {
            failures.push(format!("{name}:\n  expected {expected}\n  got      {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn live_command_line_matches_affected_mjs_on_a_copy_of_the_fixture_repository() {
    if !repo_root().join("scripts/affected.mjs").exists() || Command::new("node").arg("--version").output().is_err() {
        skip("scripts/affected.mjs or node is not available (the recorded cases still run)");
        return;
    }
    let (_keep, root) = fixture_with_origin();
    // The script finds its repository from its own location: an untracked copy of it sits in the fixture.
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("conformance/src/util")).unwrap();
    std::fs::copy(repo_root().join("scripts/affected.mjs"), root.join("scripts/affected.mjs")).unwrap();
    std::fs::copy(
        repo_root().join("conformance/src/util/is-main.mjs"),
        root.join("conformance/src/util/is-main.mjs"),
    )
    .unwrap();
    let mut failures = Vec::new();
    for case in &cli_cases() {
        let mut node = Command::new("node");
        node.arg("scripts/affected.mjs");
        let mut ours = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
        ours.arg("plan");
        let (a, b) = (run_cli(&mut node, &root, case), run_cli(&mut ours, &root, case));
        if a != b {
            failures.push(format!("{}:\n  script {a}\n  dev    {b}", case["name"]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Pairs of first-parent commits of this working tree: the range mode on real history, the script as the oracle.
#[test]
fn live_range_plans_match_affected_mjs_on_this_repositorys_first_parent_history() {
    if !repo_root().join("scripts/affected.mjs").exists() || Command::new("node").arg("--version").output().is_err() {
        skip("scripts/affected.mjs or node is not available");
        return;
    }
    let log = Command::new("git")
        .current_dir(repo_root())
        .args(["log", "--format=%H", "--first-parent", "-n", "40"])
        .output()
        .unwrap();
    let shas: Vec<String> = String::from_utf8_lossy(&log.stdout).lines().map(String::from).collect();
    if shas.len() < 12 {
        skip(&format!(
            "only {} first-parent commits are available (a shallow clone: give the job fetch-depth: 0)",
            shas.len()
        ));
        return;
    }
    for i in (0..shas.len() - 6).step_by(6) {
        let (head, base) = (&shas[i], &shas[i + 5]);
        let script = Command::new("node")
            .current_dir(repo_root())
            .args(["scripts/affected.mjs", base, head, "--explain"])
            .output()
            .unwrap();
        assert!(script.status.success(), "{}", String::from_utf8_lossy(&script.stderr));
        let ours = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
            .current_dir(repo_root())
            .args(["plan", base, head, "--explain"])
            .output()
            .unwrap();
        assert!(ours.status.success(), "{}", String::from_utf8_lossy(&ours.stderr));
        assert_eq!(
            canonical(serde_json::from_slice(&ours.stdout).unwrap()),
            canonical(serde_json::from_slice(&script.stdout).unwrap()),
            "different plan for {base}..{head}"
        );
    }
}
