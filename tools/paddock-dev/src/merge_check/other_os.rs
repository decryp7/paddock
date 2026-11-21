// ci-runs-reader: the member gate (other-os) reads the jobs and the triage rows of ONE partial member run (a base dispatch, which ci verdict refuses by design) for a report-only row per OS; it never says a commit may land or be tagged: that verdict stays ./dev ci verdict
//! The `other-os` member gate of `./dev merge check --gates` (train 14, early-catch item 1; ADR 0087 amendment of 2026-11-16): a
//! member's affected tests run on Linux and Windows BEFORE the member joins a train, **on the real CI runners**, as merge queues and
//! merge trains do. The user decided this on 2026-11-16 after the first version (a container on the Arm Mac and the Windows test VM)
//! gave false findings: 22 `paddock-dev` tests failed on the VM on a base where the real runner is green.
//!
//! The step does no selection and no running of its own:
//! 1. **Selection is the plan's** (`./dev plan <train before> <merge>`, the planner CI uses; rule 55: one copy of "what does a
//!    change reach"). A member that reaches no project says `not run (<why>)`.
//! 2. The merge commit is pushed to a short-lived ref `trial/member-<sha>` (the `trial/*` namespace of ADR 0098 WP-4, which
//!    `./dev ci trial-prune` also cleans), and `ci.yml` is dispatched on it with `base` = the train before, so CI's own planner runs
//!    only the affected jobs. The dispatch waits for a busy single-machine runner (ER-24, D-1: one train's CI per runner class at a
//!    time), bounded; a runner that stays busy is `not measured`.
//! 3. One bounded wait on the run (`./dev wait --ci-run`), then the jobs and the triage are read: **only NEW failures fail a
//!    row** (`./dev ci triage` compares with the last main run and the flake history; a failure also on main is listed, not blamed).
//!
//! Every OS gets a row made with the shared [`measure`] and [`Outcome`] types (C124): `Clean` with evidence for this run, `Findings`
//! (the NEW failures), `NotMeasured` (the reason) or `NotApplicable` (no job of the plan ran on that OS). Never a silent skip
//! (ER-33). **Enforced** ([`ENFORCED`], user, 2026-11-21: "member checks on real runners, build it next"): a train merge whose
//! other-OS tests could not be measured (no token, no trial folder, a busy runner, a run that never ended) exits 6, and a NEW
//! failure on one OS exits 5. The step dispatches the trial itself whenever `GITEA_TOKEN` is set; the trial's scratch folder is
//! `<trial root>/<branch>-<sha>` (see [`crate::ci_runs::trial::trial_root`]) and is removed at the end (ER-14, ER-51).
//!
//! **Judging (ER-12):** a test that failed both tries blocks; a FLAKY test (failed, then passed on its one retry) blocks only when
//! the member's change touches its feature ([`feature_touched`]: a changed file named like one of the test's modules, or a test
//! whose module cannot be told); otherwise the row passes and the flake is listed as a record, a fix item for the next train.
//!
//! **Logging (ER-27):** the run id, the verdict per OS and the run's wall time split into queue wait, setup, compile and tests
//! (from the job step timings, [`Phases`]) are in the step's text and data.
//! The pass/fail reading of a PARTIAL run is this module's; `ci verdict` stays the one reader of FULL runs (it refuses a base
//! dispatch by design), so it is not asked here.

use super::generator_step::Step;
use super::{Merge, git::Git, short};
use crate::ci_runs::client::{Client, TimedJob};
use crate::ci_runs::{dispatch, reservation, time, triage, trial};
use crate::queue::ports::{Blame, Failure, Sent};
use crate::queue::real::{Child, GitTrain, failures_of};
use crate::wait::{CurlHttp, Http};
use paddock_cli::outcome::{CheckId, Finding, Outcome, Reason, RunId, Sha, Subject, ToolVersion, UnixSeconds, measure};
use paddock_cli::{Exit, Severity};
use paddock_proc::{Limits, command, run_bounded};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Whether a failing or unmeasured row changes the exit code. True since 2026-11-21 (user: member checks on real runners): the gate
/// never prints "not measured" and lets the merge go on (ER-9, ER-33). The ADR 0087 amendment of 2026-11-16 records the decision.
pub const ENFORCED: bool = true;

/// The workflow the member's run uses: the plan's own jobs.
pub const WORKFLOW: &str = "ci.yml";

/// How long the dispatch may wait for a busy single-machine runner (ER-24).
const MAX_DISPATCH_WAIT: &str = "45m";
/// How long the run may take once it is dispatched (a full affected plan on the real runners).
const MAX_RUN_WAIT: &str = "2h";

/// An operating system a row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Os {
    /// The Linux jobs of the run (`ubuntu-latest`, the CI image).
    Linux,
    /// The Windows jobs of the run (`windows-latest`, the real runner).
    Windows,
}

impl Os {
    /// The name in rows and in the JSON.
    pub fn name(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }

    /// The OS a job of ci.yml's matrix belongs to, from its name (`paddock-dev (windows-latest)`); `None` for a job that names none
    /// (the plan, the conformance job) or another OS (macOS is the integrator's own machine).
    pub fn of_job(name: &str) -> Option<Os> {
        let n = name.to_ascii_lowercase();
        if n.contains("windows") {
            Some(Os::Windows)
        } else if n.contains("ubuntu") || n.contains("linux") {
            Some(Os::Linux)
        } else {
            None
        }
    }
}

/// One job of the member's run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    /// The job's display name.
    pub name: String,
    /// `success`, `failure`, `cancelled`, `skipped` or empty while unfinished.
    pub conclusion: String,
    /// Where its time went, when the server gave step timings.
    pub phases: Option<Phases>,
}

/// Where a job's wall time went, in seconds (ER-27). The split is read from the step names, for the log only: no verdict depends on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Phases {
    /// From the run's creation to the job's start (waiting for a runner).
    pub queue: u64,
    /// Checkout, runner and toolchain setup, caches.
    pub setup: u64,
    /// Build, lint and format steps.
    pub compile: u64,
    /// Test steps.
    pub tests: u64,
}

impl Phases {
    /// Jobs of one OS run side by side: the queue wait is the longest, the other phases add up (job-seconds).
    pub fn combine(self, o: Phases) -> Phases {
        Phases {
            queue: self.queue.max(o.queue),
            setup: self.setup + o.setup,
            compile: self.compile + o.compile,
            tests: self.tests + o.tests,
        }
    }

