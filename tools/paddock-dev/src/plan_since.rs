//! `dev plan --since-run <run>`: a plan that reruns only what failed in a finished CI run (ER-10, rule 50(i)): the failed jobs,
//! and inside a job whose failures are all nextest failures in a step that takes a filter, only the failed tests. Passed jobs
//! are not rerun; the full run at the end catches what the fixes broke elsewhere.
//!
//! The decision is made from the run's job list (each job's failed steps) and the failed jobs' logs:
//!
//! - nextest: every run in the log (`Starting N tests` ... `Summary [..] N tests run: ...`) must have ended with its summary,
//!   run every test it started and have no fewer `FAIL`/`TIMEOUT`/`SIG*` lines than its summary counts failures; otherwise the
//!   whole job reruns with the reason (ER-27). The filterset names each failed test exactly (`binary_id(=b) & test(=t)`), or its
//!   whole test binary when the name has characters a filterset would have to escape: never fewer tests than failed.
//! - a job that failed in any other step (a build, clippy, a doc test, a check, the end-to-end tests) reruns whole.
//! - vitest: the failed spec files are read and reported (`vitest`). A job whose only failed step is a vitest step that takes a
//!   file list ([`VITEST_STEPS`]: Debrief's `frontend`) reruns only those spec files, when the log's `Test Files` summary counts
//!   exactly as many failed files as it names; no ci.yml step takes a file list yet, so such a ci.yml job reruns whole.
//!
//! An app's own workflow (`apps/<app>/paddock.ci.json` `workflow`, today only `debrief.yml`) is planned the same way through
//! [`SincePlan::app_outputs`]: its jobs are the workflow file's own (an unknown one plans everything), a job that was skipped
//! because a job it `needs` failed reruns too, matrix legs are read from the job's `name:` template (`end-to-end (shard 2/4)`),
//! and [`APP_WORKFLOWS`] says which jobs only a full run has.
//!
//! The pure core is here ([`nextest_failures`], [`classify`], [`SincePlan`]); [`GiteaRuns`] reads a run through the Gitea API.

use crate::log_text::plain;
use crate::plan_gitea::{CI_JOBS, workflow_jobs};
use crate::wait::Http;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

fn re(pattern: &str) -> Regex {
    // The patterns are literals of this file, checked by the tests below.
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

/// The steps of a workflow's job that run nextest and take a filterset (ci.yml: `matrix.filter`; debrief.yml: the plan job's
/// `<job>_filter` output), as `(workflow, job, steps, composite)`. A job whose failed steps are all listed here, and whose
/// failures are all read from its log, reruns only its failed tests. `composite`: the step is a composite action that runs
/// several commands and names each failed one in an `::error::failed (exit N): <command>` line (app-gates), so every such line
/// must be a nextest run. Debrief's `Memory budget` is `continue-on-error` (it never fails the job, but the API may list it as
/// failed); the narrowed rerun skips it. A failed `No temp folders left behind` check is not listed: the leaked folder may be
/// any test's, not only a failed one's, so the whole job reruns (run 7961 failed both steps and reruns whole).
pub const TEST_STEPS: &[(&str, &str, &[&str], bool)] = &[
    ("ci.yml", "crate", &["Tests (workspace, every feature)"], false),
    ("ci.yml", "paddock-dev", &["Tests"], false),
    ("ci.yml", "app", &["Run ./.gitea/actions/app-gates"], true),
    ("debrief.yml", "rust-test", &["Tests (workspace)", "Memory budget"], false),
    ("debrief.yml", "rust-test-macos", &["Tests (workspace)"], false),
];

/// The steps of a workflow's job that run vitest and take a list of spec files (the plan job's `<job>_include` output), as
/// `(workflow, job, steps)`. A job whose only failed steps are these reruns only the failed spec files.
pub const VITEST_STEPS: &[(&str, &str, &[&str])] = &[("debrief.yml", "frontend", &["Frontend unit tests"])];

/// An app's own workflow as a rerun of its failures needs it: the jobs only a full run has, and the jobs whose content a full
/// run changes (Debrief's end-to-end tests add WebKit). A rerun of such a job from a full run must itself be full.
pub struct AppWorkflow {
    /// The workflow file (`debrief.yml`).
    pub file: &'static str,
    /// The input that makes a manual run full (`bundles`).
    pub full_input: &'static str,
    /// Jobs that run only in a full run.
    pub full_only: &'static [&'static str],
    /// Jobs that run in every run but test more in a full one.
    pub full_changes: &'static [&'static str],
    /// Jobs a manual run never has (a tag's release, a schedule's mark): never added as skipped dependents.
    pub never_manual: &'static [&'static str],
    /// Sharded jobs whose failed shards the plan job names (`<job>_shards`, a matrix `include` of `{"shard": n}` legs).
    pub shard_jobs: &'static [&'static str],
}

/// The line the plan job of an app workflow prints with the run's mode (`plan mode: full=true`), read back by a rerun.
pub const MODE_LINE: &str = "plan mode: full=";

/// The app workflows a rerun of failures is planned for (`dev plan --ci --app <app>` with `PLAN_SINCE_RUN`).
pub const APP_WORKFLOWS: &[AppWorkflow] = &[AppWorkflow {
    file: "debrief.yml",
    full_input: "bundles",
    full_only: &["tauri-smoke", "bundle"],
    full_changes: &["e2e"],
    never_manual: &["release", "nightly-mark"],
    shard_jobs: &["e2e"],
}];

/// The outputs [`SincePlan::app_outputs`] can write for an app workflow besides `rerun_jobs`: `<job>_filter` of its
/// [`TEST_STEPS`] jobs, `<job>_include` of its [`VITEST_STEPS`] jobs and `<job>_shards` of its sharded jobs.
pub fn app_output_names() -> Vec<String> {
    let mut out = Vec::new();
    for a in APP_WORKFLOWS {
        out.extend(
            TEST_STEPS
                .iter()
                .filter(|t| t.0 == a.file)
                .map(|t| format!("{}_filter", t.1.replace('-', "_"))),
        );
        out.extend(
            VITEST_STEPS
                .iter()
                .filter(|t| t.0 == a.file)
                .map(|t| format!("{}_include", t.1.replace('-', "_"))),
        );
        out.extend(a.shard_jobs.iter().map(|j| format!("{}_shards", j.replace('-', "_"))));
    }
    out
}

/// The line a plan job prints AFTER its decision (`plan verdict: complete=true`): true only when the plan ran its jobs
/// (`run=true`), the run is not a rerun of failures (`since_run` empty) and the mode is full. [`MODE_LINE`] states the scope
/// before the decision (a skipped or rerun run can log `full=true`); this line is the one `ci verdict` trusts.
pub const COMPLETE_LINE: &str = "plan verdict: complete=";

/// The boolean a plan job's log states after `prefix`: `None` when no line starts with it (after the colours and the
/// timestamp), `Err` for a value other than `true` or `false`. Only the FIRST such line counts, and only at the start of a line.
fn read_flag(log: &str, prefix: &str) -> Option<Result<bool, String>> {
    let v = log
        .lines()
        .map(plain)
        .find_map(|l| l.trim().strip_prefix(prefix).map(|v| v.trim().to_string()))?;
    Some(match v.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("the plan job logged an unknown value {other:?} after {prefix:?}")),
    })
}

/// The mode a plan job's log states with the [`MODE_LINE`] (the scope of the run); see [`read_flag`].
pub fn read_marker(log: &str) -> Option<Result<bool, String>> {
    read_flag(log, MODE_LINE)
}

/// Whether the plan job's log says the run was complete ([`COMPLETE_LINE`]); see [`read_flag`].
pub fn read_complete(log: &str) -> Option<Result<bool, String>> {
    read_flag(log, COMPLETE_LINE)
}

/// Whether an app workflow's run was full, from its plan job's log: the [`MODE_LINE`] its Decide step prints; for a run from
/// before that line, a schedule or a push is full and a manual run says so in the runner's `Inputs` group (`bundles: true`).
/// An error when none of these tells it: the rerun then plans everything.
pub fn read_full(a: &AppWorkflow, event: &str, plan_log: Result<&str, &str>) -> Result<bool, String> {
    let log = plan_log.map_err(|e| format!("the plan job's log could not be read ({e})"))?;
    let lines: Vec<String> = log.lines().map(plain).collect();
    if let Some(mode) = read_marker(log) {
        return mode;
    }
    match event {
        "schedule" | "push" => return Ok(true),
        "pull_request" => return Ok(false),
        _ => {}
    }
    let mut inputs = false;
    for l in &lines {
        match l.trim() {
            "::group::Inputs" => inputs = true,
            "::endgroup::" if inputs => break,
            t if inputs => {
                if let Some(v) = t.strip_prefix(&format!("{}: ", a.full_input)) {
                    return Ok(v.trim() == "true");
                }
            }
            _ => {}
        }
    }
    Err(format!(
        "the plan job's log has no \"{MODE_LINE}\" line and no {} input, so whether run was full is unknown",
        a.full_input
    ))
}

/// The workflow file of a run's `path` (`ci.yml@refs/heads/main` is `ci.yml`; `ci.yaml` reads as `ci.yml`).
pub fn workflow_file(path: &str) -> String {
    let file = path.split('@').next().unwrap_or("");
    if file == "ci.yaml" { "ci.yml".into() } else { file.to_string() }
}

/// One failed test as nextest names it: the binary id and the test name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FailedTest {
    /// `paddock-service`, `pitwall-server::it`, `dyno-app::bin/dyno`.
    pub binary: String,
    /// `per_machine::tests::every_adopting_app_is_in_sync`.
    pub test: String,
}

impl std::fmt::Display for FailedTest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.binary, self.test)
    }
}

/// The failing tests of the nextest runs in a job log, or the reason the log cannot prove that they are all of them (the shared
/// reader, [`crate::nextest_log`]): a run that never printed its summary, one that did not run every test it started, fewer
/// names than a summary counts, or no failure at all. Flaky tests count: with `flaky-result = "fail"` they fail the step.
pub fn nextest_failures(log: &str) -> Result<Vec<FailedTest>, String> {
    let n = crate::nextest_log::read(log).map_err(|e| e.to_string())?;
    let all: BTreeSet<FailedTest> = n
        .failed
        .into_iter()
        .chain(n.flaky)
        .map(|(binary, test)| FailedTest { binary, test })
        .collect();
    if all.is_empty() {
        return Err("the step failed but no failing test was found in the log".into());
    }
    Ok(all.into_iter().collect())
}

