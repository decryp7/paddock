//! The graph crate plus Paddock's rules reproduce what `scripts/affected.mjs` answered.
//!
//! - `recorded_*`: cases in `tests/golden/cases.json` run on the fixture repository in `tests/golden/fixture` (built in a scratch
//!   folder with two commits, tagged `fixture-base` and `fixture-head`, and one commit per folder of `tests/golden/steps` on a
//!   side branch, tagged `step-<folder>`: the `.gitea` and shared-script edits); the answers of the script were recorded once into `tests/golden/expected/`
//!   by `tests/golden/generate.mjs`. These need no node, so they outlive the script (ADR 0022 R4 deletes it).
//! - `recorded_command_line_*`: the same for the command line (`--ci`, `--commit-range`, the argument forms) in
//!   `tests/golden/ci-cases.json`, recorded into `tests/golden/expected/cli/`.
//! - `live_*`: the script itself as the oracle on this working tree, while the script exists. The `.gitea/` and `conformance/ci`
//!   samples need a clean tree: the script narrows those only for files that differ from HEAD (it answers everything for a clean one). Skipped, with the reason printed,
//!   when node or the script is missing.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{Request, plan_for, plan_on_graph, read_graph};
use paddock_repo_graph::GitHistory;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// A live comparison that cannot run. On a workstation a note. In CI on Linux (the paddock-ci image has node and a full clone) a
/// failure, so a missing oracle or a shallow clone cannot let the comparison pass unseen. In CI on a Windows or macOS host leg, which
/// has no node of its own, a visible `::notice::` (the Linux leg runs the same comparison).
fn skip(reason: &str) {
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "true"));
    if ci && cfg!(target_os = "linux") {
        panic!("the live comparison cannot run in CI: {reason}");
    }
    if ci {
        println!("::notice::the live comparison against scripts/affected.mjs was skipped on this host: {reason}. The Linux leg runs it.");
    } else {
        eprintln!("skipped: {reason}");
    }
}

/// The history comparison is platform-independent logic that the Linux leg proves; on a Windows or macOS host leg in CI it costs
/// minutes of node and graph reads on a loaded runner. There it says so (a `::notice::` in the job log) and stops.
fn only_on_the_linux_leg(what: &str) -> bool {
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| std::env::var(k).is_ok_and(|v| v == "true"));
    let host_leg = ci && !cfg!(target_os = "linux");
    if host_leg {
        println!("::notice::{what} was skipped on this host leg: it is platform-independent and the Linux leg runs it.");
    }
    host_leg
}

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
}