    /// `queue 12s, setup 40s, compile 8m00s, tests 2m00s`.
    pub fn text(&self) -> String {
        let f = |s: u64| {
            if s >= 60 {
                format!("{}m{:02}s", s / 60, s % 60)
            } else {
                format!("{s}s")
            }
        };
        format!(
            "queue {}, setup {}, compile {}, tests {}",
            f(self.queue),
            f(self.setup),
            f(self.compile),
            f(self.tests)
        )
    }
}

/// Which phase a step belongs to, from its name (the shared actions show as `Run ./.gitea/actions/<name>`).
fn phase_of(step: &str) -> usize {
    let n = step.to_ascii_lowercase();
    if n.contains("test") {
        2
    } else if (n.contains("build") && !n.contains("build-jobs") && !n.contains("build-folder"))
        || ["clippy", "compile", "cargo check", "lint", "fmt"].iter().any(|k| n.contains(k))
    {
        1
    } else {
        0
    }
}

/// The phases of one job: the queue wait from the run's creation to the job's start, then the steps' seconds by phase.
pub fn phases_of(run_created_ms: Option<f64>, job_start_ms: Option<f64>, steps: &[(String, Option<f64>)]) -> Phases {
    let mut by = [0u64; 3];
    for (name, secs) in steps {
        by[phase_of(name)] += secs.unwrap_or(0.0).max(0.0).round() as u64;
    }
    let queue = run_created_ms
        .zip(job_start_ms)
        .map_or(0, |(c, s)| ((s - c) / 1000.0).max(0.0).round() as u64);
    Phases {
        queue,
        setup: by[0],
        compile: by[1],
        tests: by[2],
    }
}

/// How the wait on the run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    /// The run finished (any conclusion).
    Finished,
    /// The run was still going when the bound ended.
    StillRunning,
}

/// What the plan said about the member's own change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// At least one project is affected.
    Projects,
    /// Nothing but documentation (or nothing) is affected.
    Nothing,
}

/// The real CI as this step needs it. The real one is [`Gitea`]; tests give a table.
pub trait Runs {
    /// Whether the plan reaches any project for `base..head`; `Err` when it could not be asked.
    fn reaches(&mut self, base: &str, head: &str) -> Result<Reach, String>;
    /// The files the member changes (`base..head`), to tell whether a flaky test's feature is touched.
    fn changed_files(&mut self, base: &str, head: &str) -> Result<Vec<String>, String>;
    /// Pushes `commit` to `git_ref` (a short-lived scratch ref).
    fn publish(&mut self, commit: &str, git_ref: &str) -> Result<(), String>;
    /// Dispatches [`WORKFLOW`] on `git_ref` with `base`.
    fn dispatch(&mut self, git_ref: &str, base: &str) -> Result<Sent, String>;
    /// The id of the run on `commit`, once it exists.
    fn find_run(&mut self, commit: &str) -> Result<Option<u64>, String>;
    /// Waits for the run, at most [`MAX_RUN_WAIT`].
    fn wait(&mut self, run: u64) -> Result<RunEnd, String>;
    /// The jobs of the run.
    fn jobs(&mut self, run: u64) -> Result<Vec<JobRow>, String>;
    /// The failures of the finished run, each blamed by `ci triage`.
    fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String>;
    /// Deletes the scratch ref (best effort: `ci trial-prune` catches the rest).
    fn delete(&mut self, git_ref: &str);
}

/// The plan's answer as a [`Reach`]: any project flag true, an app named, or `all`.
pub fn reach_of(plan: &Value) -> Reach {
    let flag = |k: &str| plan.get(k).and_then(Value::as_bool).unwrap_or(false);
    let any = [
        "all",
        "crates",
        "ui",
        "eslint",
        "conformance",
        "c99",
        "tools",
        "icons",
        "ciImage",
        "paddockDev",
    ]
    .iter()
    .any(|k| flag(k))
        || plan.get("apps").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    if any { Reach::Projects } else { Reach::Nothing }
}

/// One OS's row: the shared outcome, the jobs it is made of and the failures that were already on main.
#[derive(Debug)]
pub struct Row {
    /// The OS.
    pub os: Os,
    /// The outcome (C124).
    pub outcome: Outcome,
    /// The jobs of the run on this OS.
    pub jobs: Vec<String>,
    /// Failures that triage found also on main: listed, not blamed.
    pub known: Vec<String>,
    /// FLAKY tests (failed, then passed on their one retry) whose feature the member does not touch: recorded as fix items, not
    /// blamed (ER-12).
    pub flaky: Vec<String>,
    /// Where the jobs' time went, summed over the OS's jobs; `None` when no job had step timings.
    pub phases: Option<Phases>,
}

fn tool() -> ToolVersion {
    ToolVersion::new(concat!("paddock-dev merge check other-os ", env!("CARGO_PKG_VERSION")))
}

/// The job a failure belongs to: its `what` is `<job>: <test>`.
fn job_of(f: &Failure) -> &str {
    f.what.split(": ").next().unwrap_or("")
}

/// Whether the member's change touches the feature of a failed test (ER-12: "the test itself or the source it exercises"). `what` is
/// `<job>: <test>`. The test's modules (`a::b::name` gives `a` and `b`, without `tests`, `test` and `it`) are looked for as the
/// name of a changed file or folder (`a.rs`, `a/mod.rs`, `tests/it/a.rs`, `a-b` is `a_b`). A test whose module cannot be told
/// (no `::`, a whole-job failure, an unreadable test) counts as touched: when unsure it blocks.
pub fn feature_touched(what: &str, changed: &[String]) -> bool {
    let test = what.split_once(": ").map_or(what, |(_, t)| t);
    let test = test.split_whitespace().last().unwrap_or("");
    let mut segs: Vec<&str> = test.split("::").collect();
    if segs.len() < 2 {
        return true;
    }
    segs.pop();
    let mods: Vec<String> = segs
        .into_iter()
        .map(|m| m.to_ascii_lowercase().replace('-', "_"))
        .filter(|m| !matches!(m.as_str(), "tests" | "test" | "it" | ""))
        .collect();
    if mods.is_empty() {
        return true;
    }
    changed.iter().any(|f| {
        f.split('/').any(|c| {
            let stem = c.rsplit_once('.').map_or(c, |(a, _)| a).to_ascii_lowercase().replace('-', "_");
            mods.contains(&stem)
        })
    })
}

