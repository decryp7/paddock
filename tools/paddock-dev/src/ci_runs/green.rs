// ci-runs-reader: the one definition of green (ADR 0117 WP-4)
//! The one definition of "green" (ER-15, ADR 0117 section 6, WP-4), read by `ci verdict`, `ci reuse`, the landing queue and the
//! planner's base for a push to main.
//!
//! An outcome is a scope times a result:
//! - [`Scope`]: `Complete` when the run's declared scope covers a push to main's (ADR 0113, `Declared::complete`, read by
//!   `verdict::read_mode`) and every job it declared ran, else `Partial`. A run's base is complete by construction: the base of a
//!   push is picked by [`find_base`], which takes only complete runs, and a run with an explicit base declares less than it needs.
//! - [`GradedResult`]: `Green`; `GreenWithFixItems` when the only failures are FLAKY tests (failed, then passed on the one retry) in
//!   crates and apps the change did not reach (each a fix item; interim strict reading of ER-12, see [`Touched`]); `Red` when a test failed both tries, a FLAKY test is in a touched
//!   target, or a step that is not a test failed; `NotMeasured` when a test report cannot be read (ER-33).
//!
//! A skipped or cancelled job is never a pass and never a failure: a declared job that did not run makes the scope `Partial`
//! (`run_scope::check_ran`), and a cancelled run is `Partial` times `NotMeasured` (ER-33); an unfinished run is no evidence.
//!
//! A whole-tree run (a nightly, a tag, a plan of `all`) declares a scope that covers what a push needs without any base, so it is
//! `Complete` whatever its base was: the "complete base" chain of a push ends there. Runs with a base declare less and are `Partial`.
//!
//! The test report is the job log: CI uploads no JUnit or Playwright JSON today, and the nextest and Playwright summaries of the
//! log are read through the one reader, [`super::logfind`] (ER-42).
//!
//! [`find_base`] reads the run list once, in pages, and matches runs to commits in memory: no call per commit (ER-20); a run's jobs
//! and logs are read only for a run that is a candidate.

use super::client::{Client, RunInfo, TimedJob};
use super::logfind;
use super::verdict::{self, Earlier, Mode};
use crate::plan_since::workflow_file;
use crate::run_scope::PROJECT_JOBS;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Pages of 50 runs the base search reads at most (all branches and events: a train's dispatch run names the commit that lands).
pub const MAX_PAGES: u32 = 6;

/// First-parent commits of main the base search looks at, newest first.
pub const MAX_COMMITS: usize = 200;

/// How much of what a push to main needs the run covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The declared scope covers a push to main's and every declared job ran.
    Complete,
    /// Less than that.
    Partial,
}

/// What the run's jobs say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GradedResult {
    /// Nothing failed.
    Green,
    /// Only FLAKY tests in untouched targets failed; each is a fix item.
    GreenWithFixItems(Vec<String>),
    /// What to fix.
    Red(String),
    /// What could not be read.
    NotMeasured(String),
}

/// The scope and the result of one run, or of the commit it decides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// How much it covered.
    pub scope: Scope,
    /// What it found.
    pub result: GradedResult,
}

impl Outcome {
    /// Whether the commit may be a base for planning and may land: complete, and green with or without fix items.
    pub fn is_base(&self) -> bool {
        self.scope == Scope::Complete && matches!(self.result, GradedResult::Green | GradedResult::GreenWithFixItems(_))
    }

    /// Whether a tag may be cut from it: complete and plainly green (a tag on fix items takes the one allowed retry, ER-49).
    pub fn is_tag_green(&self) -> bool {
        self.scope == Scope::Complete && self.result == GradedResult::Green
    }
}

/// The targets a change touched, read from the plan of the change (its marked projects, until the graph lands).
///
/// INTERIM, strict (user decision 2026-11-23): a FLAKY test in ANY crate or app the plan reached is red, because the plan cannot yet say
/// which feature a test drives; a FLAKY test in a crate or app the plan did not reach is fix items. It becomes feature-level, as ER-12
/// words it, when ADR 0117's module edges land.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Touched {
    /// The project flags that were true (`crates`, `ui`, `paddockDev`...).
    pub flags: BTreeSet<String>,
    /// The apps the plan reached.
    pub apps: BTreeSet<String>,
}