/// What `node scripts/affected.mjs --files <files> --explain` answers for each sample, from one node process: the script's
/// `planFor` per sample (the function its command line calls), with its `cargo metadata` runs remembered across samples (the
/// tree does not change between them). Run per sample, the script read every workspace's metadata again each time: 36 samples
/// times 23 workspaces, which took 195 s of the 240 s limit on the Windows runner (run 7810 job 33008) and timed out in
/// others. Returns the plans and how many times cargo really ran.
fn oracle(samples: &[&[&str]]) -> (Vec<Value>, usize) {
    const DRIVER: &str = r#"
import { createRequire, syncBuiltinESMExports } from 'node:module';
import { pathToFileURL } from 'node:url';
const cp = createRequire(import.meta.url)('node:child_process');
const real = cp.execFileSync;
const seen = new Map();
cp.execFileSync = (file, args, opts) => {
  if (file !== 'cargo') return real(file, args, opts);
  const key = JSON.stringify([args, opts?.cwd ?? null]);
  if (!seen.has(key)) seen.set(key, real(file, args, opts));
  return seen.get(key);
};
syncBuiltinESMExports();
// Through the environment: a script path in argv[1] would make the script think it is the program (isMain) and run its CLI.
const { planFor } = await import(pathToFileURL(process.env.PADDOCK_GOLDEN_SCRIPT).href);
const samples = JSON.parse(process.env.PADDOCK_GOLDEN_SAMPLES);
const plans = samples.map((files) => planFor({ files, explain: true }).plan);
process.stdout.write(JSON.stringify({ plans, cargoRuns: seen.size }));
"#;
    let out = Command::new("node")
        .args(["--input-type=module", "-e", DRIVER])
        .env("PADDOCK_GOLDEN_SCRIPT", repo_root().join("scripts/affected.mjs"))
        .env("PADDOCK_GOLDEN_SAMPLES", serde_json::to_string(samples).unwrap())
        .current_dir(repo_root())
        .output()
        .expect("node is required: the golden test runs scripts/affected.mjs");
    assert!(
        out.status.success(),
        "affected.mjs failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let plans = v["plans"].as_array().unwrap().clone();
    (plans, v["cargoRuns"].as_u64().unwrap() as usize)
}

fn request(files: &[&str]) -> Request {
    Request {
        files: files.iter().map(|s| s.to_string()).collect(),
        all: false,
        explain: true,
        root: None,
        base: None,
        head: "HEAD".into(),
    }
}

/// The Cargo workspaces of the tree (a tracked `Cargo.toml` with a `[workspace]` line), as both planners find them.
fn workspaces(root: &Path) -> usize {
    let out = Command::new("git")
        .args(["ls-files", "*Cargo.toml"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|f| {
            std::fs::read_to_string(root.join(f))
                .map(|t| t.lines().any(|l| l == "[workspace]"))
                .unwrap_or(false)
        })
        .count()
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
    &["ui/projects/paddock-ui/shell/generated/public-api.ts"],
    &["ui/projects/paddock-ui/shell/generated/dialogs.ts"],
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
    &[".gitea/actions/app-gates/action.yml", ".gitea/workflows/nightly.yml"],
    &[".gitignore", "crates/paddock-cli/CHANGELOG.md"],
    &["docs/gates.toml"],
    &["apps/dyno/paddock.ci.json", "AGENTS.md"],
    &["lefthook.yml", "apps/dyno/scripts/check-wording.mjs"],
    // C99's inputs (ADR 0057 §3.4) plan the `c99` job; a docs-only change does not.
    &["apps/debrief/Cargo.lock"],
    &["apps/pitwall/crates/pitwall-server/Cargo.toml"],
    &["apps/pitwall/crates/pitwall-server/src/lib.rs"],
    &["crates/paddock-secrets/Cargo.toml"],
    &["apps/dyno/app/src-tauri/capabilities/default.json"],
    &["apps/pitwall/paddock.ci.json"],
    &["conformance/src/checks/C99-no-keychain.mjs"],
    &["diagrams/paddock.xml"],
    // An app's Cargo.toml and capabilities are read by conformance C48.
    &["apps/dyno/app/src-tauri/Cargo.toml"],
];

#[test]
fn live_plan_matches_affected_mjs_on_sample_change_sets() {
    if !repo_root().join("scripts/affected.mjs").exists() || Command::new("node").arg("--version").output().is_err() {
        skip("scripts/affected.mjs or node is not available (the recorded cases still run)");
        return;
    }
    let root = repo_root();
    let history = GitHistory::new(&root, Duration::from_secs(60));
    // The graphs depend only on the tree: read once (cargo metadata of every workspace), then one plan per sample.
    let graph = read_graph(&root, &history).unwrap();
    let (theirs, cargo_runs) = oracle(SAMPLES);
    assert_eq!(theirs.len(), SAMPLES.len());
    let workspaces = workspaces(&root);
    assert!(
        cargo_runs > 0 && cargo_runs <= workspaces,
        "the oracle ran cargo metadata {cargo_runs} times for {workspaces} workspaces: once per workspace for all samples"
    );
    let mut narrowed = 0;
    for (files, their) in SAMPLES.iter().zip(theirs) {
        let got = canonical(plan_on_graph(&root, &history, &request(files), &graph).unwrap());
        assert_eq!(got, canonical(their), "different plan for {files:?}");
        narrowed += usize::from(got["all"] == false);
    }
    // plan_for (what `dev graph affected` runs) is plan_on_graph over a fresh graph.
    let first = request(SAMPLES[0]);
    assert_eq!(
        plan_for(&root, &history, &first).unwrap(),
        plan_on_graph(&root, &history, &first, &graph).unwrap()
    );
    assert!(
        narrowed > 0,
        "a positive control: some sample narrows the plan, or a comparison of 'everything' with 'everything' proves nothing"
    );
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
    // The steps: one commit each on a side branch, so `main` and HEAD stay at `fixture-head` for the command line cases.
    git(&root, &["checkout", "-q", "-b", "steps"]);
    let mut steps: Vec<PathBuf> = std::fs::read_dir(golden_dir().join("steps"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    steps.sort();
    for step in steps {
        copy_tree(&step, &root);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "step"]);
        git(&root, &["tag", &format!("step-{}", step.file_name().unwrap().to_string_lossy())]);
    }
    git(&root, &["checkout", "-q", "main"]);
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
        "gitea-ci-yml-one-job",
        "gitea-shared-action",
        "gitea-helper-script",
        "gitea-reusable-workflow",
        "gitea-action-named-only-in-a-comment",
        "gitea-ci-yml-header",
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
    // The planner reads the workflow's environment; everything else (PATH, RUSTUP_HOME, SystemRoot, ...) stays as the runner has it.
    program.current_dir(root);
    for (k, _) in std::env::vars() {
        if k.starts_with("GITHUB_")
            || k.starts_with("PLAN_")
            || ["EVENT", "REF", "BEFORE", "SHA", "BASE_REF", "INPUT_APP"].contains(&k.as_str())
        {
            program.env_remove(k);
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
    for f in ["is-main.mjs", "full-checkout.mjs"] {
        std::fs::copy(
            repo_root().join("conformance/src/util").join(f),
            root.join("conformance/src/util").join(f),
        )
        .unwrap();
    }
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
    if only_on_the_linux_leg("the first-parent history comparison") {
        return;
    }
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
    let mut narrowed = 0;
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
        let got = canonical(serde_json::from_slice(&ours.stdout).unwrap());
        assert_eq!(
            got,
            canonical(serde_json::from_slice(&script.stdout).unwrap()),
            "different plan for {base}..{head}"
        );
        narrowed += usize::from(got["all"] == false);
    }
    // The guarantee that narrowing works is the edits test; this sample only shows it on real history, which may be all 'everything'.
    if narrowed == 0 {
        println!(
            "::warning::no range of the last commits narrowed the plan, so the history comparison compared only 'everything' plans; the edits test carries the narrowing guarantee."
        );
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The per-job narrowing of `.gitea/` and `conformance/ci` changes, made deterministic (the recorded cases and the history
// sample do not reach every job): uncommitted edits in a copy of this repository, the script as the oracle.

fn node_available() -> bool {
    repo_root().join("scripts/affected.mjs").exists() && Command::new("node").arg("--version").output().is_ok()
}

#[test]
fn live_ci_jobs_table_equals_the_one_affected_mjs_exports() {
    if !node_available() {
        skip("scripts/affected.mjs or node is not available");
        return;
    }
    let out = Command::new("node")
        .current_dir(repo_root())
        .args([
            "-e",
            "import('./scripts/affected.mjs').then(m => console.log(JSON.stringify(m.CI_JOBS)))",
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let script: Value = serde_json::from_slice(&out.stdout).unwrap();
    let ours: serde_json::Map<String, Value> = paddock_dev::plan_gitea::CI_JOBS
        .iter()
        .map(|(job, projects)| (job.to_string(), json!(projects)))
        .collect();
    assert_eq!(
        Value::Object(ours),
        script,
        "plan_gitea::CI_JOBS and CI_JOBS of scripts/affected.mjs must be the same table"
    );
}

/// Adds a comment line to `rel` in `root` (inside the job `job` of a workflow, right under its key, when one is named).
fn touch(root: &Path, rel: &str, job: Option<&str>) {
    let path = root.join(rel);
    let text = std::fs::read_to_string(&path).unwrap();
    let edited = match job {
        Some(job) => {
            let key = regex::Regex::new(&format!(r"(?m)^  {}:[^\n]*\n", regex::escape(job))).unwrap();
            let m = key.find(&text).unwrap_or_else(|| panic!("job {job} in {rel}"));
            format!("{}    # touched by the narrowing test\n{}", &text[..m.end()], &text[m.end()..])
        }
        None if rel.ends_with(".mjs") => format!("{text}\n// touched by the narrowing test\n"),
        None => format!("{text}\n# touched by the narrowing test\n"),
    };
    std::fs::write(path, edited).unwrap();
}

/// A fresh one-commit repository in `dst` holding the files of `src` that are present on disk, as stored (LF for text).
///
/// Not `git clone`: CI checks out a depth-1, blob-less, sparse clone, in which a local clone must fetch blobs from a promisor
/// remote that is not there ("lazy fetching disabled"). The files this test reads are on disk in every kind of checkout.
/// Ignore rules do not apply: the index is built from `src`'s, so tracked files that a rule matches stay in the graph, as in the
/// real repository.
///
/// Cheap by design (both live narrowing tests run it at once, on a loaded Windows host too): the copy borrows `src`'s object store
/// (`objects/info/alternates`) and takes its index entries, so no file is hashed or compressed again, and git writes each file
/// once, as stored (LF for text). The blob of every file on disk is in that store, a blob-less clone's included: checking a file out fetched it.
/// Only files edited in `src` (`ls-files -m`, none in CI) are hashed, from their content on disk.
fn copy_checked_out_files(src: &Path, dst: &Path) {
    use std::io::Write;
    use std::process::Stdio;
    let git = |dir: &Path, args: &[&str]| {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        out.stdout
    };
    let git_with_input = |dir: &Path, args: &[&str], input: Vec<u8>| {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // The input is written from its own thread while this one drains stdout and stderr, so neither side can stall on a full pipe.
        let mut stdin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let out = child.wait_with_output().unwrap();
        writer.join().unwrap().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    let on_disk = |rel: &str| std::fs::symlink_metadata(src.join(rel)).is_ok_and(|m| m.is_file());
    // `<mode> <blob> <stage>\t<path>` per entry. Files outside a sparse checkout are listed but not on disk (and their blobs are
    // not in a blob-less clone's store); a symlink is not a regular file here (conformance C57).
    let staged = git(src, &["ls-files", "--stage", "-z"]);
    let mut entries = Vec::new();
    for entry in staged.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let text = std::str::from_utf8(entry).unwrap();
        let (_, rel) = text.split_once('\t').unwrap_or_else(|| panic!("ls-files --stage entry {text:?}"));
        if on_disk(rel) {
            entries.extend_from_slice(entry);
            entries.push(0);
        }
    }
    let common = String::from_utf8(git(src, &["rev-parse", "--git-common-dir"])).unwrap();
    let objects = src.join(common.trim()).join("objects").canonicalize().unwrap();
    std::fs::create_dir_all(dst).unwrap();
    git(dst, &["init", "-q"]);
    // `input` stores LF whatever the host's checkout wrote, and checkouts in the copy write what is stored: LF.
    git(dst, &["config", "core.autocrlf", "input"]);
    git(dst, &["config", "core.eol", "lf"]);
    // No automatic packing: the copy's own objects stay as written (and the mechanism test counts loose and packed alike).
    git(dst, &["config", "gc.auto", "0"]);
    // `canonicalize` on Windows returns a verbatim path: `\\?\C:\...` for a drive, `\\?\UNC\server\share\...` for a share. Git
    // reads `C:/...` and `//server/share/...`.
    let objects = objects.to_str().unwrap();
    let objects = match objects.strip_prefix(r"\\?\UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => objects.strip_prefix(r"\\?\").unwrap_or(objects).to_string(),
    }
    .replace('\\', "/");
    std::fs::write(dst.join(".git/objects/info/alternates"), format!("{objects}\n")).unwrap();
    git_with_input(dst, &["update-index", "-z", "--index-info"], entries);
    // A fresh index has no file on disk to keep: every entry is written, and `-u` records what was written.
    git(dst, &["checkout-index", "--all", "--force", "--quiet", "-u"]);
    let edited: Vec<String> = modified_files(src).into_iter().filter(|rel| on_disk(rel)).collect();
    if !edited.is_empty() {
        for rel in &edited {
            std::fs::copy(src.join(rel), dst.join(rel)).unwrap_or_else(|e| panic!("copy {rel}: {e}"));
        }
        let list: Vec<u8> = edited.iter().flat_map(|rel| rel.bytes().chain([0])).collect();
        git_with_input(
            dst,
            &[
                "-c",
                "core.safecrlf=false",
                "add",
                "-f",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            list.clone(),
        );
        // The edited files as stored (LF), as the rest.
        for rel in &edited {
            std::fs::remove_file(dst.join(rel)).unwrap();
        }
        git_with_input(dst, &["checkout-index", "--force", "--quiet", "-u", "-z", "--stdin"], list);
    }
    git(
        dst,
        &[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "--no-verify",
            "-m",
            "copy",
        ],
    );
}

/// Tracked files of `src` whose content on disk differs from the index (`git ls-files -m`).
fn modified_files(src: &Path) -> Vec<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(src)
        .args(["ls-files", "-m", "-z"])
        .output()
        .unwrap();
    assert!(out.status.success(), "git ls-files -m: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| String::from_utf8(r.to_vec()).unwrap())
        .collect()
}

/// Objects in `repo`'s own store, loose and packed (`count` plus `in-pack` of `git count-objects -v`; its alternates are not
/// counted). Both, because `gc --auto` may pack loose objects at any time.
fn own_objects(repo: &Path) -> usize {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["count-objects", "-v"])
        .output()
        .unwrap();
    assert!(out.status.success(), "git count-objects: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix("count: ").or_else(|| l.strip_prefix("in-pack: ")))
        .map(|n| n.trim().parse::<usize>().unwrap())
        .sum()
}

/// The live narrowing tests each copy this repository, at the same time. Hashing and compressing every checked-out file into a new
/// object store (`git add -A` of the about 11,000 files of CI's lean checkout, then a full rewrite) took most of their 240 s budget on the Windows
/// host runner with the host to itself (run 7881: 183 s and 236 s) and passed it when another job shared the host (runs 7879,
/// 7893). The copy borrows the source's objects instead and hashes only files that differ from them: its own store holds the
/// trees a sparse checkout changes, the commit and those few files.
#[test]
fn the_repository_copy_reuses_the_source_objects_instead_of_hashing_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    copy_checked_out_files(&repo_root(), &copy);
    let listed = Command::new("git")
        .arg("-C")
        .arg(&copy)
        .args(["ls-files", "-z"])
        .output()
        .unwrap()
        .stdout;
    let files: Vec<&Path> = listed
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| Path::new(std::str::from_utf8(r).unwrap()))
        .collect();
    let folders: std::collections::BTreeSet<&Path> = files.iter().flat_map(|f| f.ancestors().skip(1)).collect();
    assert!(
        files.len() > 1000,
        "a positive control: the copy holds the checked-out files ({})",
        files.len()
    );
    let own = own_objects(&copy);
    let budget = folders.len() + 1 + modified_files(&repo_root()).len();
    assert!(
        own <= budget,
        "the copy wrote {own} objects of its own for {} files in {} folders (at most {budget} expected): it hashed the files \
         instead of borrowing the source's objects",
        files.len(),
        folders.len()
    );
}

// Takes about a minute: it copies the checked-out files and runs the script and the planner for every edit.
#[test]
fn live_workflow_action_and_script_edits_narrow_exactly_as_affected_mjs_does() {
    if !node_available() {
        skip("scripts/affected.mjs or node is not available");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    let started = Instant::now();
    copy_checked_out_files(&repo_root(), &copy);
    eprintln!("copy of the checked-out files: {:.1} s", started.elapsed().as_secs_f64());
    assert!(
        !std::fs::read_to_string(copy.join(".gitea/workflows/ci.yml"))
            .unwrap()
            .contains('\r'),
        "the copy's ci.yml has CRLF line endings: the workflow split reads LF text, so every edit would mark everything"
    );
    // LF checkout on every host: the workflow split reads LF text (a CRLF checkout cannot be split by either planner).
    // The jobs of ci.yml that CI_JOBS names, from the script's own table; one action, one reusable workflow, one script of
    // conformance/ci and a helper it imports.
    let jobs: Value = serde_json::from_slice(
        &Command::new("node")
            .current_dir(&copy)
            .args([
                "-e",
                "import('./scripts/affected.mjs').then(m => console.log(JSON.stringify(Object.keys(m.CI_JOBS))))",
            ])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let ci = std::fs::read_to_string(copy.join(".gitea/workflows/ci.yml")).unwrap();
    let mut edits: Vec<(String, Option<String>)> = jobs
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|j| ci.contains(&format!("\n  {j}:")))
        .map(|j| (".gitea/workflows/ci.yml".to_string(), Some(j.to_string())))
        .collect();
    assert!(edits.len() >= 10, "ci.yml has the jobs CI_JOBS names: {edits:?}");
    for rel in [
        ".gitea/actions/node-caches/action.yml",
        ".gitea/workflows/app-fuzz.yml",
        "conformance/ci/install-test-browser.mjs",
        "conformance/ci/runtime-catalogue.mjs",
        "conformance/ci/build-folder.sh",
    ] {
        edits.push((rel.to_string(), None));
    }
    let history = GitHistory::new(&copy, Duration::from_secs(60));
    // The edits change the text of tracked files, never which files or manifests exist: one reading of the graph serves all.
    let started = Instant::now();
    let graph = read_graph(&copy, &history).unwrap();
    eprintln!("read_graph: {:.1} s", started.elapsed().as_secs_f64());
    let mut narrowed = 0;
    for (rel, job) in &edits {
        touch(&copy, rel, job.as_deref());
        let started = Instant::now();
        let script = Command::new("node")
            .current_dir(&copy)
            .args(["scripts/affected.mjs", "--files", rel, "--explain"])
            .output()
            .unwrap();
        let script_took = started.elapsed().as_secs_f64();
        assert!(script.status.success(), "{}", String::from_utf8_lossy(&script.stderr));
        let req = Request {
            files: vec![rel.clone()],
            all: false,
            explain: true,
            root: None,
            base: None,
            head: "HEAD".into(),
        };
        let started = Instant::now();
        let got = canonical(plan_on_graph(&copy, &history, &req, &graph).unwrap());
        eprintln!(
            "edit of {rel} (job {job:?}): affected.mjs {script_took:.1} s, plan {:.1} s",
            started.elapsed().as_secs_f64()
        );
        let want = canonical(serde_json::from_slice(&script.stdout).unwrap());
        assert_eq!(got, want, "different plan for an edit of {rel} (job {job:?})");
        narrowed += usize::from(got["all"] == false);
        let restored = Command::new("git")
            .arg("-C")
            .arg(&copy)
            .args(["checkout", "-q", "--", rel])
            .status()
            .unwrap();
        assert!(restored.success());
    }
    assert!(
        narrowed >= 10,
        "a positive control: most of these edits narrow the plan ({narrowed} of {})",
        edits.len()
    );
}

#[test]
fn live_guard_edits_mark_conformance_exactly_as_affected_mjs_does() {
    if !node_available() {
        skip("scripts/affected.mjs or node is not available");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    let started = Instant::now();
    copy_checked_out_files(&repo_root(), &copy);
    eprintln!("copy of the checked-out files: {:.1} s", started.elapsed().as_secs_f64());
    let history = GitHistory::new(&copy, Duration::from_secs(60));
    let started = Instant::now();
    let graph = read_graph(&copy, &history).unwrap();
    eprintln!("read_graph: {:.1} s", started.elapsed().as_secs_f64());
    // A new Rust test with an ignore and a TypeScript spec with an environment guard, and a comment-only edit of each (no mark).
    let edits: [(&str, &str, bool); 4] = [
        (
            "crates/paddock-cli/src/lib.rs",
            "\n#[test]\n#[ignore]\nfn skipped_by_the_guard_test() {}\n",
            true,
        ),
        ("crates/paddock-cli/src/lib.rs", "\n// a comment\n", false),
        (
            "ui/projects/paddock-ui/src/public-api.ts",
            "\nit('guarded', () => { void process.env.X; });\n",
            true,
        ),
        ("ui/projects/paddock-ui/src/public-api.ts", "\n// a comment\n", false),
    ];
    for (rel, add, marks) in edits {
        let path = copy.join(rel);
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{original}{add}")).unwrap();
        let started = Instant::now();
        let script = Command::new("node")
            .current_dir(&copy)
            .args(["scripts/affected.mjs", "--files", rel, "--explain"])
            .output()
            .unwrap();
        let script_took = started.elapsed().as_secs_f64();
        assert!(script.status.success(), "{}", String::from_utf8_lossy(&script.stderr));
        let req = Request {
            files: vec![rel.to_string()],
            all: false,
            explain: true,
            root: None,
            base: None,
            head: "HEAD".into(),
        };
        let started = Instant::now();
        let got = canonical(plan_on_graph(&copy, &history, &req, &graph).unwrap());
        eprintln!(
            "guard edit of {rel} ({add:?}): affected.mjs {script_took:.1} s, plan {:.1} s",
            started.elapsed().as_secs_f64()
        );
        assert_eq!(
            got,
            canonical(serde_json::from_slice(&script.stdout).unwrap()),
            "different plan for a guard edit of {rel}"
        );
        let reasons = got["why"]["conformance"].as_array().cloned().unwrap_or_default();
        let marked = reasons
            .iter()
            .any(|r| r.as_str().is_some_and(|r| r.starts_with("C100: an ignore attribute")));
        assert_eq!(marked, marks, "the content rule on {rel} with {add:?}");
        std::fs::write(&path, original).unwrap();
    }
}