/// The rows of a finished run: for each OS, `NotApplicable` when the plan ran no job there, `NotMeasured` when a job did not finish
/// or a failure cannot be blamed, `Findings` for NEW (and flaky: still red, ER-12) failures only, and `Clean` otherwise (failures
/// also on main are `known`).
pub fn rows(run: u64, sha: &str, jobs: &[JobRow], failures: &[Failure], changed: &[String]) -> Vec<Row> {
    let subject = || Subject::CiRun {
        workflow: WORKFLOW.into(),
        run: RunId(run),
        sha: Sha::new(sha),
    };
    [Os::Linux, Os::Windows]
        .into_iter()
        .map(|os| {
            let mine: Vec<&JobRow> = jobs.iter().filter(|j| Os::of_job(&j.name) == Some(os)).collect();
            let names: Vec<String> = mine.iter().map(|j| j.name.clone()).collect();
            let fails: Vec<&Failure> = failures.iter().filter(|f| Os::of_job(job_of(f)) == Some(os)).collect();
            let known: Vec<String> = fails
                .iter()
                .filter(|f| matches!(f.blame, Blame::OnMain))
                .map(|f| f.what.clone())
                .collect();
            // A FLAKY test whose feature the member does not touch is a record, not a finding (ER-12).
            let flaky: Vec<String> = fails
                .iter()
                .filter(|f| matches!(f.blame, Blame::Flaky) && !feature_touched(&f.what, changed))
                .map(|f| f.what.clone())
                .collect();
            let phases = mine.iter().filter_map(|j| j.phases).reduce(Phases::combine);
            let outcome = if mine.is_empty() {
                Outcome::NotApplicable(Reason::new(format!("the plan ran no job on {} for this member", os.name())))
            } else if let Some(j) = mine
                .iter()
                .find(|j| !matches!(j.conclusion.as_str(), "success" | "failure" | "skipped"))
            {
                Outcome::NotMeasured(Reason::new(format!(
                    "job `{}` ended `{}` (cancelled or unfinished), so nothing can be said about {}",
                    j.name,
                    if j.conclusion.is_empty() { "unfinished" } else { &j.conclusion },
                    os.name()
                )))
            } else if let Some(j) = mine
                .iter()
                .find(|j| j.conclusion == "failure" && !fails.iter().any(|f| job_of(f) == j.name))
            {
                Outcome::NotMeasured(Reason::new(format!(
                    "job `{}` failed but triage read no failing test of it (nothing proves it is the member's)",
                    j.name
                )))
            } else if let Some(f) = fails.iter().find(|f| matches!(f.blame, Blame::Unknown)) {
                Outcome::NotMeasured(Reason::new(format!(
                    "`{}` failed and nothing proves whether it also fails on main (triage: unknown)",
                    f.what
                )))
            } else {
                let new: Vec<Finding> = fails
                    .iter()
                    .filter(|f| matches!(f.blame, Blame::New | Blame::Flaky) && !flaky.contains(&f.what))
                    .map(|f| Finding::failing(f.what.clone()))
                    .collect();
                measure(
                    CheckId::new(format!("member-gate-other-os:{}", os.name())),
                    subject(),
                    tool(),
                    UnixSeconds::now(),
                    || Ok(new),
                )
            };
            Row {
                os,
                outcome,
                jobs: names,
                known,
                flaky,
                phases,
            }
        })
        .collect()
}

/// What a row costs: nothing while report-only; once enforced, failing findings are the findings exit and an unmeasured row the
/// not-measured exit.
fn cost(enforced: bool, outcome: &Outcome) -> Exit {
    match (enforced, outcome) {
        (false, _) => Exit::Ok,
        (true, Outcome::Findings(f)) if f.findings().as_slice().iter().any(|x| x.severity == Severity::Failing) => Exit::Findings,
        (true, Outcome::NotMeasured(_) | Outcome::CheckBroken(_)) => Exit::NotMeasured,
        (true, _) => Exit::Ok,
    }
}

fn worst(a: Exit, b: Exit) -> Exit {
    if a == Exit::Findings || b == Exit::Findings {
        Exit::Findings
    } else if a == Exit::NotMeasured || b == Exit::NotMeasured {
        Exit::NotMeasured
    } else {
        Exit::Ok
    }
}

fn wording(enforced: bool) -> (&'static str, &'static str) {
    if enforced {
        ("::error", "")
    } else {
        (
            "::warning",
            " REPORT-ONLY until its first green run on a real train is recorded in the ADR 0087 amendment of 2026-11-16 (ER-41): the exit code is unchanged.",
        )
    }
}

/// The step for the rows of a run.
pub fn step_of(rows: &[Row], run: u64, git_ref: &str, base: &str, enforced: bool) -> Step {
    let (level, report_only) = wording(enforced);
    let mut text = vec![format!(
        "member gate other-os (base {base}, run {run} on {git_ref}, real CI runners):"
    )];
    let mut exit = Exit::Ok;
    let mut data = Vec::new();
    for r in rows {
        let also = if r.known.is_empty() {
            String::new()
        } else {
            format!("; {} failure(s) also on main, not blamed", r.known.len())
        };
        let split = r.phases.map_or(String::new(), |p| format!(", {}", p.text()));
        text.push(format!(
            "  {}: {} ({} jobs{split}){also}",
            r.os.name(),
            r.outcome.name(),
            r.jobs.len()
        ));
        for t in &r.flaky {
            text.push(format!(
                "    FLAKY (recorded, not blocking: the member does not touch its feature, ER-12; fix item for the next train, with a failing reproduction first): {t}"
            ));
        }
        match &r.outcome {
            Outcome::Findings(f) => {
                for x in f.findings().as_slice() {
                    text.push(format!("    NEW: {}", x.message));
                }
                text.push(format!(
                    "{level}::member gate other-os FAILED on {}: the member's run has failures that are not on main. Next step: take the member out and send it back to its author with the failures above; do not fix it on the train.{report_only}",
                    r.os.name()
                ));
            }
            Outcome::NotMeasured(why) | Outcome::CheckBroken(why) => text.push(format!(
                "{level}::member gate other-os NOT MEASURED on {}: {}. Next step: read run {run} yourself (`./dev ci triage {run}`); a member whose other-OS tests did not give a verdict is not a member that passed.{report_only}",
                r.os.name(),
                why.as_str()
            )),
            _ => {}
        }
        exit = worst(exit, cost(enforced, &r.outcome));
        data.push(json!({
            "os": r.os.name(),
            "outcome": r.outcome.json(),
            "jobs": r.jobs,
            "alsoOnMain": r.known,
            "flakyRecorded": r.flaky,
            "phases": r.phases.map(|p| json!({"queueSecs": p.queue, "setupSecs": p.setup, "compileSecs": p.compile, "testSecs": p.tests})),
        }));
    }
    let word = if rows.iter().any(|r| matches!(r.outcome, Outcome::Findings(_))) {
        "failed"
    } else if rows
        .iter()
        .any(|r| matches!(r.outcome, Outcome::NotMeasured(_) | Outcome::CheckBroken(_)))
    {
        "not measured"
    } else if rows.iter().any(|r| matches!(r.outcome, Outcome::Clean(_))) {
        "passed"
    } else {
        "not run"
    };
    Step {
        text,
        data: json!({"outcome": word, "base": base, "run": run, "ref": git_ref, "enforced": enforced, "rows": data}),
        exit,
    }
}