impl Touched {
    /// From a plan (`./dev plan` JSON).
    pub fn from_plan(plan: &Value) -> Touched {
        let flags = plan
            .as_object()
            .map(|o| {
                o.iter()
                    .filter(|(_, v)| v.as_bool() == Some(true))
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default();
        let apps = plan["apps"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        Touched { flags, apps }
    }

    /// Whether the change reached what a CI job tests. A job this cannot place counts as touched: the safe side is red.
    pub fn reaches(&self, job: &str) -> bool {
        if let Some(rest) = job.strip_prefix("app ") {
            let app = rest.split(" (").next().unwrap_or(rest);
            return self.apps.contains(app);
        }
        if job.starts_with("crate (") {
            return self.flags.contains("crates");
        }
        if job.starts_with("paddock-dev (") {
            return self.flags.contains("paddockDev");
        }
        match PROJECT_JOBS
            .iter()
            .find(|(_, prefixes)| prefixes.iter().any(|p| job.starts_with(p)))
        {
            Some((flag, _)) => self.flags.contains(*flag) || (*flag == "e2e" || *flag == "e2e-webkit") && self.flags.contains("ui"),
            None => true,
        }
    }
}

/// A failed job of a finished complete run, as the grading reads it.
pub struct FailedJob {
    /// Its display name.
    pub name: String,
    /// The names of its failed steps (the jobs API's step conclusions).
    pub failed_steps: Vec<String>,
    /// Its log, `None` when it could not be read.
    pub log: Option<String>,
}

/// Grades the failed jobs of a finished complete run of `workflow`. A job is test-only when every failed step is a test step
/// ([`crate::plan_since::failed_steps_are_tests`]) and its log shows only FLAKY tests; any other failed step is red. `touched` is
/// asked at most once, and only when a FLAKY test needs it.
pub fn grade_failures(workflow: &str, jobs: &[FailedJob], touched: &mut dyn FnMut() -> Option<Touched>) -> GradedResult {
    use crate::plan_since::{StepProblem, failure_is_tests, job_key};
    let mut red = Vec::new();
    let mut unread = Vec::new();
    let mut items = Vec::new();
    let mut touch: Option<Option<Touched>> = None;
    for FailedJob { name, failed_steps, log } in jobs {
        let Some(log) = log else {
            unread.push(format!("job {name:?} has no log"));
            continue;
        };
        let f = logfind::read(log);
        if let Some(why) = f.broken {
            unread.push(format!("the test report of job {name:?} cannot be read: {why}"));
            continue;
        }
        if !f.failed.is_empty() || !f.vitest_failed.is_empty() {
            let t = f.failed.iter().chain(f.vitest_failed.iter()).next().cloned().unwrap_or_default();
            red.push(format!("job {name:?}: a test failed both tries ({t})"));
            continue;
        }
        if f.flaky.is_empty() {
            red.push(format!("job {name:?} failed in a step that is not a test"));
            continue;
        }
        // The log shows only FLAKY tests: the job is test-only only when no other step failed (ER-33).
        match job_key(log)
            .ok_or_else(|| StepProblem::Unknown(format!("the log of job {name:?} names no workflow job")))
            .and_then(|key| failure_is_tests(workflow, &key, failed_steps, log))
        {
            Ok(()) => {}
            Err(StepProblem::Other(step)) => {
                red.push(format!("job {name:?}: {step:?} failed and is not a test step or test command"));
                continue;
            }
            Err(StepProblem::Unknown(why)) => {
                unread.push(format!("job {name:?}: {why}"));
                continue;
            }
        }
        let t = touch.get_or_insert_with(&mut *touched);
        match t {
            None => unread.push(format!(
                "the targets the change touched cannot be read (job {name:?} has a FLAKY test)"
            )),
            Some(t) if t.reaches(name) => red.push(format!(
                "job {name:?}: a FLAKY test in a target the change touched ({})",
                f.flaky.iter().next().cloned().unwrap_or_default()
            )),
            Some(_) => items.extend(f.flaky.iter().map(|t| format!("{name}: {t}"))),
        }
    }
    if let Some(r) = red.into_iter().next() {
        GradedResult::Red(r)
    } else if let Some(u) = unread.into_iter().next() {
        GradedResult::NotMeasured(u)
    } else {
        GradedResult::GreenWithFixItems(items)
    }
}

/// Reads the logs of the failed jobs of a run of `workflow` and grades them.
pub fn read_failures(client: &mut Client, workflow: &str, jobs: &[TimedJob], touched: &mut dyn FnMut() -> Option<Touched>) -> GradedResult {
    let failed: Vec<FailedJob> = jobs
        .iter()
        .filter(|j| j.conclusion == "failure" && !j.is_caller())
        .map(|j| FailedJob {
            name: j.name.clone(),
            failed_steps: j.failed_steps.clone(),
            log: client.log(j.id).ok().flatten(),
        })
        .collect();
    if failed.is_empty() {
        return GradedResult::Red("the run failed and no job names a failure".into());
    }
    grade_failures(workflow, &failed, touched)
}

/// The outcome of one run, or `None` for a run that is no evidence (unfinished). A cancelled or skipped run is `Partial` times
/// `NotMeasured`: never a pass and never a failure (ER-33).
pub fn grade_run(
    client: &mut Client,
    run: &RunInfo,
    scope_decisive: bool,
    touched: &mut dyn FnMut() -> Option<Touched>,
) -> Option<Outcome> {
    if !run.finished() {
        return None;
    }
    if matches!(run.conclusion.as_str(), "cancelled" | "skipped") {
        return Some(Outcome {
            scope: Scope::Partial,
            result: GradedResult::NotMeasured(format!("run {} was {}", run.id, run.conclusion)),
        });
    }
    let (mode, jobs) = verdict::read_mode_jobs(client, run, scope_decisive);
    let state = if run.conclusion.is_empty() {
        run.status.as_str()
    } else {
        run.conclusion.as_str()
    };
    let (scope, why) = match mode {
        Mode::Full => (Scope::Complete, String::new()),
        Mode::Partial(why) => (Scope::Partial, why),
        Mode::Unknown(why) => {
            return Some(Outcome {
                scope: Scope::Partial,
                result: GradedResult::NotMeasured(why),
            });
        }
    };
    let result = if state == "success" {
        match earlier_failure(client, run) {
            Earlier::Clean => GradedResult::Green,
            Earlier::Failed(w) => GradedResult::Red(format!("green only after a rerun of failed jobs ({w})")),
            Earlier::Unknown(w) => GradedResult::NotMeasured(w),
        }
    } else if scope == Scope::Complete {
        read_failures(client, run.workflow(), &jobs, touched)
    } else {
        GradedResult::Red(format!("{state}; {why}"))
    };
    Some(Outcome { scope, result })
}

fn earlier_failure(client: &mut Client, run: &RunInfo) -> Earlier {
    if run.run_attempt > 1 {
        verdict::read_earlier(client, run)
    } else {
        Earlier::Clean
    }
}

/// The outcome of a commit from its runs: the newest run of the workflow that is evidence and complete decides (a newer red one
/// beats an older green one; a newer cancelled run, or a push that only reused another run and so declared less, does not).
/// Without a complete run, the newest run that is evidence.
pub fn commit_outcome(
    client: &mut Client,
    runs: &[&RunInfo],
    scope_decisive: bool,
    touched: &mut dyn FnMut() -> Option<Touched>,
) -> Option<(u64, Outcome)> {
    let mut newest = runs.to_vec();
    newest.sort_by_key(|r| std::cmp::Reverse(r.id));
    let mut fallback = None;
    for r in newest {
        let Some(o) = grade_run(client, r, scope_decisive, touched) else {
            continue;
        };
        if o.scope == Scope::Complete {
            return Some((r.id, o));
        }
        fallback.get_or_insert((r.id, o));
    }
    fallback
}

/// The commit a plan or a landing is based on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// The commit.
    pub sha: String,
    /// The run that decided it.
    pub run: u64,
    /// Its outcome (always a base outcome).
    pub outcome: Outcome,
}

/// The last first-parent commit (`first_parent`: newest first, the asking commit itself left out) whose outcome is complete and green
/// with or without fix items. The runs are read once, `MAX_PAGES` pages at most, and matched to commits in memory; `touched(sha)` is
/// asked only for a commit with a FLAKY-only failure. `Ok(None)`: no such commit among them.
pub fn find_base(
    client: &mut Client,
    workflow: &str,
    first_parent: &[String],
    scope_decisive: bool,
    touched: &mut dyn FnMut(&str) -> Option<Touched>,
) -> Result<Option<Base>, String> {
    let mut by_sha: BTreeMap<String, Vec<RunInfo>> = BTreeMap::new();
    for page in 1..=MAX_PAGES {
        let p = client
            .runs_page(page)
            .map_err(|e| format!("the runs of {workflow} could not be listed: {e}"))?;
        let n = p.runs.len();
        for r in p.runs.into_iter().filter(|r| workflow_file(&r.path) == workflow) {
            by_sha.entry(r.head_sha.clone()).or_default().push(r);
        }
        if n < 50 {
            break;
        }
    }
    for sha in first_parent.iter().take(MAX_COMMITS) {
        let Some(runs) = by_sha.get(sha) else { continue };
        let refs: Vec<&RunInfo> = runs.iter().collect();
        let mut t = || touched(sha);
        if let Some((run, outcome)) = commit_outcome(client, &refs, scope_decisive, &mut t)
            && outcome.is_base()
        {
            return Ok(Some(Base {
                sha: sha.clone(),
                run,
                outcome,
            }));
        }
    }
    Ok(None)
}

/// The first-parent commits before `sha`, newest first, at most [`MAX_COMMITS`] (`git rev-list --first-parent`); empty when git cannot
/// say (then no base is found: everything runs, the safe side).
pub fn first_parent_before(root: &std::path::Path, sha: &str) -> Vec<String> {
    let range = format!("{sha}^");
    let n = MAX_COMMITS.to_string();
    crate::plan_repo::git_text(root, &["rev-list", "--first-parent", "-n", &n, &range])
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

/// What the change of `sha` (its first parent to it) touched: the plan of that change. `None` when it cannot be planned (a commit
/// that is not in this clone), which makes a FLAKY test not measured, never untouched.
pub fn plan_touched(root: &std::path::Path, sha: &str) -> Option<Touched> {
    use crate::plan::{Request, plan_with_graph};
    let history = paddock_repo_graph::GitHistory::new(root, std::time::Duration::from_secs(120));
    let req = Request {
        files: Vec::new(),
        all: false,
        explain: false,
        root: None,
        base: Some(format!("{sha}^1")),
        head: sha.to_string(),
    };
    plan_with_graph(root, &history, &req)
        .ok()
        .map(|(_, plan)| Touched::from_plan(&plan))
}

#[cfg(test)]
mod tests;