static PLAIN_TEST: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_:]+$"));
static PLAIN_BINARY: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_:/.-]+$"));

/// The nextest filterset that selects exactly `tests` (`(binary_id(=b) & (test(=x) | test(=y))) | ...`), the whole binary for
/// one whose test names a filterset would have to escape, or `None` when a binary id itself cannot be written plainly.
pub fn filterset(tests: &[FailedTest]) -> Option<String> {
    let mut by_binary: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for t in tests {
        by_binary.entry(&t.binary).or_default().push(&t.test);
    }
    let mut parts: Vec<String> = Vec::new();
    for (binary, names) in by_binary {
        if !PLAIN_BINARY.is_match(binary) {
            return None;
        }
        if names.iter().all(|n| PLAIN_TEST.is_match(n)) {
            let tests: Vec<String> = names.iter().map(|n| format!("test(={n})")).collect();
            let tests = if tests.len() == 1 {
                tests.join("")
            } else {
                format!("({})", tests.join(" | "))
            };
            parts.push(format!("(binary_id(={binary}) & {tests})"));
        } else {
            parts.push(format!("binary_id(={binary})"));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" | "))
}

static VITEST_FAIL: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*FAIL\s+(?:\S+\s+)??(\S+\.(?:spec|test)\.[cm]?[jt]sx?)(?:\s|$)"));

/// The spec files vitest lists under `Failed Tests` (` FAIL  <project> <file> > suite > test`), in order, once each.
pub fn vitest_failed_files(log: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in log.lines() {
        if let Some(m) = VITEST_FAIL.captures(&plain(raw))
            && !out.iter().any(|f| f == &m[1])
        {
            out.push(m[1].to_string());
        }
    }
    out
}

static VITEST_SUMMARY: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*Test Files\s+(?:(\d+) failed)?"));
static PLAIN_SPEC: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_./-]+$"));

/// The failed spec files of the vitest run in a job log, or why the log cannot prove that they are all of them: exactly one
/// `Test Files` summary, whose failed count equals the files named under `Failed Tests`, every name a plain path.
pub fn vitest_failures(log: &str) -> Result<Vec<String>, String> {
    let summaries: Vec<Option<usize>> = log
        .lines()
        .filter_map(|l| {
            VITEST_SUMMARY
                .captures(&plain(l))
                .map(|m| m.get(1).and_then(|n| n.as_str().parse().ok()))
        })
        .collect();
    let [failed] = summaries.as_slice() else {
        return Err(format!("{} vitest summaries in the log, not one", summaries.len()));
    };
    let files = vitest_failed_files(log);
    let failed = failed.unwrap_or(0);
    if failed == 0 || files.is_empty() {
        return Err("the step failed but vitest names no failed spec file".into());
    }
    if failed != files.len() {
        return Err(format!("vitest counts {failed} failed files but the log names {}", files.len()));
    }
    if let Some(f) = files.iter().find(|f| !PLAIN_SPEC.is_match(f)) {
        return Err(format!("the spec file {f:?} cannot be passed plainly"));
    }
    Ok(files)
}

/// The workflow job key the runner printed at the top of a job log (`Job: crate`).
pub fn job_key(log: &str) -> Option<String> {
    static KEY: LazyLock<Regex> = LazyLock::new(|| re(r"^Job: ([A-Za-z0-9_-]+)\s*$"));
    log.lines().take(60).find_map(|l| KEY.captures(&plain(l)).map(|m| m[1].to_string()))
}

/// The `::error::` annotations of a log (the text after the prefix); an echoed script line is not one.
fn errors(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|l| plain(l).strip_prefix("::error::").map(str::to_string))
        .collect()
}