/// Said out loud when the step was not asked for or cannot apply; not a failure.
pub fn not_run(why: &str) -> Step {
    Step {
        text: vec![format!("member gate other-os: not run ({why})")],
        data: json!({"outcome": "not run", "why": why}),
        exit: Exit::Ok,
    }
}

/// The step when nothing could be measured: report-only keeps the exit, enforced is the not-measured exit.
pub fn unmeasured(enforced: bool, why: &str) -> Step {
    let (level, report_only) = wording(enforced);
    Step {
        text: vec![format!(
            "{level}::member gate other-os NOT MEASURED: {why}. Next step: fix that cause and run `./dev merge check --gates <merge-commit>` again (it dispatches `ci.yml` on the member itself when GITEA_TOKEN is set), or run `./dev ci dispatch ci.yml --trial <branch> --max 45m` by hand and read the run; a member whose other-OS tests did not run is not a member that passed.{report_only}"
        )],
        data: json!({"outcome": "not measured", "why": why, "enforced": enforced}),
        exit: if enforced { Exit::NotMeasured } else { Exit::Ok },
    }
}

/// The scratch ref of a merge commit.
pub fn scratch_ref(commit: &str) -> String {
    format!("trial/member-{}", commit.get(..12).unwrap_or(commit))
}

/// Runs the gate for a merge: plan, push, dispatch, wait, read, rows. The scratch ref is deleted whatever happens.
pub fn run(merge: &Merge, runs: &mut dyn Runs, enforced: bool) -> Step {
    let (Some(base), Some(commit)) = (merge.parents.first(), merge.commit.as_deref()) else {
        return not_run("the merge has no parents or no commit");
    };
    match runs.reaches(base, commit) {
        Err(e) => return unmeasured(enforced, &format!("the plan could not be asked: {e}")),
        Ok(Reach::Nothing) => return not_run("the plan reaches no project for this member (documentation only)"),
        Ok(Reach::Projects) => {}
    }
    let git_ref = scratch_ref(commit);
    let step = measure_on_runners(runs, commit, base, &git_ref, enforced);
    runs.delete(&git_ref);
    step
}

fn measure_on_runners(runs: &mut dyn Runs, commit: &str, base: &str, git_ref: &str, enforced: bool) -> Step {
    let changed = match runs.changed_files(base, commit) {
        Ok(c) => c,
        Err(e) => return unmeasured(enforced, &format!("the member's changed files could not be listed: {e}")),
    };
    if let Err(e) = runs.publish(commit, git_ref) {
        return unmeasured(enforced, &format!("the merge could not be pushed to {git_ref}: {e}"));
    }
    match runs.dispatch(git_ref, base) {
        Err(e) => return unmeasured(enforced, &format!("ci.yml could not be dispatched on {git_ref}: {e}")),
        Ok(Sent::Busy(last)) => {
            return unmeasured(
                enforced,
                &format!("the runner stayed busy for {MAX_DISPATCH_WAIT} ({last}); one train's CI per runner class at a time (ER-24)"),
            );
        }
        Ok(Sent::Yes) => {}
    }
    let run = match runs.find_run(commit) {
        Ok(Some(id)) => id,
        Ok(None) => return unmeasured(enforced, &format!("no run of ci.yml appeared for {git_ref}")),
        Err(e) => return unmeasured(enforced, &format!("the run could not be found: {e}")),
    };
    eprintln!(
        "other-os: ci.yml run {run} on {git_ref} (base {}); waiting at most {MAX_RUN_WAIT}",
        short(base)
    );
    match runs.wait(run) {
        Err(e) => return unmeasured(enforced, &format!("run {run} could not be waited for: {e}")),
        Ok(RunEnd::StillRunning) => return unmeasured(enforced, &format!("run {run} was still going after {MAX_RUN_WAIT}")),
        Ok(RunEnd::Finished) => {}
    }
    let jobs = match runs.jobs(run) {
        Ok(j) => j,
        Err(e) => return unmeasured(enforced, &format!("the jobs of run {run} could not be read: {e}")),
    };
    let failures = match runs.failures(run) {
        Ok(f) => f,
        Err(e) => return unmeasured(enforced, &format!("the failures of run {run} could not be triaged: {e}")),
    };
    let rows = rows(run, commit, &jobs, &failures, &changed);
    for r in &rows {
        eprintln!(
            "other-os: run {run} {}: {}{}",
            r.os.name(),
            r.outcome.name(),
            r.phases.map_or(String::new(), |p| format!(" ({})", p.text()))
        );
    }
    step_of(&rows, run, git_ref, short(base), enforced)
}

/// The real CI: Gitea's API with `GITEA_TOKEN`, the tool's own git for the push, the tool's own planner and waiter.
pub struct Gitea {
    root: PathBuf,
    server: String,
    repo: String,
    token: String,
    /// The trial root and this merge's own folder below it (`<root>/<name>`): made here, removed at the end (ER-51).
    trial_root: PathBuf,
    scratch: PathBuf,
    http: CurlHttp,
}

/// The folder name of one merge's trial: its label (cleaned to letters, digits, `.`, `_`, `-`) and the commit's short id.
pub fn trial_folder_name(label: &str, commit: &str) -> String {
    let clean: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' })
        .collect();
    let clean = clean.trim_matches(['-', '.']);
    let clean = clean.get(..clean.len().min(60)).unwrap_or(clean);
    format!(
        "{}-{}",
        if clean.is_empty() { "member" } else { clean },
        commit.get(..12).unwrap_or(commit)
    )
}

impl Gitea {
    /// Reads the settings: `GITEA_TOKEN` (write access: it pushes the scratch ref) from the environment, and the trial root from
    /// [`trial::trial_root_real`] (`PADDOCK_TRIAL_DIR`, git config `paddock.trialRoot`, or `trials` beside the worktree root). It
    /// makes this merge's own folder below the root. `Err` says what is missing and the next step.
    pub fn open(root: &Path, label: &str, commit: &str) -> Result<Gitea, String> {
        Self::open_with(root, label, commit, std::env::var("GITEA_TOKEN").ok(), trial::trial_root_real(root))
    }

