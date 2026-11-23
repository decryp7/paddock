//! The recorded answers of the planner (`./dev plan`) on the fixture repository: the one reference of what it must answer.
//!
//! - `recorded_*`: cases in `tests/golden/cases.json` run on the fixture repository in `tests/golden/fixture` (built in a scratch
//!   folder with two commits, tagged `fixture-base` and `fixture-head`, and one commit per folder of `tests/golden/steps` on a
//!   side branch, tagged `step-<folder>`: the `.gitea` and shared-script edits); the answers are in `tests/golden/expected/`.
//!   They were first recorded from `scripts/affected.mjs` (retired 2026-11-18, ADR 0022 R4); the planner itself now writes them:
//!   a changed answer is a reviewed edit made with `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(golden::)'`, never by hand and never in CI.
//! - `recorded_command_line_*`: the same for the command line (`--ci`, `--commit-range`, the argument forms) in
//!   `tests/golden/ci-cases.json`, recorded into `tests/golden/expected/cli/`.
//! - `the_ci_jobs_table_*`: `plan_gitea::CI_JOBS` against `tests/golden/ci-jobs.json`.
//! - The narrowing tests: edits to a copy of this repository, answered by the planner alone, with positive controls.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{Request, plan_for, plan_on_graph, read_graph};
use paddock_repo_graph::GitHistory;
use paddock_testkit::TestCommand;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    crate::full_tree(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap())
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

/// Whether `PADDOCK_GOLDEN_BLESS=1` asks the recorded tests to rewrite their files from what the planner answers now.
pub(crate) fn bless() -> bool {
    bless_allowed(|k| std::env::var(k).ok())
}

/// The rule behind [`bless`], over an injectable environment: blessing is on only when asked for, and never in CI (a run that rewrites
/// its own reference would pass whatever the planner answered).
fn bless_allowed(env: impl Fn(&str) -> Option<String>) -> bool {
    let asked = env("PADDOCK_GOLDEN_BLESS").is_some_and(|v| v == "1");
    let ci = ["CI", "GITHUB_ACTIONS"].iter().any(|k| env(k).is_some_and(|v| v == "true"));
    assert!(
        !(asked && ci),
        "PADDOCK_GOLDEN_BLESS=1 is refused in CI: bless on a workstation, review the diff and commit it"
    );
    asked
}

#[test]
fn blessing_is_refused_in_ci_and_off_unless_asked_for() {
    let env =
        |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    assert!(bless_allowed(env(&[("PADDOCK_GOLDEN_BLESS", "1")])), "asked for, on a workstation");
    assert!(!bless_allowed(env(&[])), "not asked for");
    assert!(!bless_allowed(env(&[("CI", "true")])), "CI alone blesses nothing");
    let refused = std::panic::catch_unwind(|| bless_allowed(env(&[("PADDOCK_GOLDEN_BLESS", "1"), ("GITHUB_ACTIONS", "true")])));
    assert!(refused.is_err(), "asked for in CI is refused");
}

/// The kind of a planner error (its variant), recorded instead of a bare `true` so a different failure is a changed answer.
fn error_kind(e: &impl std::fmt::Debug) -> String {
    let d = format!("{e:?}");
    d.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("").to_string()
}

/// What a recorded file says on a mismatch: the one command that rewrites it, to review as a diff.
const REGENERATE: &str = "to accept the planner's new answer, review it, then run `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(golden::)'` and commit the changed files under tools/paddock-dev/tests/golden";

/// Writes `value` as the recorded files are written (one space per level, a final newline).
pub(crate) fn record(path: &Path, value: &Value) {
    // serde_json indents by two spaces; the recorded files use one (structural indentation only: a string never breaks a line).
    let mut out: String = serde_json::to_string_pretty(value)
        .unwrap()
        .lines()
        .map(|l| {
            let indent = l.len() - l.trim_start().len();
            format!("{}{}\n", " ".repeat(indent / 2), l.trim_start())
        })
        .collect();
    if out.is_empty() {
        out.push('\n');
    }
    std::fs::write(path, out).unwrap();
}

/// The plan flags a real-tree sample records (the rest of a plan moves with every new project).
const SAMPLE_KEYS: [&str; 12] = [
    "all",
    "crates",
    "ui",
    "eslint",
    "conformance",
    "c99",
    "tools",
    "icons",
    "paddockDev",
    "docs",
    "apps",
    "kinds",
];