/// The job keys of a workflow and their `name:` templates, to tell a job by its display name when its log has no `Job:` line.
pub fn name_templates(workflow: &str) -> Vec<(String, String)> {
    static NAME: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^ {4}name: *(.+?)\s*$"));
    workflow_jobs(workflow)
        .map(|w| {
            w.jobs
                .into_iter()
                .map(|(k, text)| {
                    let name = NAME
                        .captures(&text)
                        .map(|m| m[1].trim_matches('"').to_string())
                        .unwrap_or_else(|| k.clone());
                    (k, name)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The job key whose `name:` template matches a display name (`${{ ... }}` matches anything), when exactly one does.
fn key_by_name(name: &str, templates: &[(String, String)]) -> Option<String> {
    static EXPR: LazyLock<Regex> = LazyLock::new(|| re(r"\$\{\{.*?\}\}"));
    let hits: Vec<&String> = templates
        .iter()
        .filter(|(_, t)| {
            let pattern = EXPR.split(t).map(regex::escape).collect::<Vec<_>>().join(".*");
            Regex::new(&format!("^{pattern}$")).is_ok_and(|r| r.is_match(name))
        })
        .map(|(k, _)| k)
        .collect();
    (hits.len() == 1).then(|| hits[0].clone())
}

/// The `needs:` of each job of a workflow (`needs: [plan, rust-lint]` or `needs: plan`), in file order.
pub fn job_needs(workflow: &str) -> Vec<(String, Vec<String>)> {
    static NEEDS: LazyLock<Regex> = LazyLock::new(|| re(r"(?m)^ {4}needs: *(?:\[([^\]]*)\]|([A-Za-z0-9_-]+))\s*$"));
    workflow_jobs(workflow)
        .map(|w| {
            w.jobs
                .into_iter()
                .map(|(k, text)| {
                    let needs = NEEDS
                        .captures(&text)
                        .map(|m| {
                            m.get(1)
                                .or(m.get(2))
                                .map_or("", |x| x.as_str())
                                .split(',')
                                .map(|n| n.trim().to_string())
                                .filter(|n| !n.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();
                    (k, needs)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The matrix values a display name shows through its `name:` template: `end-to-end (shard ${{ matrix.shard }}/4)` and
/// `end-to-end (shard 2/4)` give `shard = 2`. Other expressions match anything; empty when the name does not fit.
pub fn matrix_of(name: &str, template: &str) -> BTreeMap<String, String> {
    static EXPR: LazyLock<Regex> = LazyLock::new(|| re(r"\$\{\{.*?\}\}"));
    static MATRIX: LazyLock<Regex> = LazyLock::new(|| re(r"^\$\{\{\s*matrix\.([A-Za-z0-9_]+)\s*\}\}$"));
    let mut pattern = String::from("^");
    let mut names: Vec<String> = Vec::new();
    let mut at = 0;
    for m in EXPR.find_iter(template) {
        pattern.push_str(&regex::escape(&template[at..m.start()]));
        match MATRIX.captures(m.as_str()) {
            Some(c) if !names.contains(&c[1].to_string()) => {
                pattern.push_str(&format!("(?P<{}>.*?)", &c[1]));
                names.push(c[1].to_string());
            }
            _ => pattern.push_str(".*?"),
        }
        at = m.end();
    }
    pattern.push_str(&regex::escape(&template[at..]));
    pattern.push('$');
    let Ok(r) = Regex::new(&pattern) else {
        return BTreeMap::new();
    };
    r.captures(name)
        .map(|c| {
            names
                .iter()
                .filter_map(|n| c.name(n).map(|v| (n.clone(), v.as_str().to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// A matrix leg of a failed job, read from its display name (`crate (macos-latest)`, `app dyno (ubuntu-latest, e2e-1)`,
/// `end-to-end (shard 2/4)`). `None` fields are unknown: every leg of that kind reruns.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Leg {
    /// The runner label.
    pub os: Option<String>,
    /// The app of an `app` leg.
    pub app: Option<String>,
    /// The end-to-end leg of an `app` leg (`e2e-1`), `Some("")` for the main leg.
    pub e2e_leg: Option<String>,
    /// The shard of a sharded job of an app's workflow (`matrix.shard`).
    pub shard: Option<String>,
}

/// The leg of a job of an app's own workflow, from its `name:` template.
fn app_leg_of(key: &str, name: &str, templates: &[(String, String)]) -> Leg {
    let matrix = templates
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, t)| matrix_of(name, t))
        .unwrap_or_default();
    Leg {
        os: matrix.get("os").cloned(),
        shard: matrix.get("shard").cloned(),
        ..Leg::default()
    }
}

fn leg_of(key: &str, name: &str) -> Leg {
    static OS_LEG: LazyLock<Regex> = LazyLock::new(|| re(r"^[a-z-]+ \(([a-z0-9]+-[a-z0-9.]+)\)$"));
    static APP_LEG: LazyLock<Regex> = LazyLock::new(|| re(r"^app ([a-z0-9-]+) \(([a-z0-9]+-[a-z0-9.]+)(?:, ([a-z0-9-]+))?\)$"));
    match key {
        "crate" | "paddock-dev" => Leg {
            os: OS_LEG.captures(name).map(|m| m[1].to_string()),
            ..Leg::default()
        },
        "app" => APP_LEG.captures(name).map_or_else(Leg::default, |m| Leg {
            os: Some(m[2].to_string()),
            app: Some(m[1].to_string()),
            e2e_leg: Some(m.get(3).map_or("", |x| x.as_str()).to_string()),
            shard: None,
        }),
        _ => Leg::default(),
    }
}

/// What reruns of a failed job.
#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    /// The whole job, and why it could not be narrowed.
    Job(String),
    /// Only these tests, through this filterset.
    Tests {
        /// The nextest filterset.
        filter: String,
        /// The failed tests it selects.
        tests: Vec<FailedTest>,
    },
    /// Only these vitest spec files (the step's file list).
    Specs(Vec<String>),
}

/// A job of a run as the Gitea API lists it.
#[derive(Debug, Clone)]
pub struct RunJob {
    /// The job's id.
    pub id: u64,
    /// Its display name.
    pub name: String,
    /// `success`, `failure`, `cancelled`, `skipped`...
    pub conclusion: String,
    /// The names of its failed steps.
    pub failed_steps: Vec<String>,
}

impl RunJob {
    /// The jobs of a `GET .../actions/runs/<id>/jobs` answer.
    pub fn list(body: &Value) -> Vec<RunJob> {
        body["jobs"]
            .as_array()
            .map(|jobs| {
                jobs.iter()
                    .map(|j| RunJob {
                        id: j["id"].as_u64().unwrap_or(0),
                        name: j["name"].as_str().unwrap_or("").to_string(),
                        conclusion: j["conclusion"].as_str().or(j["status"].as_str()).unwrap_or("").to_string(),
                        failed_steps: j["steps"]
                            .as_array()
                            .map(|s| {
                                s.iter()
                                    .filter(|s| s["conclusion"] == "failure")
                                    .filter_map(|s| s["name"].as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the job needs a rerun: anything but a pass or a skip (a failure, a cancel, a job that never finished).
    pub fn failed(&self) -> bool {
        !matches!(self.conclusion.as_str(), "success" | "skipped")
    }
}

/// The decision for one failed job.
#[derive(Debug, Clone, PartialEq)]
pub struct Rerun {
    /// The run's job id.
    pub id: u64,
    /// Its display name.
    pub name: String,
    /// The workflow job key, `None` when neither the log nor the name tells it.
    pub key: Option<String>,
    /// The matrix leg.
    pub leg: Leg,
    /// What reruns.
    pub scope: Scope,
    /// The spec files vitest reported failed (for the reader; no step takes them yet).
    pub vitest: Vec<String>,
}

/// Decides what of a failed job reruns. `log` is the job's log or why it could not be read; `workflow` is the run's workflow
/// file (only the steps of [`TEST_STEPS`] and [`VITEST_STEPS`] are known to take a filter); `templates` are the workflow's
/// `name:` lines.
pub fn classify(job: &RunJob, log: Result<&str, &str>, workflow: &str, templates: &[(String, String)]) -> Rerun {
    let key = log.ok().and_then(job_key).or_else(|| key_by_name(&job.name, templates));
    let leg = match key.as_deref() {
        Some(k) if workflow == "ci.yml" => leg_of(k, &job.name),
        Some(k) => app_leg_of(k, &job.name, templates),
        None => Leg::default(),
    };
    let vitest = log.map(vitest_failed_files).unwrap_or_default();
    let scope = narrow(job, log, workflow, key.as_deref()).unwrap_or_else(Scope::Job);
    Rerun {
        id: job.id,
        name: job.name.clone(),
        key,
        leg,
        scope,
        vitest,
    }
}

fn narrow(job: &RunJob, log: Result<&str, &str>, workflow: &str, key: Option<&str>) -> Result<Scope, String> {
    let log = log.map_err(|e| format!("its log could not be read ({e})"))?;
    if job.conclusion != "failure" {
        return Err(format!(
            "it ended {}",
            if job.conclusion.is_empty() { "unknown" } else { &job.conclusion }
        ));
    }
    let key = key.ok_or("its workflow job is unknown")?;
    if job.failed_steps.is_empty()
        && (TEST_STEPS.iter().any(|t| t.0 == workflow && t.1 == key) || VITEST_STEPS.iter().any(|t| t.0 == workflow && t.1 == key))
    {
        return Err("the run lists no failed step".into());
    }
    if let Some((_, _, steps)) = VITEST_STEPS.iter().find(|(w, k, _)| *w == workflow && *k == key) {
        if let Some(s) = job.failed_steps.iter().find(|s| !steps.contains(&s.as_str())) {
            return Err(format!("step \"{s}\" failed and takes no spec file list"));
        }
        return vitest_failures(log).map(Scope::Specs);
    }
    let entry = TEST_STEPS.iter().find(|(w, k, _, _)| *w == workflow && *k == key);
    let Some((_, _, steps, composite)) = entry else {
        return Err(format!("job {key} has no step that takes a test filter"));
    };
    if let Some(s) = job.failed_steps.iter().find(|s| !steps.contains(&s.as_str())) {
        return Err(format!("step \"{s}\" failed and takes no test filter"));
    }
    if *composite {
        // The step that failed inside the composite and the composite itself; a third (notices.sh, the cargo-about install,
        // which print no ::error:: line) or a missing one means another step failed too.
        let failed = log
            .lines()
            .filter(|l| plain(l).starts_with("##[error]Process completed with exit code"))
            .count();
        if failed != 2 {
            return Err(format!(
                "{failed} failed steps in the log: app-gates is narrowed only when its Rust step alone failed"
            ));
        }
        let errs = errors(log);
        if errs.is_empty() {
            return Err("the step failed without naming a failed command".into());
        }
        static NEXTEST_ERROR: LazyLock<Regex> = LazyLock::new(|| re(r"^failed \(exit \d+\): (?:\S+ )*?cargo nextest run "));
        if let Some(e) = errs.iter().find(|e| !NEXTEST_ERROR.is_match(e)) {
            let short: String = e.chars().take(120).collect();
            return Err(format!("a command other than nextest failed ({short})"));
        }
    }
    let tests = nextest_failures(log)?;
    let filter = filterset(&tests).ok_or("a test binary's name cannot be written in a filterset")?;
    Ok(Scope::Tests { filter, tests })
}

/// The plan for a run: what reruns, and how ci.yml's plan job hands it to its jobs.
#[derive(Debug)]
pub struct SincePlan {
    /// The run.
    pub run: u64,
    /// Its workflow file (`ci.yml`).
    pub workflow: String,
    /// The commit it tested.
    pub head: String,
    /// The failed jobs.
    pub reruns: Vec<Rerun>,
    /// How many jobs passed or were skipped (not rerun).
    pub not_rerun: usize,
    /// Why everything reruns instead, when the failures cannot be told job by job.
    pub everything: Option<String>,
    /// Whether the run was a full run of an app's workflow ([`AppWorkflow`]), read by [`read_full`] from its plan job's log. A
    /// rerun of its full-run jobs must be full too.
    pub full_ran: bool,
}

impl SincePlan {
    /// The plan from a run's details, its job list and a reader of job logs.
    pub fn new(
        run: &Value,
        jobs: &[RunJob],
        templates: &[(String, String)],
        log: &mut dyn FnMut(u64) -> Result<String, String>,
    ) -> SincePlan {
        let path = run["path"].as_str().unwrap_or("");
        let workflow = workflow_file(path);
        let ci = workflow == "ci.yml";
        let mut reruns = Vec::new();
        for j in jobs.iter().filter(|j| j.failed()) {
            let text = log(j.id);
            reruns.push(classify(j, text.as_deref().map_err(String::as_str), &workflow, templates));
        }
        let everything = if let Some(r) = reruns.iter().find(|r| r.key.as_deref() == Some("plan")) {
            Some(format!("the plan job failed ({}): nothing after it ran", r.name))
        } else if ci {
            // A job the plan's outputs cannot gate (unknown, or added after this planner) cannot be rerun on its own.
            reruns
                .iter()
                .find(|r| !r.key.as_deref().is_some_and(|k| CI_JOBS.iter().any(|(j, _)| *j == k)))
                .map(|r| format!("job \"{}\" is not a job of this ci.yml", r.name))
        } else {
            // An app's workflow: its jobs are the file's own; one this checkout does not have cannot be gated.
            reruns
                .iter()
                .find(|r| !r.key.as_deref().is_some_and(|k| templates.iter().any(|(j, _)| j == k)))
                .map(|r| format!("job \"{}\" is not a job of this {workflow}", r.name))
        };
        let mut everything = everything;
        let mut full_ran = false;
        if let Some(a) = APP_WORKFLOWS.iter().find(|a| a.file == workflow) {
            let plan_job = jobs.iter().find(|j| key_by_name(&j.name, templates).as_deref() == Some("plan"));
            let text = match plan_job {
                Some(j) => log(j.id),
                None => Err("the run has no plan job".into()),
            };
            match read_full(a, run["event"].as_str().unwrap_or(""), text.as_deref().map_err(String::as_str)) {
                Ok(f) => full_ran = f,
                Err(e) => {
                    everything.get_or_insert(format!("whether run {} was full cannot be told: {e}", run["id"]));
                }
            }
        }
        SincePlan {
            run: run["id"].as_u64().unwrap_or(0),
            workflow,
            head: run["head_sha"].as_str().unwrap_or("").to_string(),
            not_rerun: jobs.len() - reruns.len(),
            reruns,
            everything,
            full_ran,
        }
    }

    /// Adds the jobs that were skipped because a job they `need` failed (Debrief's installers and real-app smoke after a failed
    /// `rust-lint`): they never ran, so they rerun whole after it. Repeated until nothing is added; the plan job is not a
    /// reason (its failure plans everything).
    pub fn add_skipped_dependents(&mut self, jobs: &[RunJob], templates: &[(String, String)], needs: &[(String, Vec<String>)]) {
        let never: &[&str] = APP_WORKFLOWS
            .iter()
            .find(|a| a.file == self.workflow)
            .map_or(&[], |a| a.never_manual);
        loop {
            let failed: BTreeSet<String> = self.reruns.iter().filter_map(|r| r.key.clone()).collect();
            let mut added = false;
            for j in jobs.iter().filter(|j| j.conclusion == "skipped") {
                if self.reruns.iter().any(|r| r.id == j.id) {
                    continue;
                }
                let Some(key) = key_by_name(&j.name, templates).filter(|k| !never.contains(&k.as_str())) else {
                    continue;
                };
                let Some(need) = needs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .and_then(|(_, n)| n.iter().find(|n| *n != "plan" && failed.contains(*n)))
                else {
                    continue;
                };
                self.reruns.push(Rerun {
                    id: j.id,
                    name: j.name.clone(),
                    leg: app_leg_of(&key, &j.name, templates),
                    key: Some(key),
                    scope: Scope::Job(format!("it did not run: {need}, which it needs, failed")),
                    vitest: Vec::new(),
                });
                added = true;
            }
            if !added {
                break;
            }
        }
        self.not_rerun = jobs.len().saturating_sub(self.reruns.len());
    }

    /// Whether this rerun must be a full run: the earlier run was full and a job that only a full run has, or that a full run
    /// changes, reruns.
    pub fn needs_full(&self) -> bool {
        APP_WORKFLOWS.iter().find(|a| a.file == self.workflow).is_some_and(|a| {
            self.full_ran
                && self.reruns.iter().any(|r| {
                    r.key
                        .as_deref()
                        .is_some_and(|k| a.full_only.contains(&k) || a.full_changes.contains(&k))
                })
        })
    }

    /// An app workflow's plan job outputs for this rerun (only without [`SincePlan::everything`]):
    ///
    /// - `rerun_jobs`: the job keys that rerun (each job's `if` reads an empty list as its own name);
    /// - `<job>_filter`: the nextest filterset of a job narrowed to its failed tests (`rust-test` is `rust_test_filter`);
    /// - `<job>_include`: the spec files, space separated, of a job narrowed to its failed vitest files;
    /// - `<job>_shards`: the failed shards of a sharded job ([`AppWorkflow::shard_jobs`]) as matrix legs (`[{"shard": 3}]`), written
    ///   only when every failed leg's shard is known
    ///   (otherwise the plan step writes every shard).
    pub fn app_outputs(&self) -> Vec<(String, String)> {
        let keys: BTreeSet<&str> = self.reruns.iter().filter_map(|r| r.key.as_deref()).collect();
        let mut out = vec![("rerun_jobs".to_string(), serde_json::to_string(&keys).unwrap_or_default())];
        for key in &keys {
            let runs: Vec<&Rerun> = self.reruns.iter().filter(|r| r.key.as_deref() == Some(key)).collect();
            let name = key.replace('-', "_");
            // One filter per job: a job with legs reruns them all with it, so it is narrowed only when it has one leg.
            if let [one] = runs.as_slice() {
                match &one.scope {
                    Scope::Tests { filter, .. } => out.push((format!("{name}_filter"), filter.clone())),
                    Scope::Specs(files) => out.push((format!("{name}_include"), files.join(" "))),
                    Scope::Job(_) => {}
                }
            }
            let sharded = APP_WORKFLOWS.iter().any(|a| a.file == self.workflow && a.shard_jobs.contains(key));
            if sharded && runs.iter().all(|r| r.leg.shard.is_some()) {
                let mut shards: Vec<Value> = Vec::new();
                for r in &runs {
                    let s = r.leg.shard.as_deref().unwrap_or("");
                    let v = s.parse::<u64>().map_or_else(|_| json!({ "shard": s }), |n| json!({ "shard": n }));
                    if !shards.contains(&v) {
                        shards.push(v);
                    }
                }
                out.push((format!("{name}_shards"), serde_json::to_string(&shards).unwrap_or_default()));
            }
        }
        out
    }

    /// Whether the rerun needs the WebKit legs in the matrix: the WebKit job failed, an app's WebKit leg failed, or an app leg
    /// that cannot be told failed (it may be the WebKit one). Without them a failed WebKit leg would not be found to rerun.
    pub fn needs_webkit(&self) -> bool {
        self.reruns.iter().any(|r| match r.key.as_deref() {
            Some("e2e-webkit") => true,
            Some("app") => r.leg.e2e_leg.as_deref().is_none_or(|e| e == "e2e-webkit"),
            _ => false,
        })
    }

    /// The plan as JSON (`dev plan --since-run`).
    pub fn to_json(&self) -> Value {
        let jobs: Vec<Value> = self
            .reruns
            .iter()
            .map(|r| {
                let mut o = Map::new();
                o.insert("id".into(), json!(r.id));
                o.insert("name".into(), json!(r.name));
                o.insert("job".into(), json!(r.key));
                if let Some(os) = &r.leg.os {
                    o.insert("os".into(), json!(os));
                }
                if let Some(app) = &r.leg.app {
                    o.insert("app".into(), json!(app));
                }
                if let Some(e2e) = r.leg.e2e_leg.as_ref().filter(|e| !e.is_empty()) {
                    o.insert("e2eLeg".into(), json!(e2e));
                }
                match &r.scope {
                    Scope::Job(why) => {
                        o.insert("scope".into(), json!("job"));
                        o.insert("reason".into(), json!(why));
                    }
                    Scope::Tests { filter, tests } => {
                        o.insert("scope".into(), json!("tests"));
                        o.insert("filter".into(), json!(filter));
                        o.insert("tests".into(), json!(tests.iter().map(ToString::to_string).collect::<Vec<_>>()));
                    }
                    Scope::Specs(files) => {
                        o.insert("scope".into(), json!("specs"));
                        o.insert("specs".into(), json!(files));
                    }
                }
                if let Some(shard) = &r.leg.shard {
                    o.insert("shard".into(), json!(shard));
                }
                if !r.vitest.is_empty() {
                    o.insert("vitest".into(), json!(r.vitest));
                }
                Value::Object(o)
            })
            .collect();
        json!({
            "run": self.run,
            "workflow": self.workflow,
            "head": self.head,
            "everything": self.everything,
            "fullRan": self.full_ran,
            "jobs": jobs,
            "notRerun": self.not_rerun,
        })
    }

    /// ci.yml's plan job outputs for this rerun. `legs` is the matrix of every app (the plan for everything): the failed app
    /// legs are taken from it, a narrowed one with its filter and only its tests (`tests_only`). Only for a ci.yml run without
    /// [`SincePlan::everything`]. The warnings (a failed leg that today's matrix no longer has) go to `warn`.
    pub fn ci_outputs(&self, legs: &[Value], warn: &mut dyn FnMut(String)) -> Vec<(String, String)> {
        let keys: BTreeSet<&str> = self.reruns.iter().filter_map(|r| r.key.as_deref()).collect();
        // One boolean output per project of the job table (`paddock-dev` is `paddock_dev`); `apps` is the legs below.
        let mut projects: Vec<&str> = Vec::new();
        for (_, ps) in CI_JOBS {
            for p in ps.iter().filter(|p| !matches!(**p, "everything" | "apps")) {
                if !projects.contains(p) {
                    projects.push(p);
                }
            }
        }
        let mut out: Vec<(String, String)> = projects
            .iter()
            .map(|p| {
                let on = keys.iter().any(|k| CI_JOBS.iter().any(|(j, ps)| j == k && ps.contains(p)));
                (p.replace('-', "_"), on.to_string())
            })
            .collect();
        let mut app_legs: Vec<Value> = Vec::new();
        for r in self.reruns.iter().filter(|r| r.key.as_deref() == Some("app")) {
            let matches = |l: &Value| {
                let is = |field: &str, want: &Option<String>| want.as_ref().is_none_or(|w| l[field].as_str() == Some(w.as_str()));
                is("app", &r.leg.app) && is("os", &r.leg.os) && is("e2e_leg", &r.leg.e2e_leg)
            };
            let mut found: Vec<Value> = legs.iter().filter(|l| matches(l)).cloned().collect();
            if found.is_empty() {
                let app = r.leg.app.clone();
                warn(format!(
                    "::warning::the failed leg \"{}\" is not in today's matrix; every leg of {} reruns",
                    r.name,
                    app.as_deref().unwrap_or("every app")
                ));
                found = legs
                    .iter()
                    .filter(|l| app.as_ref().is_none_or(|a| l["app"].as_str() == Some(a.as_str())))
                    .cloned()
                    .collect();
            }
            for mut l in found {
                if let Scope::Tests { filter, .. } = &r.scope {
                    for (k, v) in [
                        ("filter", json!(filter)),
                        ("tests_only", json!("true")),
                        ("checks", json!("")),
                        ("rust_checks", json!("")),
                        ("ui_checks", json!("")),
                        ("ui", json!(false)),
                        ("notices", json!("false")),
                    ] {
                        l[k] = v;
                    }
                }
                if !app_legs.contains(&l) {
                    app_legs.push(l);
                }
            }
        }
        let os_legs = |key: &str| -> String {
            let runs: Vec<&Rerun> = self.reruns.iter().filter(|r| r.key.as_deref() == Some(key)).collect();
            if runs.is_empty() || runs.iter().any(|r| r.leg.os.is_none()) {
                return String::new(); // not rerun, or a leg that cannot be told: the plan step writes every leg, whole
            }
            let legs: Vec<Value> = runs
                .iter()
                .map(|r| {
                    let filter = match &r.scope {
                        Scope::Tests { filter, .. } => filter.clone(),
                        Scope::Job(_) | Scope::Specs(_) => String::new(),
                    };
                    json!({ "os": r.leg.os, "filter": filter })
                })
                .collect();
            serde_json::to_string(&legs).unwrap_or_default()
        };
        out.extend([
            ("apps".into(), serde_json::to_string(&app_legs).unwrap_or_default()),
            ("any_app".into(), (!app_legs.is_empty()).to_string()),
            // The commit lint, when it reruns, lints what came after the failed run.
            ("base".into(), self.head.clone()),
            ("rerun_jobs".into(), serde_json::to_string(&keys).unwrap_or_default()),
        ]);
        // Written only for a job that reruns with known legs; otherwise the plan step writes every OS (ci.yml).
        for (name, key) in [("crate_legs", "crate"), ("paddock_dev_legs", "paddock-dev")] {
            let legs = os_legs(key);
            if !legs.is_empty() {
                out.push((name.into(), legs));
            }
        }
        out
    }

    /// A Markdown summary for the job summary.
    pub fn summary(&self) -> String {
        let mut lines = vec![
            format!("### Rerun of run {} ({})", self.run, self.workflow),
            String::new(),
            format!(
                "{} failed job(s) rerun, {} passed or skipped job(s) not rerun (ER-10). A full run before landing catches the rest.",
                self.reruns.len(),
                self.not_rerun
            ),
            String::new(),
            "| Job | Reruns |".into(),
            "|---|---|".into(),
        ];
        for r in &self.reruns {
            let what = match &r.scope {
                Scope::Job(why) => format!("the whole job: {why}"),
                Scope::Tests { tests, .. } => format!("{} failed test(s) only", tests.len()),
                Scope::Specs(files) => format!("{} failed spec file(s) only", files.len()),
            };
            lines.push(format!("| {} | {what} |", r.name.replace('|', "/")));
        }
        format!("{}\n", lines.join("\n"))
    }
}

/// Reads a run through the Gitea API (`/api/v1/repos/<repo>/actions/...`). The token travels in a header only.
pub struct GiteaRuns<'a> {
    http: &'a mut dyn Http,
    api: String,
    token: String,
}

impl<'a> GiteaRuns<'a> {
    /// `server` like `https://dev.decryptology.net`, `repo` like `decryp7/paddock`.
    pub fn new(http: &'a mut dyn Http, server: &str, repo: &str, token: String) -> GiteaRuns<'a> {
        GiteaRuns {
            http,
            api: format!("{}/api/v1/repos/{repo}/actions", server.trim_end_matches('/')),
            token,
        }
    }

    /// The run's details (`status`, `path`, `head_sha`).
    pub fn run(&mut self, id: u64) -> Result<Value, String> {
        self.http.get_json(&format!("{}/runs/{id}", self.api), &self.token)
    }

    /// Every job of the run, page by page (the server caps a page at 50).
    pub fn jobs(&mut self, id: u64) -> Result<Vec<RunJob>, String> {
        let mut out = Vec::new();
        for page in 1..=20 {
            let body = self
                .http
                .get_json(&format!("{}/runs/{id}/jobs?limit=50&page={page}", self.api), &self.token)?;
            let jobs = RunJob::list(&body);
            let got = jobs.len();
            out.extend(jobs);
            let total = body["total_count"].as_u64().unwrap_or(0) as usize;
            if got == 0 || out.len() >= total {
                return Ok(out);
            }
        }
        Err("the run has more than 1000 jobs".into())
    }

    /// A job's log.
    pub fn log(&mut self, job: u64) -> Result<String, String> {
        self.http.get_text(&format!("{}/jobs/{job}/logs", self.api), &self.token)
    }
}

/// Reads the run, decides per failed job, and logs each phase (`log`). An unfinished run is an error: a rerun of a run still
/// going would miss what has not failed yet.
pub fn since_run(
    runs: &mut GiteaRuns,
    id: u64,
    workflow_text: &dyn Fn(&str) -> Option<String>,
    log: &mut dyn FnMut(&str),
) -> Result<SincePlan, String> {
    let run = runs.run(id).map_err(|e| format!("run {id} could not be read: {e}"))?;
    let status = run["status"].as_str().unwrap_or("unknown");
    if !matches!(status, "completed" | "success" | "failure" | "cancelled" | "skipped") {
        return Err(format!(
            "run {id} is {status}: wait until it finishes (`./dev wait --ci-run {id} --max 2h`), then plan its rerun"
        ));
    }
    let jobs = runs.jobs(id).map_err(|e| format!("the jobs of run {id} could not be read: {e}"))?;
    let path = run["path"].as_str().unwrap_or("");
    let file = path.split('@').next().unwrap_or("");
    let text = workflow_text(file);
    let templates = text.as_deref().map(name_templates).unwrap_or_default();
    let failed = jobs.iter().filter(|j| j.failed()).count();
    log(&format!("run {id} ({path}): {} jobs, {failed} failed", jobs.len()));
    let started = std::time::Instant::now();
    let plan = SincePlan::new(&run, &jobs, &templates, &mut |job| {
        let text = runs.log(job);
        match &text {
            Ok(t) => log(&format!("job {job}: log read ({} bytes)", t.len())),
            Err(e) => log(&format!(
                "::warning::job {job}: its log could not be read ({e}); the whole job reruns"
            )),
        }
        text
    });
    log(&format!(
        "{failed} job logs read and decided in {:.1} s",
        started.elapsed().as_secs_f64()
    ));
    let mut plan = plan;
    if plan.everything.is_none() {
        let before = plan.reruns.len();
        plan.add_skipped_dependents(&jobs, &templates, &text.as_deref().map(job_needs).unwrap_or_default());
        if plan.reruns.len() > before {
            log(&format!(
                "{} skipped job(s) rerun after the jobs they need",
                plan.reruns.len() - before
            ));
        }
    }
    for r in &plan.reruns {
        match &r.scope {
            Scope::Job(why) => log(&format!("{}: the whole job reruns: {why}", r.name)),
            Scope::Tests { tests, filter } => log(&format!("{}: {} failed test(s) rerun: {filter}", r.name, tests.len())),
            Scope::Specs(files) => log(&format!(
                "{}: {} failed spec file(s) rerun: {}",
                r.name,
                files.len(),
                files.join(" ")
            )),
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_markers_are_read_from_the_start_of_a_line_and_the_first_line_wins() {
        assert_eq!(read_marker("2026-11-12T00:00:00Z plan mode: full=true\n"), Some(Ok(true)));
        // A mention in the middle of a line is not the marker.
        assert_eq!(read_marker("echo plan mode: full=true\n"), None);
        assert_eq!(read_marker("x plan mode: full=true"), None);
        // The first marker wins, whatever follows.
        assert_eq!(read_marker("plan mode: full=false\nplan mode: full=true\n"), Some(Ok(false)));
        assert_eq!(
            read_complete("plan verdict: complete=true\nplan verdict: complete=false\n"),
            Some(Ok(true))
        );
        assert_eq!(read_complete("the plan verdict: complete=true\n"), None);
        assert!(matches!(read_complete("plan verdict: complete=maybe\n"), Some(Err(_))));
        // A value that is neither true nor false is an error, not false.
        assert!(matches!(read_marker("plan mode: full=maybe\n"), Some(Err(_))));
        // The two markers do not read each other.
        assert_eq!(read_complete("plan mode: full=true\n"), None);
        assert_eq!(read_marker("plan verdict: complete=true\n"), None);
    }

    use super::*;

    const CRATE_MACOS: &str = include_str!("../tests/since-run/crate-macos.log");
    const CRATE_WINDOWS: &str = include_str!("../tests/since-run/crate-windows.log");
    const APP_DYNO: &str = include_str!("../tests/since-run/app-dyno-macos.log");
    const APP_PITWALL: &str = include_str!("../tests/since-run/app-pitwall-windows.log");
    const VITEST: &str = include_str!("../tests/since-run/vitest-frontend.log");
    const UI_SCRIPTS: &str = include_str!("../tests/since-run/ui-script-tests.log");
    const JOBS: &str = include_str!("../tests/since-run/run-7821-jobs.json");

    fn t(binary: &str, test: &str) -> FailedTest {
        FailedTest {
            binary: binary.into(),
            test: test.into(),
        }
    }

    #[test]
    fn nextest_failures_are_read_from_the_coloured_log_of_every_run_once_each() {
        assert_eq!(
            nextest_failures(CRATE_MACOS).unwrap(),
            vec![t("paddock-service", "per_machine::tests::every_adopting_app_is_in_sync")]
        );
        // A timed out test is a failure.
        assert_eq!(
            nextest_failures(APP_DYNO).unwrap(),
            vec![t(
                "dyno-app",
                "link::model::a_diverging_search_hits_the_ci_cap_in_seconds_and_every_real_scenario_stays_well_inside_it"
            )]
        );
        // 103 failures in unit and integration binaries; the summary's count is met.
        let pitwall = nextest_failures(APP_PITWALL).unwrap();
        assert_eq!(pitwall.len(), 103);
        assert!(pitwall.contains(&t("pitwall-server", "auth::tests::keys_authenticate_and_revoke")));
        assert!(pitwall.contains(&t("pitwall-server::it", "caching::affinity_can_be_switched_off")));
        // Two runs (every feature, features off): the union, the shared ones once.
        let windows = nextest_failures(CRATE_WINDOWS).unwrap();
        assert_eq!(windows.len(), 9);
        assert!(windows.contains(&t("paddock-secrets", "file::tests::a_secret_round_trips_and_deletes")));
    }

    #[test]
    fn a_log_that_cannot_prove_every_failure_was_read_is_refused_with_the_reason() {
        let stamp = "2026-01-01T00:00:00.0000000Z ";
        let start = format!("{stamp}    Starting 3 tests across 1 binaries\n");
        let fail = format!("{stamp}        FAIL [   0.1s] (1/3) a-crate tests::one\n");
        let summary = |ran: u32, failed: u32| {
            format!(
                "{stamp}     Summary [   1.0s] {ran} tests run: {} passed, {failed} failed, 0 skipped\n",
                ran - failed
            )
        };
        let cases: Vec<(String, &str)> = vec![
            (String::new(), "no nextest run"),
            (format!("{start}{fail}"), "without its summary"),
            (
                format!("{start}{fail}error: could not compile\n{start}{}", summary(3, 1)),
                "without its summary",
            ),
            (format!("{start}{fail}{}", summary(2, 1)), "started 3 tests but ran 2"),
            (format!("{start}{fail}{}", summary(3, 2)), "counts 2 failures but the log names 1"),
            (format!("{start}{}", summary(3, 0)), "no failing test"),
            (
                format!(
                    "{start}{fail}{}{stamp}warning: 2/3 tests were not run due to test failure\n",
                    summary(3, 1)
                ),
                "stopped before every test ran",
            ),
            (
                format!("{start}{fail}{stamp}Cancelling due to test failure\n{}", summary(3, 1)),
                "stopped before",
            ),
        ];
        for (log, want) in cases {
            let e = nextest_failures(&log).unwrap_err();
            assert!(e.contains(want), "{want:?} in {e:?} for {log:?}");
        }
        // Retries: a test that failed on a try and one reported flaky are both failures to rerun.
        let retried = format!(
            "{start}{stamp}   TRY 1 FAIL [   0.1s] (1/3) a-crate tests::one\n{stamp}       FLAKY 2/2 [   0.2s] (2/3) a-crate tests::two\n{}",
            summary(3, 1)
        );
        assert_eq!(
            nextest_failures(&retried).unwrap(),
            vec![t("a-crate", "tests::one"), t("a-crate", "tests::two")]
        );
    }

    #[test]
    fn the_filterset_names_each_failed_test_exactly_and_a_whole_binary_for_an_odd_name() {
        assert_eq!(filterset(&[]), None);
        assert_eq!(filterset(&[t("b", "x::y")]).unwrap(), "(binary_id(=b) & test(=x::y))");
        assert_eq!(
            filterset(&[t("b::it", "m::one"), t("a", "z"), t("b::it", "m::two")]).unwrap(),
            "(binary_id(=a) & test(=z)) | (binary_id(=b::it) & (test(=m::one) | test(=m::two)))"
        );
        // A case of a parameterised test can have characters the filterset would have to escape: its whole binary.
        assert_eq!(filterset(&[t("b", "case(1, 2)")]).unwrap(), "binary_id(=b)");
        assert_eq!(filterset(&[t("b c)", "x")]), None);
    }

    #[test]
    fn vitest_failed_files_come_from_the_failed_tests_list_and_the_job_key_from_the_header() {
        assert_eq!(
            vitest_failed_files(VITEST),
            vec!["src/app/analysis/outliers-view.spec.ts", "src/app/watched/watched-folders.spec.ts"]
        );
        assert!(vitest_failed_files(CRATE_MACOS).is_empty());
        assert_eq!(job_key(CRATE_MACOS).as_deref(), Some("crate"));
        assert_eq!(job_key(APP_DYNO).as_deref(), Some("app"));
        assert_eq!(job_key("no header"), None);
    }

    fn jobs() -> Vec<RunJob> {
        RunJob::list(&serde_json::from_str(JOBS).unwrap())
    }

    fn job(id: u64) -> RunJob {
        jobs().into_iter().find(|j| j.id == id).unwrap()
    }

    fn templates() -> Vec<(String, String)> {
        let ci = "name: ci\non:\n  push:\njobs:\n  plan:\n    name: plan (affected projects)\n  ui:\n    name: ui (tokens, contrast, components, entry points, publish dry run)\n  e2e:\n    name: reference apps (shard ${{ matrix.shard }}/2)\n  conformance:\n    name: conformance (checks, release scripts)\n  crates:\n    name: crates (fmt, workspace hack, unused deps, coverage, deny)\n  crate:\n    name: crate (${{ matrix.os }})\n  app:\n    name: app ${{ matrix.app }} (${{ matrix.os }}${{ matrix.e2e_leg != '' && format(', {0}', matrix.e2e_leg) || '' }})\n  paddock-dev:\n    name: paddock-dev (${{ matrix.os }})\n";
        name_templates(ci)
    }

    #[test]
    fn a_job_failed_only_in_its_test_step_reruns_its_failed_tests_and_any_other_failure_the_whole_job() {
        let mac = classify(&job(33271), Ok(CRATE_MACOS), "ci.yml", &templates());
        assert_eq!(mac.key.as_deref(), Some("crate"));
        assert_eq!(mac.leg.os.as_deref(), Some("macos-latest"));
        assert_eq!(
            mac.scope,
            Scope::Tests {
                filter: "(binary_id(=paddock-service) & test(=per_machine::tests::every_adopting_app_is_in_sync))".into(),
                tests: vec![t("paddock-service", "per_machine::tests::every_adopting_app_is_in_sync")],
            }
        );
        // Windows also failed in the features-off step, whose first failed command stopped the ones after it: whole job.
        let win = classify(&job(33273), Ok(CRATE_WINDOWS), "ci.yml", &templates());
        assert_eq!(
            win.scope,
            Scope::Job("step \"Tests with the optional features off\" failed and takes no test filter".into())
        );
        // app-gates: only nextest failed, so the leg's failed tests.
        let dyno = classify(&job(33275), Ok(APP_DYNO), "ci.yml", &templates());
        assert_eq!(
            dyno.leg,
            Leg {
                os: Some("macos-latest".into()),
                app: Some("dyno".into()),
                e2e_leg: Some(String::new()),
                shard: None,
            }
        );
        assert!(matches!(dyno.scope, Scope::Tests { ref tests, .. } if tests.len() == 1));
        // Another command of app-gates failed too: whole leg.
        let doc = format!("{APP_DYNO}2026-11-09T07:41:30.0000000Z ::error::failed (exit 101): cargo test --workspace --doc\n");
        assert!(
            matches!(classify(&job(33275), Ok(&doc), "ci.yml", &templates()).scope, Scope::Job(ref w) if w.contains("other than nextest"))
        );
        // A job without a test step (the ui job's node --test scripts), an unreadable log, a run of another workflow.
        let ui = classify(&job(33261), Ok(UI_SCRIPTS), "ci.yml", &templates());
        assert_eq!(ui.key.as_deref(), Some("ui"));
        assert_eq!(ui.scope, Scope::Job("job ui has no step that takes a test filter".into()));
        let lost = classify(&job(33271), Err("HTTP 404"), "ci.yml", &templates());
        assert_eq!(lost.key.as_deref(), Some("crate"), "the name tells the job when the log is missing");
        assert_eq!(lost.scope, Scope::Job("its log could not be read (HTTP 404)".into()));
        assert!(matches!(
            classify(&job(33271), Ok(CRATE_MACOS), "debrief.yml", &templates()).scope,
            Scope::Job(_)
        ));
    }

    fn run_7821() -> Value {
        json!({"id": 7821, "status": "completed", "path": "ci.yml@refs/heads/train/x", "head_sha": "f1d7e0791f250c88dacd318f7caebf1dcf066cdf"})
    }

    fn logs(id: u64) -> Result<String, String> {
        Ok(match id {
            33271 => CRATE_MACOS,
            33273 => CRATE_WINDOWS,
            33275 => APP_DYNO,
            33303 => APP_PITWALL,
            33261 => UI_SCRIPTS,
            _ => return Err("not in the fixture".into()),
        }
        .to_string())
    }

    fn leg(app: &str, os: &str, e2e: &str) -> Value {
        json!({"app": app, "os": os, "e2e_leg": e2e, "filter": "", "checks": "x", "rust_checks": "y", "ui_checks": "z", "ui": true, "notices": "true"})
    }

    #[test]
    fn the_plan_of_run_7821_reruns_the_sixteen_failed_jobs_and_none_of_the_passed_ones() {
        let jobs = jobs();
        assert_eq!(jobs.len(), 39);
        let plan = SincePlan::new(&run_7821(), &jobs, &templates(), &mut logs);
        assert_eq!(plan.reruns.len(), 16);
        assert_eq!(plan.not_rerun, 23);
        assert!(
            plan.everything.is_some(),
            "jobs this ci.yml does not have (C99, C102) cannot be gated: everything"
        );
        // Without the jobs that are not in this ci.yml.
        let ours: Vec<RunJob> = jobs
            .into_iter()
            .filter(|j| !j.name.starts_with('C') && !j.name.starts_with("endpoint"))
            .collect();
        let plan = SincePlan::new(&run_7821(), &ours, &templates(), &mut logs);
        assert_eq!(plan.everything, None);
        let narrowed: Vec<&str> = plan
            .reruns
            .iter()
            .filter(|r| matches!(r.scope, Scope::Tests { .. }))
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            narrowed,
            ["crate (macos-latest)", "app dyno (macos-latest)", "app pitwall (windows-latest)"]
        );
        let all_legs = vec![
            leg("dyno", "ubuntu-latest", ""),
            leg("dyno", "macos-latest", ""),
            leg("dyno", "windows-latest", ""),
            leg("dyno", "ubuntu-latest", "e2e-1"),
            leg("pitwall", "windows-latest", ""),
            leg("pitwall", "ubuntu-latest", ""),
            leg("pitwall", "ubuntu-latest", "e2e-1"),
            leg("pitwall", "ubuntu-latest", "e2e-2"),
            leg("pitcrew", "ubuntu-latest", ""),
            leg("spotter", "ubuntu-latest", ""),
            leg("spotter", "ubuntu-latest", "e2e-1"),
            leg("spotter", "macos-latest", ""),
        ];
        let mut warned = Vec::new();
        let out: BTreeMap<String, String> = plan.ci_outputs(&all_legs, &mut |w| warned.push(w)).into_iter().collect();
        assert!(warned.is_empty(), "{warned:?}");
        assert_eq!(out["crates"], "true");
        assert_eq!(out["ui"], "true");
        assert_eq!(out["conformance"], "true");
        assert_eq!(out["tools"], "false", "passed jobs are not rerun");
        assert_eq!(out["icons"], "false");
        assert_eq!(out["paddock_dev"], "false");
        assert_eq!(out["base"], "f1d7e0791f250c88dacd318f7caebf1dcf066cdf");
        assert_eq!(out["rerun_jobs"], r#"["app","conformance","crate","crates","e2e","ui"]"#);
        assert!(!out.contains_key("paddock_dev_legs"), "not rerun: the plan step writes every leg");
        let crate_legs: Value = serde_json::from_str(&out["crate_legs"]).unwrap();
        assert_eq!(crate_legs.as_array().unwrap().len(), 2);
        assert_eq!(crate_legs[0]["os"], "macos-latest");
        assert!(crate_legs[0]["filter"].as_str().unwrap().contains("every_adopting_app_is_in_sync"));
        assert_eq!(
            crate_legs[1],
            json!({"os": "windows-latest", "filter": ""}),
            "the Windows leg reruns whole"
        );
        let apps: Vec<Value> = serde_json::from_str(&out["apps"]).unwrap();
        let names: Vec<(String, String)> = apps
            .iter()
            .map(|l| (l["app"].as_str().unwrap().to_string(), l["os"].as_str().unwrap().to_string()))
            .collect();
        let pair = |a: &str, o: &str| (a.to_string(), o.to_string());
        assert_eq!(
            names,
            [
                pair("dyno", "macos-latest"),
                pair("dyno", "windows-latest"),
                pair("pitwall", "windows-latest"),
                pair("dyno", "ubuntu-latest"),
                pair("pitcrew", "ubuntu-latest"),
                pair("pitwall", "ubuntu-latest"),
                pair("pitwall", "ubuntu-latest"),
                pair("pitwall", "ubuntu-latest"),
                pair("spotter", "ubuntu-latest"),
            ],
            "the failed legs only: not dyno's passed end-to-end leg, not spotter's"
        );
        assert_eq!(apps[6]["e2e_leg"], "e2e-2");
        // The narrowed macOS leg runs only its tests; the Windows leg (its log is not in the fixture) runs whole.
        assert_eq!(apps[0]["tests_only"], "true");
        assert_eq!(apps[0]["checks"], "");
        assert_eq!(apps[0]["ui"], false);
        assert!(
            apps[0]["filter"]
                .as_str()
                .unwrap()
                .starts_with("(binary_id(=dyno-app) & test(=link::model::")
        );
        assert_eq!(apps[1]["checks"], "x");
        assert!(apps[1].get("tests_only").is_none());
        assert_eq!(out["any_app"], "true");
        let summary = plan.summary();
        assert!(summary.contains("| crate (macos-latest) | 1 failed test(s) only |"), "{summary}");
        assert!(summary.contains("| ui (tokens, contrast, components, entry points, publish dry run) | the whole job: "));
    }

    fn rerun(key: &str, leg: Leg) -> Rerun {
        Rerun {
            id: 1,
            name: "x".into(),
            key: Some(key.into()),
            leg,
            scope: Scope::Job("x".into()),
            vitest: vec![],
        }
    }

    fn plan_of(reruns: Vec<Rerun>) -> SincePlan {
        SincePlan {
            run: 1,
            workflow: "ci.yml".into(),
            head: "h".into(),
            reruns,
            not_rerun: 0,
            everything: None,
            full_ran: false,
        }
    }

    #[test]
    fn a_failed_webkit_job_or_webkit_leg_asks_for_the_webkit_legs() {
        let app = |e2e: Option<&str>| Leg {
            os: Some("ubuntu-latest".into()),
            app: Some("x".into()),
            e2e_leg: e2e.map(str::to_string),
            shard: None,
        };
        assert!(plan_of(vec![rerun("e2e-webkit", Leg::default())]).needs_webkit());
        assert!(plan_of(vec![rerun("app", app(Some("e2e-webkit")))]).needs_webkit());
        assert!(
            plan_of(vec![rerun("app", Leg::default())]).needs_webkit(),
            "an unknown leg may be the WebKit one"
        );
        assert!(!plan_of(vec![rerun("app", app(Some(""))), rerun("e2e", Leg::default())]).needs_webkit());
    }

    #[test]
    fn app_gates_is_narrowed_only_when_the_rust_step_and_the_composite_are_its_only_failures() {
        // A third failure (notices.sh or the cargo-about install, which print no ::error::) keeps the whole leg.
        let third = format!("{APP_DYNO}2026-11-09T07:46:00.0000000Z ##[error]Process completed with exit code 1.\n");
        assert!(
            matches!(classify(&job(33275), Ok(&third), "ci.yml", &templates()).scope, Scope::Job(ref w) if w.contains("3 failed steps"))
        );
        let one: String = APP_DYNO
            .lines()
            .filter(|l| !l.contains("07:45:57"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(matches!(classify(&job(33275), Ok(&one), "ci.yml", &templates()).scope, Scope::Job(ref w) if w.contains("1 failed steps")));
        assert!(matches!(
            classify(&job(33275), Ok(APP_DYNO), "ci.yml", &templates()).scope,
            Scope::Tests { .. }
        ));
    }

    #[test]
    fn the_project_outputs_are_every_project_of_the_job_table() {
        let out: Vec<String> = plan_of(vec![rerun("c99", Leg::default())])
            .ci_outputs(&[], &mut |_| {})
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        for (_, projects) in CI_JOBS {
            for p in projects.iter().filter(|p| !matches!(**p, "everything" | "apps")) {
                assert!(out.contains(&p.replace('-', "_")), "{p} in {out:?}");
            }
        }
        let c99: BTreeMap<String, String> = plan_of(vec![rerun("c99", Leg::default())])
            .ci_outputs(&[], &mut |_| {})
            .into_iter()
            .collect();
        assert_eq!(c99["c99"], "true");
        assert_eq!(c99["rerun_jobs"], r#"["c99"]"#);
        assert!(!c99.contains_key("crate_legs"), "not rerun: the plan step writes every leg");
    }

    #[test]
    fn a_failed_leg_missing_from_todays_matrix_reruns_every_leg_of_its_app_with_a_warning() {
        let plan = SincePlan {
            run: 1,
            workflow: "ci.yml".into(),
            head: "h".into(),
            reruns: vec![Rerun {
                id: 1,
                name: "app dyno (freebsd-latest)".into(),
                key: Some("app".into()),
                leg: Leg {
                    os: Some("freebsd-latest".into()),
                    app: Some("dyno".into()),
                    e2e_leg: Some(String::new()),
                    shard: None,
                },
                scope: Scope::Job("x".into()),
                vitest: vec![],
            }],
            not_rerun: 0,
            everything: None,
            full_ran: false,
        };
        let mut warned = Vec::new();
        let out: BTreeMap<String, String> = plan
            .ci_outputs(&[leg("dyno", "ubuntu-latest", ""), leg("spotter", "ubuntu-latest", "")], &mut |w| {
                warned.push(w)
            })
            .into_iter()
            .collect();
        assert_eq!(warned.len(), 1);
        let apps: Vec<Value> = serde_json::from_str(&out["apps"]).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0]["app"], "dyno");
    }

    #[test]
    fn a_failed_plan_job_or_a_job_of_another_workflow_plans_everything_and_the_json_names_each_decision() {
        let failed_plan = vec![RunJob {
            id: 9,
            name: "plan (affected projects)".into(),
            conclusion: "failure".into(),
            failed_steps: vec!["Affected projects".into()],
        }];
        let plan = SincePlan::new(&run_7821(), &failed_plan, &templates(), &mut |_| Err("gone".into()));
        assert!(plan.everything.unwrap().contains("the plan job failed"));
        // A run of another workflow: its jobs are that file's own, so ci.yml's job names are unknown there (everything), and
        // only the steps the tables list for that workflow take a filter.
        let other = json!({"id": 5, "status": "completed", "path": "debrief.yml@refs/heads/main", "head_sha": "h"});
        let plan = SincePlan::new(&other, &jobs(), &[], &mut logs);
        assert!(
            plan.everything
                .as_deref()
                .is_some_and(|e| e.contains("is not a job of this debrief.yml"))
        );
        assert!(
            plan.reruns.iter().all(|r| matches!(r.scope, Scope::Job(_))),
            "only the listed steps of a workflow take a filter"
        );
        let v = SincePlan::new(&run_7821(), &jobs(), &templates(), &mut logs).to_json();
        assert_eq!(v["run"], 7821);
        assert_eq!(v["workflow"], "ci.yml");
        let mac = v["jobs"].as_array().unwrap().iter().find(|j| j["id"] == 33271).unwrap();
        assert_eq!(mac["scope"], "tests");
        assert_eq!(mac["os"], "macos-latest");
        assert_eq!(
            mac["tests"],
            json!(["paddock-service per_machine::tests::every_adopting_app_is_in_sync"])
        );
        let ui = v["jobs"].as_array().unwrap().iter().find(|j| j["id"] == 33261).unwrap();
        assert_eq!(ui["scope"], "job");
    }

    const DEBRIEF_JOBS: &str = include_str!("../tests/since-run/debrief-run-jobs.json");
    const DEBRIEF_MACOS: &str = include_str!("../tests/since-run/debrief-rust-test-macos.log");

    /// Debrief's jobs as debrief.yml names them and what they need (tests/it/tree.rs checks the real file has these).
    const DEBRIEF_YML: &str = "name: debrief\non:\n  push:\njobs:\n  plan:\n    name: plan\n  rust-lint:\n    name: rust format + clippy\n    needs: [plan]\n  rust-test:\n    name: rust tests\n    needs: [plan]\n  rust-test-macos:\n    name: rust tests (macOS)\n    needs: [plan]\n  plugin-sdk:\n    name: plugin SDK templates\n    needs: [plan]\n  licences:\n    name: licences, notices + commits\n    needs: [plan]\n  frontend:\n    name: frontend build, lint + unit tests\n    needs: [plan]\n  e2e:\n    name: end-to-end (shard ${{ matrix.shard }}/4)\n    needs: [plan]\n  tauri-smoke:\n    name: real-app smoke (${{ matrix.os }})\n    needs: [plan, rust-lint]\n  bundle:\n    name: bundle (${{ matrix.os }})\n    needs: [plan, rust-lint]\n  keychain:\n    name: C99 (no Keychain in a macOS build)\n    needs: [plan]\n  release:\n    name: release\n    needs: [plan, bundle, tauri-smoke, rust-lint, rust-test, rust-test-macos, plugin-sdk, licences, frontend, e2e, keychain]\n  nightly-mark:\n    name: nightly-mark\n    needs: [rust-lint, rust-test, rust-test-macos, plugin-sdk, licences, frontend, e2e, tauri-smoke, bundle]\n";

    fn debrief_jobs() -> Vec<RunJob> {
        RunJob::list(&serde_json::from_str(DEBRIEF_JOBS).unwrap())
    }

    fn debrief_run(event: &str) -> Value {
        json!({"id": 7961, "status": "completed", "event": event, "path": "debrief.yml@refs/heads/train/2026-11-11-5", "head_sha": "d5"})
    }

    /// The fixture with some jobs changed: `(id, conclusion, failed steps)`.
    fn debrief_with(changes: &[(u64, &str, &[&str])]) -> Vec<RunJob> {
        let mut jobs = debrief_jobs();
        for (id, conclusion, steps) in changes {
            let j = jobs.iter_mut().find(|j| j.id == *id).unwrap();
            j.conclusion = conclusion.to_string();
            j.failed_steps = steps.iter().map(|s| s.to_string()).collect();
        }
        jobs
    }

    /// The plan job's log: the runner's header with the `bundles` input, and the mode line when given.
    fn plan_log(bundles: &str, mode: Option<&str>) -> String {
        let stamp = "2026-11-11T14:01:05.0000000Z ";
        let mut l =
            format!("{stamp}Job: plan\n{stamp}::group::Inputs\n{stamp}bundles: {bundles}\n{stamp}since_run: \n{stamp}::endgroup::\n");
        if let Some(m) = mode {
            l.push_str(&format!("{stamp}{MODE_LINE}{m}\n"));
        }
        l
    }

    fn plan_debrief_with(event: &str, jobs: &[RunJob], plan: Result<String, String>) -> SincePlan {
        let templates = name_templates(DEBRIEF_YML);
        let mut logs = |id: u64| -> Result<String, String> {
            match id {
                41001 => plan.clone(),
                41004 => Ok(DEBRIEF_MACOS.into()),
                41007 => Ok(VITEST.into()),
                _ => Err("not in the fixture".into()),
            }
        };
        let mut p = SincePlan::new(&debrief_run(event), jobs, &templates, &mut logs);
        if p.everything.is_none() {
            p.add_skipped_dependents(jobs, &templates, &job_needs(DEBRIEF_YML));
        }
        p
    }

    /// A run whose plan job logged its mode: full for a schedule, not full for a manual run without bundles.
    fn plan_debrief(event: &str, jobs: &[RunJob]) -> SincePlan {
        let full = if event == "schedule" { "true" } else { "false" };
        plan_debrief_with(event, jobs, Ok(plan_log("false", Some(full))))
    }

    #[test]
    fn a_debrief_run_that_failed_only_in_rust_tests_on_macos_reruns_that_job_and_its_failed_tests_only() {
        // Run 7961's shape: 20 jobs, only `rust tests (macOS)` failed. The fixture also failed the leak check after the tests,
        // as run 7961 did: the leaked folder may be any test's, so the whole job reruns.
        let whole = plan_debrief("workflow_dispatch", &debrief_jobs());
        assert_eq!(whole.reruns.len(), 1);
        assert_eq!(
            whole.reruns[0].scope,
            Scope::Job("step \"No temp folders left behind\" failed and takes no test filter".into())
        );
        assert_eq!(
            whole.app_outputs(),
            vec![("rerun_jobs".to_string(), r#"["rust-test-macos"]"#.to_string())]
        );
        // Only the nextest step failed: only its failed tests.
        let jobs = debrief_with(&[(41004, "failure", &["Tests (workspace)"])]);
        assert_eq!(jobs.len(), 20);
        let plan = plan_debrief("workflow_dispatch", &jobs);
        assert_eq!(plan.everything, None);
        assert_eq!(plan.reruns.len(), 1);
        assert_eq!(plan.not_rerun, 19);
        assert!(!plan.full_ran && !plan.needs_full());
        let out: BTreeMap<String, String> = plan.app_outputs().into_iter().collect();
        assert_eq!(out["rerun_jobs"], r#"["rust-test-macos"]"#);
        assert_eq!(
            out["rust_test_macos_filter"],
            "(binary_id(=debrief-app) & test(=browser::launch::tests::the_first_start_is_paid_before_the_guard)) | (binary_id(=debrief-store-duck::it) & test(=worker::imports_and_queries_within_the_budgets))"
        );
        assert_eq!(out.len(), 2, "no other job, shard or file list: {out:?}");
        assert!(plan.summary().contains("| rust tests (macOS) | 2 failed test(s) only |"));
    }

    #[test]
    fn a_failed_shard_reruns_alone_and_failed_unit_tests_rerun_only_their_spec_files() {
        let jobs = debrief_with(&[
            (41004, "success", &[]),
            (41010, "failure", &["Frontend end-to-end tests"]),
            (41007, "failure", &["Frontend unit tests"]),
        ]);
        let plan = plan_debrief("workflow_dispatch", &jobs);
        let out: BTreeMap<String, String> = plan.app_outputs().into_iter().collect();
        assert_eq!(out["rerun_jobs"], r#"["e2e","frontend"]"#);
        assert_eq!(out["e2e_shards"], r#"[{"shard":3}]"#, "only shard 3 of 4");
        assert!(!out.contains_key("e2e_filter"), "Playwright reruns the whole shard");
        assert_eq!(
            out["frontend_include"],
            "src/app/analysis/outliers-view.spec.ts src/app/watched/watched-folders.spec.ts"
        );
        // Another step of the frontend job failed too: the whole job.
        let jobs = debrief_with(&[(41004, "success", &[]), (41007, "failure", &["Lint", "Frontend unit tests"])]);
        let out: BTreeMap<String, String> = plan_debrief("workflow_dispatch", &jobs).app_outputs().into_iter().collect();
        assert_eq!(out["rerun_jobs"], r#"["frontend"]"#);
        assert!(!out.contains_key("frontend_include"));
    }

    #[test]
    fn a_full_runs_failed_lint_reruns_the_jobs_it_skipped_and_asks_for_a_full_rerun() {
        let jobs = debrief_with(&[(41004, "success", &[]), (41002, "failure", &["Clippy (workspace)"])]);
        let plan = plan_debrief("schedule", &jobs);
        assert!(plan.full_ran);
        let out: BTreeMap<String, String> = plan.app_outputs().into_iter().collect();
        assert_eq!(out["rerun_jobs"], r#"["bundle","rust-lint","tauri-smoke"]"#);
        assert!(!out.contains_key("tauri_smoke_shards"));
        assert!(plan.needs_full(), "the installers and the real-app smoke run only in a full run");
        let smoke = plan.reruns.iter().find(|r| r.key.as_deref() == Some("tauri-smoke")).unwrap();
        assert_eq!(smoke.scope, Scope::Job("it did not run: rust-lint, which it needs, failed".into()));
        assert_eq!(smoke.leg.os.as_deref(), Some("ubuntu-latest"));
        assert_eq!(plan.not_rerun, 13);
        // The release (a tag's) and the nightly mark (a schedule's) need the failed job too, but a manual run never has them.
        assert!(
            !plan
                .reruns
                .iter()
                .any(|r| matches!(r.key.as_deref(), Some("plan" | "release" | "nightly-mark")))
        );
    }

    #[test]
    fn a_manual_full_run_whose_lint_failed_is_known_full_from_its_plan_log_and_an_unknown_mode_plans_everything() {
        // bundles: true, rust-lint failed, so the installers and the real-app smoke never ran (skipped): nothing in the job list
        // says the run was full; the plan job's mode line does.
        let jobs = debrief_with(&[(41004, "success", &[]), (41002, "failure", &["Clippy (workspace)"])]);
        assert!(
            jobs.iter()
                .filter(|j| j.name.starts_with("bundle") || j.name.starts_with("real-app"))
                .all(|j| j.conclusion == "skipped")
        );
        let plan = plan_debrief_with("workflow_dispatch", &jobs, Ok(plan_log("true", Some("true"))));
        assert!(plan.full_ran && plan.needs_full());
        assert_eq!(plan.app_outputs()[0].1, r#"["bundle","rust-lint","tauri-smoke"]"#);
        // A run from before the mode line: the runner's Inputs group tells it.
        assert!(plan_debrief_with("workflow_dispatch", &jobs, Ok(plan_log("true", None))).full_ran);
        assert!(!plan_debrief_with("workflow_dispatch", &jobs, Ok(plan_log("false", None))).full_ran);
        // A failed WebKit-changing job (e2e) of a full manual run asks for a full rerun too.
        let e2e = debrief_with(&[(41004, "success", &[]), (41009, "failure", &["Frontend end-to-end tests"])]);
        assert!(plan_debrief_with("workflow_dispatch", &e2e, Ok(plan_log("true", Some("true")))).needs_full());
        // Nothing tells the mode (an unreadable plan log, or no line and no input): everything reruns.
        for log in [
            Err("HTTP 404".to_string()),
            Ok("2026-11-11T14:01:05.0000000Z Job: plan\n".to_string()),
        ] {
            let p = plan_debrief_with("workflow_dispatch", &jobs, log);
            assert!(
                p.everything.as_deref().is_some_and(|e| e.contains("cannot be told")),
                "{:?}",
                p.everything
            );
        }
    }

    #[test]
    fn a_failed_plan_job_or_an_unknown_job_of_an_app_workflow_plans_everything() {
        let jobs = debrief_with(&[(41001, "failure", &["Decide"])]);
        assert!(
            plan_debrief("workflow_dispatch", &jobs)
                .everything
                .unwrap()
                .contains("the plan job failed")
        );
        let mut jobs = debrief_jobs();
        jobs.push(RunJob {
            id: 9,
            name: "a job added later".into(),
            conclusion: "failure".into(),
            failed_steps: vec!["x".into()],
        });
        assert!(
            plan_debrief("workflow_dispatch", &jobs)
                .everything
                .unwrap()
                .contains("is not a job of this debrief.yml")
        );
    }

    #[test]
    fn vitest_failures_are_counted_against_the_summary_and_the_matrix_is_read_from_the_name() {
        assert_eq!(vitest_failures(VITEST).unwrap().len(), 2);
        let short = VITEST.replace("src/app/watched/watched-folders.spec.ts\u{1b}[2m >", "x");
        assert!(
            vitest_failures(&short)
                .unwrap_err()
                .contains("counts 2 failed files but the log names 1")
        );
        assert!(vitest_failures("nothing").unwrap_err().contains("0 vitest summaries"));
        let shard = matrix_of("end-to-end (shard 2/4)", "end-to-end (shard ${{ matrix.shard }}/4)");
        assert_eq!(shard.get("shard").map(String::as_str), Some("2"));
        let both = matrix_of(
            "app dyno (ubuntu-latest, e2e-1)",
            "app ${{ matrix.app }} (${{ matrix.os }}${{ matrix.e2e_leg != '' && format(', {0}', matrix.e2e_leg) || '' }})",
        );
        assert_eq!(both.get("app").map(String::as_str), Some("dyno"));
        assert!(matrix_of("other", "end-to-end (shard ${{ matrix.shard }}/4)").is_empty());
        assert_eq!(
            job_needs(DEBRIEF_YML).into_iter().find(|(k, _)| k == "bundle").unwrap().1,
            ["plan", "rust-lint"]
        );
    }

    struct Fake {
        answers: Vec<(String, Result<Value, String>)>,
        texts: Vec<(String, Result<String, String>)>,
        urls: Vec<String>,
    }

    impl Http for Fake {
        fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
            self.urls.push(url.into());
            self.answers
                .iter()
                .find(|(u, _)| url.ends_with(u.as_str()))
                .map_or(Err(format!("no answer for {url}")), |(_, a)| a.clone())
        }
        fn get_text(&mut self, url: &str, _token: &str) -> Result<String, String> {
            self.urls.push(url.into());
            self.texts
                .iter()
                .find(|(u, _)| url.ends_with(u.as_str()))
                .map_or(Err(format!("no answer for {url}")), |(_, a)| a.clone())
        }
    }

    #[test]
    fn the_run_is_read_page_by_page_and_an_unfinished_run_is_refused() {
        let all = jobs();
        assert_eq!(all.len(), 39);
        let page = |from: usize, to: usize| {
            let list: Vec<Value> = all[from..to]
                .iter()
                .map(|j| json!({"id": j.id, "name": j.name, "conclusion": j.conclusion, "steps": []}))
                .collect();
            Ok(json!({"total_count": all.len(), "jobs": list}))
        };
        let mut http = Fake {
            answers: vec![
                ("/runs/7821".into(), Ok(run_7821())),
                ("/runs/7821/jobs?limit=50&page=1".into(), page(0, 30)),
                ("/runs/7821/jobs?limit=50&page=2".into(), page(30, all.len())),
            ],
            texts: vec![("/jobs/33271/logs".into(), Ok(CRATE_MACOS.into()))],
            urls: vec![],
        };
        let mut runs = GiteaRuns::new(&mut http, "https://example.invalid/", "o/r", "secret".into());
        let mut lines = Vec::new();
        let plan = since_run(&mut runs, 7821, &|_| None, &mut |l| lines.push(l.to_string())).unwrap();
        assert_eq!(plan.reruns.len() + plan.not_rerun, all.len());
        assert!(
            lines[0].starts_with("run 7821 (ci.yml@refs/heads/train/x): 39 jobs, 16 failed"),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l.contains("job 33261: its log could not be read")));
        assert!(
            http.urls
                .contains(&"https://example.invalid/api/v1/repos/o/r/actions/runs/7821/jobs?limit=50&page=2".to_string())
        );
        let mut http = Fake {
            answers: vec![("/runs/7".into(), Ok(json!({"id": 7, "status": "running"})))],
            texts: vec![],
            urls: vec![],
        };
        let mut runs = GiteaRuns::new(&mut http, "https://example.invalid", "o/r", String::new());
        let e = since_run(&mut runs, 7, &|_| None, &mut |_| {}).unwrap_err();
        assert!(e.contains("run 7 is running: wait until it finishes"), "{e}");
    }
}