    /// [`Gitea::open`] with the token and the trial root given (tests).
    pub fn open_with(
        root: &Path,
        label: &str,
        commit: &str,
        token: Option<String>,
        trial_root: Result<PathBuf, String>,
    ) -> Result<Gitea, String> {
        let token = token
            .filter(|t| !t.is_empty())
            .ok_or("GITEA_TOKEN is not set (it needs write access: it pushes the trial ref and dispatches ci.yml); run `source ~/.zshenv` or export it")?;
        let trial_root = trial_root?;
        let scratch = trial_root.join(trial_folder_name(label, commit));
        std::fs::create_dir_all(&scratch).map_err(|e| {
            format!(
                "cannot create {}: {e}. Next step: check the trial folder is writable",
                scratch.display()
            )
        })?;
        Ok(Gitea {
            root: root.to_path_buf(),
            server: "https://dev.decryptology.net".into(),
            repo: "decryp7/paddock".into(),
            token,
            trial_root,
            scratch,
            http: CurlHttp,
        })
    }

    fn git(&self) -> GitTrain {
        GitTrain {
            root: self.root.clone(),
            trusted: None,
            url: format!("{}/{}.git", self.server, self.repo),
            token: Some(self.token.clone()),
            authed_fetch: false,
            allow_file: false,
            scratch: self.scratch.clone(),
            merge_check: crate::merge_check::command,
            planner: crate::queue::real::run_planner,
            timeout: crate::queue::real::GIT_TIMEOUT,
        }
    }

    fn me() -> Result<PathBuf, String> {
        crate::tool_cache::self_exe().map_err(|e| format!("this program cannot be found: {e}"))
    }
}

impl Drop for Gitea {
    /// Removes this merge's folder (never the root), through the one safe-remove rule, whatever way the step ended.
    fn drop(&mut self) {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        if let Err(e) = crate::safe_remove::remove_under(&self.trial_root, &self.scratch, home.as_deref()) {
            eprintln!(
                "::warning::other-os: {} could not be removed ({e}); delete it by hand",
                self.scratch.display()
            );
        }
    }
}

impl Runs for Gitea {
    fn changed_files(&mut self, base: &str, head: &str) -> Result<Vec<String>, String> {
        let (ok, out) = self.git().run_git(Child::Local, &["diff", "--name-only", base, head])?;
        if ok {
            Ok(out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        } else {
            Err(out.trim().to_string())
        }
    }

    fn reaches(&mut self, base: &str, head: &str) -> Result<Reach, String> {
        ask_plan(&self.root, base, head)
    }

    fn publish(&mut self, commit: &str, git_ref: &str) -> Result<(), String> {
        let g = self.git();
        let (ok, out) = g.run_git(
            Child::Authed,
            &["push", "--quiet", "--force", "origin", &format!("{commit}:refs/heads/{git_ref}")],
        )?;
        if ok { Ok(()) } else { Err(out.trim().to_string()) }
    }

    fn dispatch(&mut self, git_ref: &str, base: &str) -> Result<Sent, String> {
        // The train always goes first: a member run never takes the gap between two workflows of a train's CI sequence.
        match reservation::wait_real(Duration::from_secs(45 * 60), &mut std::io::stderr())? {
            reservation::Free::Yes { .. } => {}
            reservation::Free::StillHeld { holder } => return Ok(Sent::Busy(format!("a train's CI sequence holds the runners: {holder}"))),
        }
        let req = dispatch::parse_request(WORKFLOW, git_ref, &[format!("base={base}")], MAX_DISPATCH_WAIT, None)?;
        let root = self.root.clone();
        let read = move |name: &str| std::fs::read_to_string(root.join(".gitea/workflows").join(name)).ok();
        let mut clock = dispatch::system_clock();
        let target = dispatch::Target {
            server: &self.server,
            repo: &self.repo,
            token: &self.token,
        };
        match dispatch::run(&mut self.http, &target, &req, &read, &mut clock, &mut std::io::stderr())? {
            dispatch::Dispatched::Yes { .. } => Ok(Sent::Yes),
            dispatch::Dispatched::StillBusy { last } => Ok(Sent::Busy(last)),
        }
    }

    fn find_run(&mut self, commit: &str) -> Result<Option<u64>, String> {
        // The run exists within seconds of the dispatch; ask for up to two minutes (bounded: ER-32).
        for attempt in 0..12 {
            let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
            let page = c.runs_page_where(1, &format!("head_sha={commit}"))?;
            if let Some(r) = page
                .runs
                .iter()
                .filter(|r| r.workflow() == WORKFLOW && r.head_sha == commit)
                .map(|r| r.id)
                .max()
            {
                return Ok(Some(r));
            }
            if attempt < 11 {
                std::thread::sleep(Duration::from_secs(10));
            }
        }
        Ok(None)
    }

    fn wait(&mut self, run: u64) -> Result<RunEnd, String> {
        let mut cmd = command(Self::me()?);
        cmd.args(["wait", "--ci-run", &run.to_string(), "--max", MAX_RUN_WAIT]);
        // The wait ends at its own --max; the outer bound only ends a hang (ER-32). What decides is the run's own state.
        let _ = run_bounded(cmd, Limits::new(Duration::from_secs(2 * 3600 + 300))).map_err(|e| format!("`dev wait` could not run: {e}"))?;
        let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
        match c.run(run)? {
            Some(r) if r.finished() => Ok(RunEnd::Finished),
            Some(_) => Ok(RunEnd::StillRunning),
            None => Err(format!("run {run} is not listed")),
        }
    }

    fn jobs(&mut self, run: u64) -> Result<Vec<JobRow>, String> {
        let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
        let jobs: Vec<TimedJob> = c.timed_jobs(run)?.ok_or_else(|| format!("run {run} has no job list"))?;
        let created = c.run(run)?.and_then(|r| time::parse_ms(&r.created_at));
        Ok(jobs
            .into_iter()
            .filter(|j| !j.is_caller())
            .map(|j| JobRow {
                phases: (!j.steps.is_empty()).then(|| phases_of(created, j.start, &j.steps)),
                name: j.name,
                conclusion: j.conclusion,
            })
            .collect())
    }

    fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String> {
        let report = {
            let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
            let req = triage::Request {
                run,
                against: None,
                app: None,
                days: 14,
                history: true,
                repo: self.repo.clone(),
                now_secs: time::now_secs(),
            };
            triage::collect(&mut c, &req, &mut |l| eprintln!("{l}"))?
        };
        let http: &mut dyn Http = &mut self.http;
        Ok(failures_of(&report, run, http, &self.server, &self.repo, &self.token))
    }

    fn delete(&mut self, git_ref: &str) {
        if let Err(e) = trial::delete(&self.git(), git_ref) {
            eprintln!("::warning::other-os: {git_ref} could not be deleted ({e}); run `./dev ci trial-prune`");
        }
    }
}

/// Asks the tool's own planner (`dev plan <base> <head>`) what the member reaches: the selection CI uses (rule 55).
fn ask_plan(root: &Path, base: &str, head: &str) -> Result<Reach, String> {
    // The planner is this tool, or `PADDOCK_MERGE_CHECK_PLAN` (tests point it at a stand-in, as `PADDOCK_MERGE_CHECK_GATES` does for the
    // fast gates); its answer still decides whether the member reaches anything.
    let program = match std::env::var_os("PADDOCK_MERGE_CHECK_PLAN") {
        Some(p) => PathBuf::from(p),
        None => Gitea::me()?,
    };
    let mut cmd = command(program);
    cmd.args(["plan", base, head]).current_dir(root);
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(300))).map_err(|e| format!("`dev plan` could not run: {e}"))?;
    if !out.success() {
        return Err(format!("`dev plan {base} {head}` failed: {}", out.stderr_text().trim()));
    }
    let text = out.stdout_text();
    let plan: Value = text
        .lines()
        .find(|l| l.starts_with('{'))
        .and_then(|l| serde_json::from_str(l).ok())
        .ok_or("`dev plan` printed no JSON")?;
    Ok(reach_of(&plan))
}