/// The change sets of `tests/golden/real-tree-samples.json` on this repository: the flags the planner answers, as recorded from
/// the retired `scripts/affected.mjs` on 2026-11-18 (a change of the tree that moves one is a reviewed bless, not a silent edit).
#[test]
fn recorded_real_tree_samples_match_the_planners_flags() {
    let root = repo_root();
    let path = golden_dir().join("real-tree-samples.json");
    let samples: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(samples.len() >= 40, "the recorded samples");
    let history = GitHistory::new(&root, Duration::from_secs(60));
    // The graphs depend only on the tree: read once (cargo metadata of every workspace), then one plan per sample.
    let graph = read_graph(&root, &history).unwrap();
    let mut narrowed = 0;
    let mut recorded = Vec::new();
    let mut failures = Vec::new();
    for sample in &samples {
        let files: Vec<&str> = sample["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        let plan = plan_on_graph(&root, &history, &request(&files), &graph).unwrap();
        let flags: serde_json::Map<String, Value> = SAMPLE_KEYS.iter().map(|k| (k.to_string(), plan[k].clone())).collect();
        narrowed += usize::from(plan["all"] == false);
        if Value::Object(flags.clone()) != sample["plan"] {
            failures.push(format!(
                "{files:?}: recorded {}, planner {}",
                sample["plan"],
                Value::Object(flags.clone())
            ));
        }
        recorded.push(json!({"files": files, "plan": flags}));
    }
    if bless() {
        record(&path, &Value::Array(recorded));
        return;
    }
    assert!(failures.is_empty(), "{}\n{REGENERATE}", failures.join("\n"));
    // plan_for (what `dev graph affected` runs) is plan_on_graph over a fresh graph.
    let first = request(&["crates/paddock-cli/src/lib.rs"]);
    assert_eq!(
        plan_for(&root, &history, &first).unwrap(),
        plan_on_graph(&root, &history, &first, &graph).unwrap()
    );
    assert!(
        narrowed > 0,
        "a positive control: some sample narrows the plan, or a comparison of 'everything' with 'everything' proves nothing"
    );
}

/// The migration net (ADR 0117 §8): the repository model's answer beside the rule tables' for every real-tree sample, recorded in
/// `tests/golden/model-compare.json`. Every recorded difference is one of §8's listed differences or a defect; a changed difference
/// is a reviewed bless. The positive control: some sample differs (a comparison that finds nothing proves nothing until WP-5).
#[test]
fn recorded_model_compare_on_the_real_tree_samples() {
    let root = repo_root();
    let samples: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(golden_dir().join("real-tree-samples.json")).unwrap()).unwrap();
    let path = golden_dir().join("model-compare.json");
    let history = GitHistory::new(&root, Duration::from_secs(60));
    let graph = read_graph(&root, &history).unwrap();
    let mut recorded = Vec::new();
    for sample in &samples {
        let files: Vec<&str> = sample["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        let req = request(&files);
        let plan = plan_on_graph(&root, &history, &req, &graph).unwrap();
        let (_, reached) = paddock_dev::repo_model::reach_for(&root, &history, &req, &graph).unwrap();
        let inputs = paddock_dev::repo_inputs::load(&root).unwrap();
        let c = paddock_dev::repo_model::compare(&plan, &reached, &graph, &inputs);
        let diffs: Vec<Value> = c["differences"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| json!({"project": d["project"], "rules": d["rules"], "model": d["model"]}))
            .collect();
        recorded.push(
            json!({"files": files, "allRules": c["allRules"], "allModel": c["allModel"], "differences": diffs, "unowned": c["unowned"]}),
        );
    }
    let got = Value::Array(recorded);
    if bless() {
        record(&path, &got);
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|_| "null".into())).unwrap();
    assert!(
        got == want,
        "the model's differences changed:\n{}\n{REGENERATE}",
        serde_json::to_string_pretty(&got).unwrap()
    );
    assert!(
        got.as_array()
            .unwrap()
            .iter()
            .any(|s| !s["differences"].as_array().unwrap().is_empty()),
        "a positive control: some sample differs"
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
    let root = paddock_env::winpath::real(dir.path()).unwrap();
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
fn recorded_plans_match_the_planners_answers_on_the_fixture_repository() {
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
        let got = plan_for(&root, &history, &req);
        if bless() {
            let recorded = match &got {
                Ok(plan) => plan.clone(),
                Err(e) => json!({"error": true, "kind": error_kind(e)}),
            };
            record(&golden_dir().join("expected").join(format!("{name}.json")), &recorded);
            continue;
        }
        let ok = match (&got, expected.get("error").is_some()) {
            (Err(e), true) => expected["kind"].as_str().is_none_or(|k| k == error_kind(e)),
            (Ok(plan), false) => canonical(plan.clone()) == canonical(expected.clone()),
            _ => false,
        };
        if !ok {
            failures.push(format!("{name}: expected {expected}, got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "{}\n{REGENERATE}", failures.join("\n"));
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
// The command line: `dev plan` against the recorded answers

/// What a caller of the planner sees of one run, with commit ids masked (they differ per run). Its shape is the one the first recording (made with `scripts/affected.mjs`) used.
fn run_cli(program: &mut TestCommand, root: &Path, case: &Value) -> Value {
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
    for (k, _) in paddock_dev::process_env::vars() {
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

/// The fixture repository with `origin/main` at `fixture-base`, as the recording leaves it.
fn fixture_with_origin() -> (tempfile::TempDir, PathBuf) {
    let (keep, root) = fixture_repo();
    git(&root, &["update-ref", "refs/remotes/origin/main", "fixture-base"]);
    (keep, root)
}

#[test]
fn recorded_command_line_cases_match_the_planners_answers_on_the_fixture_repository() {
    let (_keep, shared_root) = fixture_with_origin();
    let mut failures = Vec::new();
    let cases = cli_cases();
    assert!(cases.len() >= 25, "the recorded command line cases");
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        // A case with an `edit` ({path: text}) runs on its own copy of the fixture with one more commit that writes those files, the
        // change of a member's branch.
        let own = case.get("edit").and_then(Value::as_object).map(|edit| {
            let (keep, root) = fixture_with_origin();
            for (path, text) in edit {
                std::fs::write(root.join(path), text.as_str().unwrap()).unwrap();
            }
            git(&root, &["add", "-A"]);
            git(&root, &["commit", "-q", "-m", "member"]);
            (keep, root)
        });
        let root = own.as_ref().map_or(&shared_root, |(_, r)| r);
        let recorded = std::fs::read_to_string(golden_dir().join("expected/cli").join(format!("{name}.json"))).ok();
        let mut program = crate::dev_bin::command();
        program.arg("plan");
        let got = run_cli(&mut program, root, case);
        if bless() {
            record(&golden_dir().join("expected/cli").join(format!("{name}.json")), &got);
            continue;
        }
        let expected: Option<Value> = recorded.map(|t| serde_json::from_str(&t).unwrap());
        if expected.as_ref() != Some(&got) {
            failures.push(format!("{name}:\n  expected {expected:?}\n  got      {got}"));
        }
    }
    assert!(failures.is_empty(), "{}\n{REGENERATE}", failures.join("\n"));
}

/// The `crates_filter` and `paddock_dev_filter` lines `./dev plan --ci` wrote for `event` with `base`, on the fixture after one more
/// commit that touches only the leaf crate `crates/b`.
fn package_filters(event: &str, base: &str, app: Option<&str>) -> (String, String, String) {
    let (_keep, root) = fixture_with_origin();
    std::fs::write(root.join("crates/b/src/lib.rs"), "// a member's one change\npub fn b() {}\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "member"]);
    let mut env = serde_json::Map::new();
    env.insert("GITHUB_EVENT_NAME".into(), json!(event));
    // A push is a push to main (the ref ci.yml's reuse and full-run rules read); the others run on a branch.
    env.insert(
        "GITHUB_REF".into(),
        json!(if event == "push" { "refs/heads/main" } else { "refs/heads/topic" }),
    );
    if !base.is_empty() {
        env.insert("PLAN_BASE".into(), json!(base));
    }
    if let Some(app) = app {
        env.insert("PLAN_APP".into(), json!(app));
    }
    let mut program = crate::dev_bin::command();
    program.arg("plan");
    let got = run_cli(&mut program, &root, &json!({"args": ["--ci"], "env": env}));
    assert_eq!(got["code"], 0, "{got}");
    let out = got["outputs"].as_str().unwrap_or_default().to_string();
    let line = |k: &str| out.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(str::to_string);
    (
        line("crates_filter").expect("the output crates_filter is always written"),
        line("paddock_dev_filter").expect("the output paddock_dev_filter is always written"),
        out,
    )
}

/// D-member (2026-11-21): a member run (a dispatch with a base) narrows nextest to the packages the change reaches; every full run
/// (a push to main, a tag, the nightly, a manual run with no base, an app's gates) tests the whole workspace.
#[test]
fn a_member_run_narrows_nextest_to_the_reached_packages_and_a_full_run_never_does() {
    let (crates, _, out) = package_filters("workflow_dispatch", "fixture-head", None);
    assert_eq!(crates, "package(=fx-b)", "{out}");
    // Positive control: the same change, the full runs. A missing narrowing would show here as the filter on a run that must be whole.
    for (event, base, app) in [
        ("push", "", None),
        ("schedule", "", None),
        ("workflow_dispatch", "", None),
        ("workflow_dispatch", "fixture-head", Some("x")),
    ] {
        let (crates, paddock_dev, out) = package_filters(event, base, app);
        assert_eq!((crates.as_str(), paddock_dev.as_str()), ("", ""), "{event} {base:?} {app:?}: {out}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The per-job narrowing of `.gitea/` and `conformance/ci` changes, made deterministic (the recorded cases and the history
// sample do not reach every job): uncommitted edits in a copy of this repository, the script as the oracle.

/// `plan_gitea::CI_JOBS` is the table `tests/golden/ci-jobs.json` records (it was `CI_JOBS` of `scripts/affected.mjs`): a change of
/// which projects a ci.yml job reaches is a reviewed edit of both.
#[test]
fn the_ci_jobs_table_equals_the_recorded_one() {
    let ours: serde_json::Map<String, Value> = paddock_dev::plan_gitea::CI_JOBS
        .iter()
        .map(|(job, projects)| (job.to_string(), json!(projects)))
        .collect();
    let path = golden_dir().join("ci-jobs.json");
    if bless() {
        record(&path, &Value::Object(ours));
        return;
    }
    let recorded: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(
        recorded.as_object().is_some_and(|m| m.len() >= 10),
        "a positive control: the recorded table is not empty"
    );
    assert_eq!(Value::Object(ours), recorded, "{REGENERATE}");
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
    let objects = paddock_env::winpath::real(&src.join(common.trim()).join("objects")).unwrap();
    std::fs::create_dir_all(dst).unwrap();
    git(dst, &["init", "-q"]);
    // `input` stores LF whatever the host's checkout wrote, and checkouts in the copy write what is stored: LF.
    git(dst, &["config", "core.autocrlf", "input"]);
    git(dst, &["config", "core.eol", "lf"]);
    // No automatic packing: the copy's own objects stay as written (and the mechanism test counts loose and packed alike).
    git(dst, &["config", "gc.auto", "0"]);
    // `canonicalize` on Windows returns a verbatim path: `\\?\C:\...` for a drive, `\\?\UNC\server\share\...` for a share. Git
    // reads `C:/...` and `//server/share/...`.
    let objects = paddock_testkit::path::text(&objects);
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

// It copies the checked-out files and runs the script and the planner for every edit. ADR 0103: the edits are split over a few
// copies that run side by side (`run_cases`); one copy, one edit after the other, took 21 s at load 27 and 80-121 s on the loaded CI
// Mac (runs 8060, 8066, 8099, SLOW lines).
#[test]
fn workflow_action_and_script_edits_narrow_the_plan() {
    // The jobs of ci.yml that CI_JOBS names; one action, one reusable workflow, one script of conformance/ci and a helper it imports.
    let jobs: Vec<&str> = paddock_dev::plan_gitea::CI_JOBS.iter().map(|(j, _)| *j).collect();
    let ci = std::fs::read_to_string(repo_root().join(".gitea/workflows/ci.yml")).unwrap();
    let mut edits: Vec<(String, Option<String>)> = jobs
        .iter()
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
    let narrowed = std::sync::atomic::AtomicUsize::new(0);
    let per = edits.len().div_ceil(paddock_testkit::cases::case_threads().min(4));
    let cases = edits
        .chunks(per)
        .enumerate()
        .map(|(n, chunk)| {
            let narrowed = &narrowed;
            paddock_testkit::case(format!("copy {n} ({} edits)", chunk.len()), move || {
                edits_on_a_copy(chunk, narrowed)
            })
        })
        .collect();
    paddock_testkit::run_cases("the workflow, action and script edits", cases);
    let narrowed = narrowed.into_inner();
    assert!(
        narrowed >= 10,
        "a positive control: most of these edits narrow the plan ({narrowed} of {})",
        edits.len()
    );
}

/// One case of [`workflow_action_and_script_edits_narrow_the_plan`]: a copy of its own, then each edit
/// in turn (the edits share the copy, so they run one after the other here), counting the plans that narrowed.
fn edits_on_a_copy(edits: &[(String, Option<String>)], narrowed: &std::sync::atomic::AtomicUsize) {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy");
    let started = Instant::now();
    copy_checked_out_files(&repo_root(), &copy);
    eprintln!("copy of the checked-out files: {:.1} s", started.elapsed().as_secs_f64());
    // LF checkout on every host: the workflow split reads LF text (a CRLF checkout cannot be split by either planner).
    assert!(
        !std::fs::read_to_string(copy.join(".gitea/workflows/ci.yml"))
            .unwrap()
            .contains('\r'),
        "the copy's ci.yml has CRLF line endings: the workflow split reads LF text, so every edit would mark everything"
    );
    let history = GitHistory::new(&copy, Duration::from_secs(60));
    // The edits change the text of tracked files, never which files or manifests exist: one reading of the graph serves all.
    let started = Instant::now();
    let graph = read_graph(&copy, &history).unwrap();
    eprintln!("read_graph: {:.1} s", started.elapsed().as_secs_f64());
    for (rel, job) in edits {
        touch(&copy, rel, job.as_deref());
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
        eprintln!("edit of {rel} (job {job:?}): plan {:.1} s", started.elapsed().as_secs_f64());
        // A job edit reaches exactly the projects CI_JOBS names for it, plus conformance (in every plan); an action or script edit
        // is narrowed, never everything. (`plan` wires every other job, so its edit is everything by design.)
        match job {
            Some(job) => {
                let projects = paddock_dev::plan_gitea::CI_JOBS.iter().find(|(j, _)| j == job).unwrap().1;
                if projects == ["everything"] {
                    assert_eq!(got["all"], true, "an edit of the plan job reaches everything");
                } else {
                    assert_eq!(got["all"], false, "an edit of job {job} is narrowed");
                    let mut want: std::collections::BTreeSet<String> = projects.iter().map(|p| p.to_string()).collect();
                    want.insert("conformance".into());
                    let mut have: std::collections::BTreeSet<String> = [
                        ("crates", "crates"),
                        ("ui", "ui"),
                        ("eslint", "eslint"),
                        ("conformance", "conformance"),
                        ("c99", "c99"),
                        ("tools", "tools"),
                        ("icons", "icons"),
                        ("ciImage", "ci-image"),
                        ("paddockDev", "paddock-dev"),
                    ]
                    .iter()
                    .filter(|(k, _)| got[k] == true)
                    .map(|(_, p)| p.to_string())
                    .collect();
                    if got["apps"].as_array().is_some_and(|a| !a.is_empty()) {
                        have.insert("apps".into());
                    }
                    assert_eq!(have, want, "projects reached by an edit of job {job} of {rel}");
                }
            }
            None => assert_eq!(got["all"], false, "an edit of {rel} is narrowed"),
        }
        narrowed.fetch_add(usize::from(got["all"] == false), std::sync::atomic::Ordering::Relaxed);
        let restored = Command::new("git")
            .arg("-C")
            .arg(&copy)
            .args(["checkout", "-q", "--", rel])
            .status()
            .unwrap();
        assert!(restored.success());
    }
}

#[test]
fn guard_edits_mark_conformance() {
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
        eprintln!("guard edit of {rel} ({add:?}): plan {:.1} s", started.elapsed().as_secs_f64());
        let reasons = got["why"]["conformance"].as_array().cloned().unwrap_or_default();
        let marked = reasons
            .iter()
            .any(|r| r.as_str().is_some_and(|r| r.starts_with("C100: an ignore attribute")));
        assert_eq!(marked, marks, "the content rule on {rel} with {add:?}");
        std::fs::write(&path, original).unwrap();
    }
}