/// The real step for `merge check --gates`: a missing token or trial folder is `not measured` (the exit 6 when enforced), with what
/// to set. The plan decides first whether the member reaches anything, so a documentation-only member needs no token.
pub fn run_real(root: &Path, _git: &Git, merge: &Merge, enforced: bool) -> Step {
    let Some(commit) = merge.commit.as_deref() else {
        return not_run("the merge has no commit");
    };
    match Gitea::open(root, &merge.label, commit) {
        Err(why) => match merge.parents.first().map(|b| ask_plan(root, b, commit)) {
            Some(Ok(Reach::Nothing)) => not_run("the plan reaches no project for this member (documentation only)"),
            _ => unmeasured(enforced, &why),
        },
        Ok(mut g) => run(merge, &mut g, enforced),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // The gate is enforced since 2026-11-21 (user): a train merge it could not measure must not go on. Un-enforcing it is a decision of
    // the user, so this fails the build until that is decided.
    const _: () = assert!(ENFORCED);

    fn job(name: &str, conclusion: &str) -> JobRow {
        JobRow {
            name: name.into(),
            conclusion: conclusion.into(),
            phases: None,
        }
    }

    fn fail(job: &str, test: &str, blame: Blame) -> Failure {
        Failure {
            what: format!("{job}: {test}"),
            projects: BTreeSet::new(),
            files: Vec::new(),
            blame,
        }
    }

    fn row(rows: &[Row], os: Os) -> &Row {
        rows.iter().find(|r| r.os == os).unwrap()
    }

    #[test]
    fn jobs_belong_to_the_os_their_matrix_names() {
        assert_eq!(Os::of_job("paddock-dev (windows-latest)"), Some(Os::Windows));
        assert_eq!(Os::of_job("crates (ubuntu-latest)"), Some(Os::Linux));
        assert_eq!(Os::of_job("app dyno (macos-latest)"), None);
        assert_eq!(Os::of_job("plan (affected projects)"), None);
    }

    #[test]
    fn the_plan_decides_what_a_member_reaches() {
        assert_eq!(reach_of(&json!({"crates": true})), Reach::Projects);
        assert_eq!(reach_of(&json!({"all": true})), Reach::Projects);
        assert_eq!(reach_of(&json!({"apps": ["dyno"], "crates": false})), Reach::Projects);
        assert_eq!(reach_of(&json!({"docs": true, "crates": false, "apps": []})), Reach::Nothing);
        assert_eq!(reach_of(&json!({})), Reach::Nothing);
    }

    #[test]
    fn a_green_run_is_clean_with_evidence_and_an_os_without_jobs_is_not_applicable() {
        let r = rows(
            7,
            "abc",
            &[job("crates (ubuntu-latest)", "success"), job("plan", "success")],
            &[],
            &[],
        );
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::Clean(_)));
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::NotApplicable(_)));
    }

    /// Only NEW failures fail a row: a failure also on main is listed, not blamed (the base is not the member's).
    #[test]
    fn only_new_failures_fail_a_row_and_failures_also_on_main_are_listed() {
        let jobs = [
            job("paddock-dev (windows-latest)", "failure"),
            job("crates (ubuntu-latest)", "failure"),
        ];
        let fails = [
            fail("paddock-dev (windows-latest)", "places::a", Blame::OnMain),
            fail("paddock-dev (windows-latest)", "wrapper::b", Blame::OnMain),
            fail("crates (ubuntu-latest)", "proc::c", Blame::New),
        ];
        let r = rows(7, "abc", &jobs, &fails, &[]);
        let win = row(&r, Os::Windows);
        assert!(matches!(win.outcome, Outcome::Clean(_)), "{:?}", win.outcome);
        assert_eq!(win.known.len(), 2);
        match &row(&r, Os::Linux).outcome {
            Outcome::Findings(f) => assert_eq!(f.findings().as_slice().len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn what_cannot_be_blamed_is_not_measured_never_a_pass() {
        // Unknown blame, a failed job with no test read, a cancelled job, an unfinished job.
        let r = rows(
            1,
            "a",
            &[job("x (windows-latest)", "failure")],
            &[fail("x (windows-latest)", "t", Blame::Unknown)],
            &[],
        );
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::NotMeasured(_)));
        let r = rows(1, "a", &[job("x (windows-latest)", "failure")], &[], &[]);
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::NotMeasured(_)));
        let r = rows(1, "a", &[job("x (ubuntu-latest)", "cancelled")], &[], &[]);
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::NotMeasured(_)));
        let r = rows(1, "a", &[job("x (ubuntu-latest)", "")], &[], &[]);
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::NotMeasured(_)));
    }

    /// A table CI: records what was asked, answers from fields.
    struct Fake {
        reach: Result<Reach, String>,
        sent: Result<Sent, String>,
        run: Option<u64>,
        end: RunEnd,
        jobs: Vec<JobRow>,
        failures: Vec<Failure>,
        changed: Vec<String>,
        asked: Vec<String>,
    }

    impl Fake {
        fn green() -> Fake {
            Fake {
                reach: Ok(Reach::Projects),
                sent: Ok(Sent::Yes),
                run: Some(9),
                end: RunEnd::Finished,
                jobs: vec![job("crates (ubuntu-latest)", "success"), job("crates (windows-latest)", "success")],
                failures: Vec::new(),
                changed: vec!["crates/paddock-view/src/lib.rs".into()],
                asked: Vec::new(),
            }
        }
    }

    impl Runs for Fake {
        fn changed_files(&mut self, _base: &str, _head: &str) -> Result<Vec<String>, String> {
            Ok(self.changed.clone())
        }
        fn reaches(&mut self, base: &str, head: &str) -> Result<Reach, String> {
            self.asked.push(format!("plan {base}..{head}"));
            self.reach.clone()
        }
        fn publish(&mut self, commit: &str, git_ref: &str) -> Result<(), String> {
            self.asked.push(format!("publish {commit} {git_ref}"));
            Ok(())
        }
        fn dispatch(&mut self, git_ref: &str, base: &str) -> Result<Sent, String> {
            self.asked.push(format!("dispatch {git_ref} base={base}"));
            self.sent.clone()
        }
        fn find_run(&mut self, _commit: &str) -> Result<Option<u64>, String> {
            Ok(self.run)
        }
        fn wait(&mut self, _run: u64) -> Result<RunEnd, String> {
            Ok(self.end)
        }
        fn jobs(&mut self, _run: u64) -> Result<Vec<JobRow>, String> {
            Ok(self.jobs.clone())
        }
        fn failures(&mut self, _run: u64) -> Result<Vec<Failure>, String> {
            Ok(self.failures.clone())
        }
        fn delete(&mut self, git_ref: &str) {
            self.asked.push(format!("delete {git_ref}"));
        }
    }

    fn merge() -> Merge {
        Merge::new(
            vec![
                "1111111111111111111111111111111111111111".into(),
                "2222222222222222222222222222222222222222".into(),
            ],
            "t",
            "m",
        )
        .with_commit(Some("abcdef0123456789abcdef0123456789abcdef01".into()))
    }

    #[test]
    fn a_member_the_plan_does_not_reach_is_not_run_and_nothing_is_pushed() {
        let mut f = Fake::green();
        f.reach = Ok(Reach::Nothing);
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["outcome"], "not run");
        assert_eq!(f.asked.len(), 1, "{:?}", f.asked);
    }

    #[test]
    fn the_merge_is_pushed_dispatched_with_the_train_before_as_base_and_the_scratch_ref_is_always_deleted() {
        let mut f = Fake::green();
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["outcome"], "passed");
        assert_eq!(s.exit, Exit::Ok);
        let sref = "trial/member-abcdef012345";
        assert_eq!(
            f.asked,
            vec![
                "plan 1111111111111111111111111111111111111111..abcdef0123456789abcdef0123456789abcdef01".to_string(),
                format!("publish abcdef0123456789abcdef0123456789abcdef01 {sref}"),
                format!("dispatch {sref} base=1111111111111111111111111111111111111111"),
                format!("delete {sref}"),
            ]
        );
        // Also when the dispatch did not happen.
        let mut f = Fake::green();
        f.sent = Ok(Sent::Busy("run 8200".into()));
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["outcome"], "not measured");
        assert!(f.asked.last().unwrap().starts_with("delete "));
    }

    /// A planted new failure on Windows is red there and green on Linux; a busy runner or a still-running run is not measured; and
    /// the exit changes only when enforced.
    #[test]
    fn a_new_failure_on_one_os_is_red_there_and_the_exit_changes_only_when_enforced() {
        for (enforced, want) in [(false, Exit::Ok), (true, Exit::Findings)] {
            let mut f = Fake::green();
            f.jobs = vec![job("crates (ubuntu-latest)", "success"), job("crates (windows-latest)", "failure")];
            f.failures = vec![fail("crates (windows-latest)", "sh_fixture", Blame::New)];
            let s = run(&merge(), &mut f, enforced);
            assert_eq!(s.exit, want, "enforced={enforced}");
            assert_eq!(s.data["outcome"], "failed");
            assert_eq!(s.data["rows"][0]["os"], "linux");
            assert_eq!(s.data["rows"][0]["outcome"]["status"], "clean");
            assert_eq!(s.data["rows"][1]["outcome"]["status"], "findings");
            let text = s.text.join("\n");
            let want_line = if enforced {
                "::error::member gate other-os FAILED on windows"
            } else {
                "::warning::member gate other-os FAILED on windows"
            };
            assert!(text.contains(want_line), "{text}");
        }
        let mut f = Fake::green();
        f.end = RunEnd::StillRunning;
        assert_eq!(run(&merge(), &mut f, true).exit, Exit::NotMeasured);
        let mut f = Fake::green();
        f.run = None;
        assert_eq!(run(&merge(), &mut f, true).data["outcome"], "not measured");
        let mut f = Fake::green();
        f.reach = Err("no plan".into());
        assert_eq!(run(&merge(), &mut f, true).data["outcome"], "not measured");
    }

    // ---- Self-dispatch, judging and logging (2026-11-21) ----

    /// A test that failed both tries (blame New) blocks, whatever the member touched (ER-12).
    #[test]
    fn a_test_that_fails_both_tries_blocks_and_exits_with_findings() {
        let mut f = Fake::green();
        f.jobs = vec![
            job("paddock-dev (windows-latest)", "failure"),
            job("crates (ubuntu-latest)", "success"),
        ];
        f.failures = vec![fail("paddock-dev (windows-latest)", "sccache_eviction::evicts_oldest", Blame::New)];
        f.changed = vec!["docs/rules.md".into()];
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.exit, Exit::Findings);
        assert_eq!(s.data["outcome"], "failed");
        assert!(s.text.join("\n").contains("NEW: "), "{:?}", s.text);
    }

    /// A FLAKY test in a feature the member does not touch passes and is recorded; one in a touched feature blocks.
    #[test]
    fn a_flaky_test_blocks_only_when_the_member_touches_its_feature() {
        let mut f = Fake::green();
        f.jobs = vec![
            job("paddock-dev (windows-latest)", "failure"),
            job("crates (ubuntu-latest)", "success"),
        ];
        f.failures = vec![fail("paddock-dev (windows-latest)", "home_dashboard::refreshes", Blame::Flaky)];
        f.changed = vec!["tools/paddock-dev/src/sccache_eviction.rs".into()];
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.exit, Exit::Ok, "{:?}", s.text);
        assert_eq!(s.data["outcome"], "passed");
        assert_eq!(
            s.data["rows"][1]["flakyRecorded"][0],
            "paddock-dev (windows-latest): home_dashboard::refreshes"
        );
        assert!(s.text.join("\n").contains("FLAKY (recorded, not blocking"), "{:?}", s.text);
        // The same flake, now with the member touching `home_dashboard`.
        f.changed = vec!["tools/paddock-dev/src/home_dashboard/mod.rs".into()];
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.exit, Exit::Findings);
        assert_eq!(s.data["rows"][1]["flakyRecorded"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn the_feature_of_a_test_is_its_module_and_an_unknown_one_counts_as_touched() {
        let c = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let t = "j (windows-latest): sccache_eviction::tests::evicts";
        assert!(feature_touched(t, &c(&["tools/paddock-dev/tests/it/sccache_eviction.rs"])));
        assert!(feature_touched(t, &c(&["crates/sccache-eviction/src/lib.rs"])));
        assert!(feature_touched(t, &c(&["a/sccache_eviction/mod.rs"])));
        assert!(!feature_touched(t, &c(&["docs/rules.md", "tools/paddock-dev/src/plan.rs"])));
        // A binary id before the name (`paddock-service per_machine::tests::x`) is not a module.
        assert!(feature_touched(
            "j: paddock-service per_machine::tests::x",
            &c(&["a/per_machine.rs"])
        ));
        assert!(!feature_touched(
            "j: paddock-service per_machine::tests::x",
            &c(&["a/paddock_service.rs"])
        ));
        // Unknown module: no `::`, or only `tests`: blocks.
        assert!(feature_touched("j: some spec name", &c(&["docs/a.md"])));
        assert!(feature_touched("j: tests::x", &c(&["docs/a.md"])));
        assert!(feature_touched("j: the job failed outside any test", &[]));
    }

    /// Positive control (ER-33): a run the gate never dispatched cannot pass. Without the dispatch there is no run to find, and the
    /// row is `not measured` with the exit 6; with it, the dispatch is the one the gate asked for.
    #[test]
    fn without_the_dispatch_the_row_cannot_pass() {
        let mut f = Fake::green();
        f.run = None; // what no dispatch gives: no run of ci.yml on the ref
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.data["outcome"], "not measured");
        assert_eq!(s.exit, Exit::NotMeasured);
        let mut f = Fake::green();
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.data["outcome"], "passed");
        assert!(
            f.asked.iter().any(|a| a.starts_with("dispatch ")),
            "a pass needs the gate's own dispatch: {:?}",
            f.asked
        );
    }

    #[test]
    fn a_busy_runner_and_a_missing_token_or_folder_are_not_measured_with_exit_6() {
        let mut f = Fake::green();
        f.sent = Ok(Sent::Busy("run 8200".into()));
        assert_eq!(run(&merge(), &mut f, ENFORCED).exit, Exit::NotMeasured);
        // No token: nothing is dispatched; the cause and the next step are named.
        let err = Gitea::open_with(Path::new("/x"), "m", "abcdef0123456789", None, Ok(PathBuf::from("/d/trials")))
            .err()
            .unwrap();
        assert!(err.contains("GITEA_TOKEN is not set"), "{err}");
        let s = unmeasured(ENFORCED, &err);
        assert_eq!(s.exit, Exit::NotMeasured);
        let text = s.text.join("\n");
        assert!(
            text.contains("::error::member gate other-os NOT MEASURED") && text.contains("Next step"),
            "{text}"
        );
        // No trial folder: the same.
        let err = Gitea::open_with(
            Path::new("/x"),
            "m",
            "abcdef0123456789",
            Some("t".into()),
            Err("no trial folder is set".into()),
        )
        .err()
        .unwrap();
        assert!(err.contains("no trial folder"), "{err}");
    }

    #[test]
    fn the_trial_folder_is_one_per_merge_and_is_removed_without_touching_the_root() {
        assert_eq!(
            trial_folder_name("fix/member gate", "abcdef0123456789"),
            "fix-member-gate-abcdef012345"
        );
        assert_eq!(trial_folder_name("", "abcdef0123456789"), "member-abcdef012345");
        assert_ne!(
            trial_folder_name("a", "1111111111111111"),
            trial_folder_name("a", "2222222222222222")
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("trials");
        std::fs::create_dir_all(&root).unwrap();
        let g = Gitea::open_with(Path::new("/x"), "m", "abcdef0123456789", Some("t".into()), Ok(root.clone())).unwrap();
        let mine = root.join("m-abcdef012345");
        assert!(mine.is_dir());
        std::fs::write(root.join("keep"), "other merge").unwrap();
        drop(g);
        assert!(!mine.exists(), "its own folder is removed at the end");
        assert!(root.join("keep").exists(), "the root and other merges' folders stay");
    }

    #[test]
    fn the_run_is_logged_with_its_phases_and_a_verdict_per_os() {
        // queue = job start - run creation; steps split by name.
        let steps = vec![
            ("Set up job".to_string(), Some(5.0)),
            ("Run ./.gitea/actions/build-jobs".to_string(), Some(1.0)),
            ("Run ./.gitea/actions/setup-rust".to_string(), Some(30.0)),
            ("Build".to_string(), Some(120.0)),
            ("Clippy".to_string(), Some(60.0)),
            ("Tests (nextest)".to_string(), Some(300.0)),
        ];
        let p = phases_of(Some(1_000.0), Some(13_000.0), &steps);
        assert_eq!(
            p,
            Phases {
                queue: 12,
                setup: 36,
                compile: 180,
                tests: 300
            }
        );
        assert_eq!(p.text(), "queue 12s, setup 36s, compile 3m00s, tests 5m00s");
        let mut j = job("crates (ubuntu-latest)", "success");
        j.phases = Some(p);
        let mut w = job("crates (windows-latest)", "success");
        w.phases = Some(Phases {
            queue: 600,
            setup: 10,
            compile: 20,
            tests: 30,
        });
        let r = rows(77, "abc", &[j, w], &[], &[]);
        let s = step_of(&r, 77, "trial/member-x", "1111111", true);
        let text = s.text.join("\n");
        assert!(text.contains("run 77 on trial/member-x"), "{text}");
        assert!(
            text.contains("linux: clean (1 jobs, queue 12s, setup 36s, compile 3m00s, tests 5m00s)"),
            "{text}"
        );
        assert!(text.contains("windows: clean (1 jobs, queue 10m00s"), "{text}");
        assert_eq!(s.data["rows"][1]["phases"]["queueSecs"], 600);
    }

    #[test]
    fn the_scratch_ref_is_in_the_trial_namespace_that_trial_prune_cleans() {
        assert_eq!(scratch_ref("abcdef0123456789"), "trial/member-abcdef012345");
        assert!(scratch_ref("ab").starts_with("trial/"));
    }
}
