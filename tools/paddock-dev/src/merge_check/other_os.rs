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
//! **Two parts, two runs (user, 2026-11-21: trials use spare lanes).** The trial is one `ci.yml` run per OS, each its own handle: the
//! Linux part (`os=linux`) is dispatched at once, also while a train's CI holds the runners, only beside a train
//! that is working, with no Linux job queued or waiting and as many lanes free as the member's own Linux jobs ([`linux_may_start`]:
//! a trial never jumps a running train, ER-24; the lanes are `conformance/ci/runner-slots.json`'s, six since 2026-11-20; every job
//! keeps its lanes-times-threads share, ER-48); the Windows part (`os=windows`) waits for the train's reservation by progress. Each OS's row is judged from its
//! own run as soon as that run ends and the Linux verdict is reported at once, but the member stays pending (the command has not
//! returned) until the Windows part is judged: a row is never "clean" for the member before both parts are in. The Mac runner is
//! not judged here, so no run asks for it. The split is ci.yml's `os` input, handled by the planner (`src/plan_os.rs`).
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
use crate::ci_runs::client::{Client, RunInfo, TimedJob};
use crate::ci_runs::{dispatch, reservation, time, triage, trial};
use crate::plan_os;
use crate::queue::ports::{Blame, Failure};
use crate::queue::real::{Child, GitTrain, failures_of};
use crate::wait::{Clock, CurlHttp, Http};
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

/// How long a part's run may take once it is dispatched (a full affected plan of one OS on the real runners).
const MAX_RUN_WAIT: Duration = Duration::from_secs(2 * 3600);
/// How often the gate looks at its two parts.
pub const TICK: Duration = Duration::from_secs(15);

/// Why the Mac has no row: it is the developer's own machine, and its tests are the local pre-push run (not a CI-like clone).
pub const MAC_NOT_JUDGED: &str = "dev OS; covered by the local pre-push run, not a CI-like clone";

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
    /// The runner labels it ran on (`ubuntu-latest`, with a matrix value filled in): the OS of a job whose name names none (the
    /// conformance, tools, crates and ui jobs).
    pub labels: Vec<String>,
}

impl JobRow {
    /// The OS the job ran on: its name's matrix label, else its runner labels; `None` for a job on another machine (macOS) or one
    /// with no label (the row says "not judged", never "clean").
    pub fn os(&self) -> Option<Os> {
        Os::of_job(&self.name).or_else(|| self.labels.iter().find_map(|l| Os::of_job(l)))
    }

    /// Whether it is the plan job, whose success is the precondition of every row.
    pub fn is_plan(&self) -> bool {
        self.name.to_ascii_lowercase().starts_with("plan")
    }
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

/// What the plan said about the member's own change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// At least one project is affected.
    Projects,
    /// Nothing but documentation (or nothing) is affected.
    Nothing,
}

/// How a part's run was started: the run the dispatch created (its id came back), or only the highest run id before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// The server named the run it created: judge only this id (ER-51).
    Run(u64),
    /// The server did not: the run to judge is the one newer than this id that proves it is ours ([`pick_run`]).
    After(u64),
}

/// What one try to start a part gave. The gate tries again on every tick, so a part waits without blocking the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryStart {
    /// Dispatched.
    Started(Started),
    /// Not yet (a train holds the runner, or no Linux lane is spare); the text says what.
    Wait(String),
    /// Nothing progressed for the limit: the part cannot be measured; the text says why.
    Stalled(String),
}

/// The real CI as this step needs it. The real one is [`Gitea`]; tests give a table.
pub trait Runs {
    /// The plan (`dev plan <base> <head> --explain`) as JSON; `Err` when it could not be asked.
    fn plan(&mut self, base: &str, head: &str) -> Result<Value, String>;
    /// The files the member changes (`base..head`), to tell whether a flaky test's feature is touched.
    fn changed_files(&mut self, base: &str, head: &str) -> Result<Vec<String>, String>;
    /// Pushes `commit` to `git_ref` (a short-lived scratch ref).
    fn publish(&mut self, commit: &str, git_ref: &str) -> Result<(), String>;
    /// One try to dispatch [`WORKFLOW`] on `git_ref` with `base` and the `os` input of `part`: Linux at once on a spare lane, Windows
    /// after the train's reservation. Never blocks for long: a part that cannot start yet is [`TryStart::Wait`].
    fn try_dispatch(&mut self, part: Os, git_ref: &str, base: &str) -> Result<TryStart, String>;
    /// The id of the run this step dispatched for `part`: the handle of `started`, or the one run newer than it that is ours.
    fn find_run(&mut self, part: Os, git_ref: &str, commit: &str, started: &Started) -> Result<Option<u64>, String>;
    /// Whether the run has ended (any conclusion); one look.
    fn finished(&mut self, run: u64) -> Result<bool, String>;
    /// The run's own overall conclusion (`success`, `failure`, `cancelled`...).
    fn conclusion(&mut self, run: u64) -> Result<String, String>;
    /// The jobs of the run.
    fn jobs(&mut self, run: u64) -> Result<Vec<JobRow>, String>;
    /// The failures of the finished run, each blamed by `ci triage`.
    fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String>;
    /// Waits one tick (the shared clock) and says how long the gate has been going; the gate's loop is bounded by it (ER-32).
    fn tick(&mut self) -> Duration;
    /// Says a line now (progress: the Linux verdict as soon as it is in).
    fn report(&mut self, line: &str);
    /// Deletes the scratch ref (best effort: `ci trial-prune` catches the rest).
    fn delete(&mut self, git_ref: &str);
}

/// The run among `runs` that this step's dispatch made, when the server did not say: newer than `after` (the highest id before the
/// dispatch), `ci.yml`, on the commit, from a `workflow_dispatch` on exactly `git_ref`. An older run on the same commit (a train's,
/// a rerun, an earlier trial of the same merge) never counts. More than one is an error (nothing proves which is ours).
pub fn pick_run(runs: &[RunInfo], after: u64, git_ref: &str, commit: &str) -> Result<Option<u64>, String> {
    let ours: Vec<u64> = runs
        .iter()
        .filter(|r| {
            r.id > after
                && r.workflow() == WORKFLOW
                && r.head_sha == commit
                && r.event == "workflow_dispatch"
                && (r.head_branch == git_ref || r.git_ref() == format!("refs/heads/{git_ref}"))
        })
        .map(|r| r.id)
        .collect();
    match ours.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(*one)),
        many => Err(format!(
            "runs {many:?} all look like this dispatch (newer than run {after}, {WORKFLOW} on {git_ref} at {}), so none is proven to be ours",
            short(commit)
        )),
    }
}

/// The plan's answer as a [`Reach`], read from the plan's own output (rule 55, ER-22, ER-42): any true flag other than `docs` and
/// `conformance`, `all`, an app named, or conformance reached for a reason beyond the one it has in every plan
/// ([`crate::plan::CONFORMANCE_ALWAYS`], read from `why`). A plan that cannot be read this way (no `why`) reaches projects: when
/// unsure the gate runs.
pub fn reach_of(plan: &Value) -> Reach {
    let Some(obj) = plan.as_object() else {
        return Reach::Projects;
    };
    let flag = obj
        .iter()
        .any(|(k, v)| !matches!(k.as_str(), "docs" | "conformance") && v.as_bool() == Some(true));
    let apps = obj.get("apps").and_then(Value::as_array).is_some_and(|a| !a.is_empty());
    if flag || apps || conformance_reached(plan) {
        Reach::Projects
    } else {
        Reach::Nothing
    }
}

/// Whether the plan reaches conformance for a reason of its own (a change under `conformance/`, `lint/`...), not only because it is
/// in every plan. No `why` in the answer: reached (never skipped on a guess).
fn conformance_reached(plan: &Value) -> bool {
    if plan.get("conformance").and_then(Value::as_bool) != Some(true) {
        return false;
    }
    match plan.get("why").and_then(|w| w.get("conformance")).and_then(Value::as_array) {
        Some(reasons) => reasons.iter().any(|r| r.as_str() != Some(crate::plan::CONFORMANCE_ALWAYS)),
        None => true,
    }
}

/// One OS's row: the shared outcome, the jobs it is made of and the failures that were also on main.
#[derive(Debug)]
pub struct Row {
    /// The OS.
    pub os: Os,
    /// The outcome (C124).
    pub outcome: Outcome,
    /// The jobs of the run on this OS.
    pub jobs: Vec<String>,
    /// Failures that triage found also on main: still findings (a test that fails both tries blocks even then, ER-12, user
    /// 2026-11-20); the list only says which ones.
    pub known: Vec<String>,
    /// FLAKY tests (failed, then passed on their one retry) whose feature the member does not touch: recorded as fix items, not
    /// blamed (ER-12).
    pub flaky: Vec<String>,
    /// Where the jobs' time went, summed over the OS's jobs; `None` when no job had step timings.
    pub phases: Option<Phases>,
}

/// What the rows are judged against: the member's changed files, the plan for them and the run's own conclusion.
pub struct Ctx<'a> {
    /// The files the member changes.
    pub changed: &'a [String],
    /// The plan (`dev plan <base> <head> --explain`).
    pub plan: &'a Value,
    /// The run's overall conclusion (`success`, `failure`, `cancelled`...).
    pub run_conclusion: &'a str,
}

fn tool() -> ToolVersion {
    ToolVersion::new(concat!("paddock-dev merge check other-os ", env!("CARGO_PKG_VERSION")))
}

/// The job a failure belongs to: its `what` is `<job>: <test>`.
fn job_of(f: &Failure) -> &str {
    f.what.split(": ").next().unwrap_or("")
}

/// A name for comparing project, crate and file names: lower case, no `-`, `_` or `.`.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '-' | '_' | '.'))
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Whether the plan reaches `project` (a crate, an app, or a job's own name such as `ui`): through the plan's own graph, which
/// already follows dependents (`packages`: the crates the change reaches; `apps`; the project flags; `all`).
fn plan_reaches(plan: &Value, project: &str) -> bool {
    let p = norm(project);
    if plan.get("all").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    let listed = |v: Option<&Value>| {
        v.and_then(Value::as_array)
            .is_some_and(|a| a.iter().filter_map(Value::as_str).any(|n| norm(n) == p))
    };
    if listed(plan.get("apps")) {
        return true;
    }
    if plan
        .get("packages")
        .and_then(Value::as_object)
        .is_some_and(|m| m.values().any(|v| listed(Some(v))))
    {
        return true;
    }
    plan.as_object().is_some_and(|o| {
        o.iter().any(|(k, v)| {
            norm(k) == p
                && if k == "conformance" {
                    conformance_reached(plan)
                } else {
                    v.as_bool() == Some(true)
                }
        })
    })
}

/// The spec file a Playwright or Vitest test name carries (`[chromium] > e2e/tray.spec.js:5:3 > tray > x`, or a bare spec path).
fn spec_of(test: &str) -> Option<&str> {
    test.split(|c: char| c.is_whitespace() || c == '\u{203a}' || c == '>')
        .map(|w| w.split(':').next().unwrap_or(w))
        .find(|w| {
            let n = w.rsplit('/').next().unwrap_or(w);
            ["spec", "test"].iter().any(|k| n.contains(&format!(".{k}.")))
        })
}

/// Whether the member's change touches the feature of a failed test (ER-12: "the test itself or the source it exercises (its feature
/// or module: the files it drives, not any file of the same app package or crate)"; user, 2026-11-19, for the UI flakes). Two
/// steps. (1) The plan's own graph: a test whose projects the plan for the member does not reach is untouched. (2) Within a reached
/// project, the files the test drives: for a Playwright or Vitest test, its spec file's feature (the spec itself, and any file or
/// folder named like it, `home-dashboard.spec.ts` to `home-dashboard.component.ts`); for a Rust test, the files of its own crate or
/// app named like its modules (`a::b::name` gives `a` and `b`; `a.rs`, `a/mod.rs`, `tests/it/a.rs`). A change only in a dependency
/// of the test's crate does not count as its feature. A test whose feature cannot be told (a whole-job failure, no module, no spec)
/// counts as touched: when unsure it blocks.
pub fn feature_touched(f: &Failure, ctx: &Ctx) -> bool {
    if !f.projects.is_empty() && !f.projects.iter().any(|p| plan_reaches(ctx.plan, p)) {
        return false;
    }
    let test = f.what.split_once(": ").map_or(f.what.as_str(), |(_, t)| t);
    let changed = ctx.changed.iter().map(|c| c.replace('\\', "/"));
    if let Some(spec) = spec_of(test) {
        let stem = spec.rsplit('/').next().unwrap_or(spec).split('.').next().unwrap_or("");
        let stem = norm(stem);
        return !stem.is_empty()
            && changed
                .clone()
                .any(|c| c.ends_with(spec) || c.split('/').any(|comp| norm(comp.split('.').next().unwrap_or(comp)) == stem));
    }
    let id = test.split_whitespace().last().unwrap_or("");
    let mut segs: Vec<&str> = id.split("::").collect();
    if segs.len() < 2 {
        return true;
    }
    segs.pop();
    let mods: Vec<String> = segs
        .into_iter()
        .map(norm)
        .filter(|m| !matches!(m.as_str(), "tests" | "test" | "it" | ""))
        .collect();
    if mods.is_empty() {
        return true;
    }
    let scope: Vec<String> = f.projects.iter().map(|p| norm(p)).collect();
    changed.into_iter().any(|c| {
        let comps: Vec<&str> = c.split('/').collect();
        let in_scope = scope.is_empty() || comps.iter().any(|x| scope.contains(&norm(x)));
        in_scope
            && comps
                .iter()
                .any(|x| mods.contains(&norm(x.rsplit_once('.').map_or(*x, |(a, _)| a))))
    })
}

/// The jobs of the run that no row judges: on another machine (macOS is the integrator's own) or with no runner label. They are
/// said out loud, never counted as clean (ER-33).
pub fn not_judged(jobs: &[JobRow]) -> Vec<String> {
    jobs.iter()
        .filter(|j| !j.is_plan() && j.os().is_none())
        .map(|j| j.name.clone())
        .collect()
}

/// The rows of a finished run: for each OS, `NotMeasured` unless the plan job succeeded and the run was neither cancelled nor
/// unfinished (a run that never reached its tests says nothing), `NotApplicable` when the plan ran no job there (or skipped them
/// all), `NotMeasured` when a job did not finish or a failure cannot be blamed, `Findings` for every failure that failed both tries
/// (also when it also fails on main: ER-12) and for a FLAKY test whose feature the member touches, and `Clean` otherwise.
pub fn rows(run: u64, sha: &str, jobs: &[JobRow], failures: &[Failure], ctx: &Ctx) -> Vec<Row> {
    let subject = || Subject::CiRun {
        workflow: WORKFLOW.into(),
        run: RunId(run),
        sha: Sha::new(sha),
    };
    // The precondition of every row: the plan job ran and succeeded, so the jobs that exist are the jobs that were asked for.
    let precondition: Option<String> = match jobs.iter().find(|j| j.is_plan()) {
        None => Some("the run has no plan job, so nothing proves which jobs it should have run".to_string()),
        Some(j) if j.conclusion != "success" => Some(format!(
            "the plan job `{}` ended `{}`, so nothing proves which jobs it should have run",
            j.name,
            if j.conclusion.is_empty() { "unfinished" } else { &j.conclusion }
        )),
        Some(_) if matches!(ctx.run_conclusion, "cancelled" | "") => Some(format!(
            "the run ended `{}`, so it did not reach its tests",
            if ctx.run_conclusion.is_empty() {
                "unfinished"
            } else {
                ctx.run_conclusion
            }
        )),
        Some(_) => None,
    };
    [Os::Linux, Os::Windows]
        .into_iter()
        .map(|os| {
            let mine: Vec<&JobRow> = jobs.iter().filter(|j| !j.is_plan() && j.os() == Some(os)).collect();
            let names: Vec<String> = mine.iter().map(|j| j.name.clone()).collect();
            let os_of_failure = |f: &Failure| jobs.iter().find(|j| j.name == job_of(f)).and_then(JobRow::os);
            let fails: Vec<&Failure> = failures.iter().filter(|f| os_of_failure(f) == Some(os)).collect();
            let known: Vec<String> = fails
                .iter()
                .filter(|f| matches!(f.blame, Blame::OnMain))
                .map(|f| f.what.clone())
                .collect();
            // A FLAKY test (passed on its one retry) whose feature the member does not touch is a record, not a finding (ER-12).
            let flaky: Vec<String> = fails
                .iter()
                .filter(|f| f.retried && !matches!(f.blame, Blame::Unknown) && !feature_touched(f, ctx))
                .map(|f| f.what.clone())
                .collect();
            let phases = mine.iter().filter_map(|j| j.phases).reduce(Phases::combine);
            let outcome = if let Some(why) = &precondition {
                Outcome::NotMeasured(Reason::new(format!("{why} ({})", os.name())))
            } else if mine.is_empty() {
                Outcome::NotApplicable(Reason::new(format!("the plan ran no job on {} for this member", os.name())))
            } else if mine.iter().all(|j| j.conclusion == "skipped") {
                Outcome::NotApplicable(Reason::new(format!("the plan skipped every job on {} for this member", os.name())))
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
                    "`{}` failed and nothing proves whose failure it is (triage: unknown)",
                    f.what
                )))
            } else {
                let findings: Vec<Finding> = fails
                    .iter()
                    .filter(|f| !flaky.contains(&f.what))
                    .map(|f| {
                        if matches!(f.blame, Blame::OnMain) {
                            Finding::failing(format!(
                                "{} (also failing on main: a test that fails both tries blocks even then, ER-12)",
                                f.what
                            ))
                        } else {
                            Finding::failing(f.what.clone())
                        }
                    })
                    .collect();
                measure(
                    CheckId::new(format!("member-gate-other-os:{}", os.name())),
                    subject(),
                    tool(),
                    UnixSeconds::now(),
                    || Ok(findings),
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
pub fn step_of(rows: &[Row], not_judged: &[String], runs: &[(Os, Option<u64>)], git_ref: &str, base: &str, enforced: bool) -> Step {
    let (level, report_only) = wording(enforced);
    let run_of = |os: Os| runs.iter().find(|(o, _)| *o == os).and_then(|(_, r)| *r);
    let named: Vec<String> = runs
        .iter()
        .map(|(os, r)| format!("{} {}", os.name(), r.map_or("not dispatched".to_string(), |id| format!("run {id}"))))
        .collect();
    let mut text = vec![format!(
        "member gate other-os (base {base}, {} on {git_ref}, real CI runners):",
        named.join(", ")
    )];
    let mut exit = Exit::Ok;
    let mut data = Vec::new();
    for r in rows {
        let also = if r.known.is_empty() {
            String::new()
        } else {
            format!("; {} of its failures also on main (still blocking, ER-12)", r.known.len())
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
                "{level}::member gate other-os NOT MEASURED on {}: {}. Next step: read the run yourself (`./dev ci triage <run>`); a member whose other-OS tests did not give a verdict is not a member that passed.{report_only}",
                r.os.name(),
                why.as_str()
            )),
            _ => {}
        }
        exit = worst(exit, cost(enforced, &r.outcome));
        data.push(json!({
            "os": r.os.name(),
            "run": run_of(r.os),
            "outcome": r.outcome.json(),
            "jobs": r.jobs,
            "alsoOnMain": r.known,
            "flakyRecorded": r.flaky,
            "phases": r.phases.map(|p| json!({"queueSecs": p.queue, "setupSecs": p.setup, "compileSecs": p.compile, "testSecs": p.tests})),
        }));
    }
    // The Mac is the dev OS: no run asks for it, and the row says so, so its absence is never read as a pass.
    text.push(format!("  macos: not judged ({MAC_NOT_JUDGED})"));
    if !not_judged.is_empty() {
        text.push(format!(
            "  not judged (no Linux or Windows runner: macOS is the integrator's own machine, or the job names no runner): {}",
            not_judged.join(", ")
        ));
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
        data: json!({"outcome": word, "base": base, "runs": {"linux": run_of(Os::Linux), "windows": run_of(Os::Windows)}, "ref": git_ref, "enforced": enforced, "rows": data, "macos": {"outcome": "not judged", "why": MAC_NOT_JUDGED}, "notJudged": not_judged}),
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

/// The step when the fast gates already failed: nothing is dispatched, and the row is `not measured`, never a pass.
pub fn not_dispatched_after_failed_gates(enforced: bool) -> Step {
    unmeasured(
        enforced,
        "the fast gates already failed on this merge, so no trial was dispatched (it would spend the real runners on a member that must be fixed first)",
    )
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
    let plan = match runs.plan(base, commit) {
        Ok(p) => p,
        Err(e) => return unmeasured(enforced, &format!("the plan could not be asked: {e}")),
    };
    if reach_of(&plan) == Reach::Nothing {
        return not_run("the plan reaches no project for this member (documentation only)");
    }
    let git_ref = scratch_ref(commit);
    let step = measure_on_runners(runs, commit, base, &git_ref, &plan, enforced);
    runs.delete(&git_ref);
    step
}

/// One part's progress through the gate.
enum Part {
    /// Not dispatched yet.
    Todo,
    /// Dispatched: the handle and when (the gate's elapsed time).
    Running { run: u64, since: Duration },
    /// Judged (or not measurable): its row, its run and the jobs nothing judges.
    Done {
        row: Box<Row>,
        run: Option<u64>,
        not_judged: Vec<String>,
    },
}

/// A row for an OS that could not be measured, with the cause.
fn unmeasured_row(os: Os, why: String) -> Row {
    Row {
        os,
        outcome: Outcome::NotMeasured(Reason::new(why)),
        jobs: Vec::new(),
        known: Vec::new(),
        flaky: Vec::new(),
        phases: None,
    }
}

/// What an OS part's run says: its row (from the run's own jobs, failures and conclusion) and the jobs no row judges.
fn judge_part(
    runs: &mut dyn Runs,
    os: Os,
    run: u64,
    commit: &str,
    changed: &[String],
    plan: &Value,
) -> Result<(Box<Row>, Vec<String>), String> {
    let conclusion = runs
        .conclusion(run)
        .map_err(|e| format!("the conclusion of run {run} could not be read: {e}"))?;
    let jobs = runs
        .jobs(run)
        .map_err(|e| format!("the jobs of run {run} could not be read: {e}"))?;
    let failures = runs
        .failures(run)
        .map_err(|e| format!("the failures of run {run} could not be triaged: {e}"))?;
    let ctx = Ctx {
        changed,
        plan,
        run_conclusion: &conclusion,
    };
    let row = rows(run, commit, &jobs, &failures, &ctx)
        .into_iter()
        .find(|r| r.os == os)
        .ok_or_else(|| format!("no row for {}", os.name()))?;
    Ok((Box::new(row), not_judged(&jobs)))
}

/// What a tick does to one part.
enum Move {
    /// Nothing yet.
    Stay,
    /// The run was found: the part is running.
    Start(u64),
    /// The part ended: `Ok` its row and the jobs nothing judges, or why it cannot be measured; and its run, when it had one.
    Finish(Result<(Box<Row>, Vec<String>), String>, Option<u64>),
}

/// Runs the two parts side by side and returns the step. The loop looks at both on every tick, so the Linux part runs and is judged
/// while the Windows part still waits for the train; it ends when both are judged or cannot be measured, and every wait inside it has
/// its own bound (a stall limit for a part not started, [`MAX_RUN_WAIT`] for a started one). The member is clean only after both.
fn measure_on_runners(runs: &mut dyn Runs, commit: &str, base: &str, git_ref: &str, plan: &Value, enforced: bool) -> Step {
    let changed = match runs.changed_files(base, commit) {
        Ok(c) => c,
        Err(e) => return unmeasured(enforced, &format!("the member's changed files could not be listed: {e}")),
    };
    if let Err(e) = runs.publish(commit, git_ref) {
        return unmeasured(enforced, &format!("the merge could not be pushed to {git_ref}: {e}"));
    }
    let order = [Os::Linux, Os::Windows];
    let mut parts: Vec<Part> = order.iter().map(|_| Part::Todo).collect();
    let mut waiting_said = [false; 2];
    let mut elapsed = Duration::ZERO;
    while parts.iter().any(|p| !matches!(p, Part::Done { .. })) {
        for (i, os) in order.iter().copied().enumerate() {
            let mv = match &parts[i] {
                Part::Done { .. } => Move::Stay,
                Part::Todo => match runs.try_dispatch(os, git_ref, base) {
                    Err(e) => Move::Finish(
                        Err(format!("ci.yml could not be dispatched for {} on {git_ref}: {e}", os.name())),
                        None,
                    ),
                    Ok(TryStart::Stalled(why)) => Move::Finish(Err(format!("the {} part was not started: {why}", os.name())), None),
                    Ok(TryStart::Wait(why)) => {
                        if !waiting_said[i] {
                            runs.report(&format!("other-os: the {} part waits: {why}", os.name()));
                            waiting_said[i] = true;
                        }
                        Move::Stay
                    }
                    Ok(TryStart::Started(started)) => match runs.find_run(os, git_ref, commit, &started) {
                        Ok(Some(run)) => Move::Start(run),
                        Ok(None) => Move::Finish(
                            Err(format!(
                                "no run of ci.yml that this dispatch made appeared for the {} part on {git_ref}",
                                os.name()
                            )),
                            None,
                        ),
                        Err(e) => Move::Finish(Err(format!("the {} part's run could not be found: {e}", os.name())), None),
                    },
                },
                Part::Running { run, since } => {
                    let (run, since) = (*run, *since);
                    match runs.finished(run) {
                        Ok(true) => Move::Finish(judge_part(runs, os, run, commit, &changed, plan), Some(run)),
                        Ok(false) if elapsed.saturating_sub(since) >= MAX_RUN_WAIT => Move::Finish(
                            Err(format!(
                                "run {run} was still going after {}",
                                crate::wait::fmt_duration(MAX_RUN_WAIT)
                            )),
                            Some(run),
                        ),
                        Ok(false) => Move::Stay,
                        Err(e) => Move::Finish(Err(format!("run {run} could not be read: {e}")), Some(run)),
                    }
                }
            };
            match mv {
                Move::Stay => {}
                Move::Start(run) => {
                    runs.report(&format!(
                        "other-os: the {} part is ci.yml run {run} on {git_ref} (base {}); waiting at most {}",
                        os.name(),
                        short(base),
                        crate::wait::fmt_duration(MAX_RUN_WAIT)
                    ));
                    parts[i] = Part::Running { run, since: elapsed };
                }
                Move::Finish(result, run) => {
                    let (row, not_judged) = result.unwrap_or_else(|why| (Box::new(unmeasured_row(os, why)), Vec::new()));
                    let others_pending = parts.iter().enumerate().any(|(j, p)| j != i && !matches!(p, Part::Done { .. }));
                    // The verdict of the part that is in is said at once; the member is not clean until the others are judged too.
                    runs.report(&format!(
                        "other-os: {} {}{}{}",
                        os.name(),
                        row.outcome.name(),
                        row.phases.map_or(String::new(), |p| format!(" ({})", p.text())),
                        if others_pending {
                            " (the member stays pending until the other OS part is judged)"
                        } else {
                            ""
                        }
                    ));
                    parts[i] = Part::Done { row, run, not_judged };
                }
            }
        }
        if parts.iter().any(|p| !matches!(p, Part::Done { .. })) {
            elapsed = runs.tick();
        }
    }
    let mut rows = Vec::new();
    let mut handles = Vec::new();
    let mut judged_not = Vec::new();
    for (os, p) in order.iter().copied().zip(parts) {
        if let Part::Done { row, run, not_judged } = p {
            rows.push(*row);
            handles.push((os, run));
            judged_not.extend(not_judged);
        }
    }
    judged_not.sort();
    judged_not.dedup();
    step_of(&rows, &judged_not, &handles, git_ref, short(base), enforced)
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
    /// The gate's own clock: [`Runs::tick`] sleeps on it and reports how long the gate has been going.
    clock: crate::wait::SystemClock,
    /// Per part, whether the train's CI still moves while the part waits for it ([`reservation::Stall`]).
    stalls: std::collections::BTreeMap<Os, reservation::Stall>,
    /// How many Linux jobs the member's own Linux part will run (from the plan, set by [`Runs::plan`]); the Linux part starts beside a
    /// running train only when that many lanes are spare.
    linux_need: usize,
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
            clock: crate::wait::SystemClock::new(),
            stalls: std::collections::BTreeMap::new(),
            linux_need: usize::MAX,
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

impl Gitea {
    /// The Linux lanes in use now across the unfinished runs, and whether a train is working: `(use, train_running, lanes)`. The lanes
    /// are `conformance/ci/runner-slots.json`'s (one place, ER-48). A job counts as using a lane while it is running, queued or
    /// waiting for its turn; a train is working when an unfinished run on `train/*` has a job in progress.
    fn linux_lane_use(&mut self) -> Result<(LaneUse, bool, usize), String> {
        let lanes = dispatch::linux_slots(&self.root)?;
        let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
        let unfinished: Vec<(u64, bool)> = c
            .runs_page(1)?
            .runs
            .iter()
            .filter(|r| !r.finished())
            .map(|r| (r.id, r.git_ref().starts_with("refs/heads/train/")))
            .collect();
        let mut total = LaneUse::default();
        let mut train_running = false;
        for (id, is_train) in unfinished {
            // per-item: this Gitea's Actions API lists jobs per run (`/runs/<id>/jobs`, the only jobs call the tool uses); a
            // repository-wide jobs list is not part of it. Unfinished runs are few (a train's, this gate's own two), and this is
            // read only while a train holds the runners, once per 15 s tick (ER-20).
            if let Some(jobs) = c.timed_jobs(id)? {
                total = total.plus(busy_linux_jobs(&jobs));
                train_running |= is_train && jobs.iter().any(|j| j.status == "in_progress");
            }
        }
        Ok((total, train_running, lanes))
    }

    /// One dispatch of `ci.yml` with the `os` input of `part` (the planner narrows the run to it, `src/plan_os.rs`). The dispatch's own
    /// runner guard is asked for at most a few seconds, so a busy Windows runner is `Err(why)` for this tick, not a long block of the
    /// loop that judges the other part. `Ok(Ok(started))` carries the handle.
    fn dispatch_part(&mut self, part: Os, git_ref: &str, base: &str) -> Result<Result<Started, String>, String> {
        let after = {
            let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
            c.runs_page(1)?.runs.iter().map(|r| r.id).max().unwrap_or(0)
        };
        let os = match part {
            Os::Linux => plan_os::OsPart::Linux,
            Os::Windows => plan_os::OsPart::Windows,
        };
        let inputs = [format!("base={base}"), format!("os={}", os.input())];
        let req = dispatch::parse_request(WORKFLOW, git_ref, &inputs, "2s", Some("1s"))?;
        let root = self.root.clone();
        let read = move |name: &str| std::fs::read_to_string(root.join(".gitea/workflows").join(name)).ok();
        let mut clock = dispatch::system_clock();
        let target = dispatch::Target {
            server: &self.server,
            repo: &self.repo,
            token: &self.token,
        };
        Ok(
            match dispatch::run_with_id(&mut self.http, &target, &req, &read, &mut clock, &mut std::io::stderr())? {
                (dispatch::Dispatched::Yes { .. }, Some(id)) => Ok(Started::Run(id)),
                (dispatch::Dispatched::Yes { .. }, None) => Ok(Started::After(after)),
                (dispatch::Dispatched::StillBusy { last }, _) => Err(format!("the {} runner is busy ({last})", part.name())),
            },
        )
    }
}

/// The Linux lanes in use: jobs running, and jobs queued or waiting for their turn (they will take a lane the moment one is free,
/// ahead of anything dispatched later).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaneUse {
    /// Linux jobs in progress.
    pub running: usize,
    /// Linux jobs queued or waiting.
    pub pending: usize,
}

impl LaneUse {
    fn plus(self, o: LaneUse) -> LaneUse {
        LaneUse {
            running: self.running + o.running,
            pending: self.pending + o.pending,
        }
    }
}

/// The Linux jobs of a run's jobs that use or will use a lane: running (`in_progress`) and pending (`queued`, `waiting`).
pub fn busy_linux_jobs(jobs: &[TimedJob]) -> LaneUse {
    let mut u = LaneUse::default();
    for j in jobs
        .iter()
        .filter(|j| Os::of_job(&j.name).or_else(|| j.labels.iter().find_map(|l| Os::of_job(l))) == Some(Os::Linux))
    {
        match j.status.as_str() {
            "in_progress" => u.running += 1,
            "queued" | "waiting" => u.pending += 1,
            _ => {}
        }
    }
    u
}

/// How many Linux jobs the member's Linux part will run, read from the plan's own output: the plan job, one per project flag that is
/// true (any flag but `docs`, `all` and `conformance`'s standing one counts, as in [`reach_of`]), one per app, and the conformance
/// job; every lane when the plan says `all`. At least 1. An estimate on the high side: the gate then waits rather than jumps.
pub fn own_linux_jobs(plan: &Value) -> usize {
    let Some(obj) = plan.as_object() else {
        return usize::MAX;
    };
    if obj.get("all").and_then(Value::as_bool) == Some(true) {
        return usize::MAX;
    }
    let flags = obj
        .iter()
        .filter(|(k, v)| !matches!(k.as_str(), "docs" | "all" | "conformance") && v.as_bool() == Some(true))
        .count();
    let apps = obj.get("apps").and_then(Value::as_array).map_or(0, Vec::len);
    1 + flags + apps + usize::from(conformance_reached(plan)) + 1
}

/// Whether the member's Linux part may start beside a train that holds the runners (ER-24: a trial never jumps a running train).
/// `Err(why)` waits. It starts only when (1) a train is working right now: between two stages of its sequence nothing of it runs while
/// the next stage may start any moment, so the gate waits; (2) none of the Linux jobs of any unfinished run is queued or waiting,
/// since those take the lanes first; and (3) at least as many lanes are free as the member's own Linux jobs (`need`, capped at the
/// lanes there are).
pub fn linux_may_start(lanes: usize, in_use: &LaneUse, train_running: bool, need: usize) -> Result<(), String> {
    if !train_running {
        return Err("nothing of the train is running now (between two stages of its sequence the next stage may start any moment)".into());
    }
    if in_use.pending > 0 {
        return Err(format!(
            "{} Linux job(s) are queued or waiting and take the lanes first ({} running of {lanes})",
            in_use.pending, in_use.running
        ));
    }
    let need = need.clamp(1, lanes.max(1));
    let spare = lanes.saturating_sub(in_use.running);
    if spare < need {
        return Err(format!(
            "no room for the member's {need} Linux job(s): {} of {lanes} Linux lanes are in use",
            in_use.running
        ));
    }
    Ok(())
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

    fn plan(&mut self, base: &str, head: &str) -> Result<Value, String> {
        let plan = ask_plan(&self.root, base, head)?;
        self.linux_need = own_linux_jobs(&plan);
        Ok(plan)
    }

    fn publish(&mut self, commit: &str, git_ref: &str) -> Result<(), String> {
        let g = self.git();
        let (ok, out) = g.run_git(
            Child::Authed,
            &["push", "--quiet", "--force", "origin", &format!("{commit}:refs/heads/{git_ref}")],
        )?;
        if ok { Ok(()) } else { Err(out.trim().to_string()) }
    }

    fn try_dispatch(&mut self, part: Os, git_ref: &str, base: &str) -> Result<TryStart, String> {
        // The train always goes first: its CI sequence holds the reservation for its whole duration. The Windows part waits for it. The
        // Linux part goes on Linux lanes the train is not using (spare lanes), whether or not a train holds the runners.
        let holder = {
            let root = reservation::lock_root().ok_or("the train reservation folder cannot be derived (set PADDOCK_TRAIN_CI_LOCK_ROOT)")?;
            reservation::Held::holder(&mut reservation::LockSlots::new(root))?
        };
        let blocked: Option<String> = match (&holder, part) {
            (None, _) => None,
            (Some(h), Os::Windows) => Some(format!("a train's CI sequence holds the runners ({h})")),
            (Some(h), Os::Linux) => {
                let (lanes_in_use, train_running, lanes) = self.linux_lane_use()?;
                linux_may_start(lanes, &lanes_in_use, train_running, self.linux_need)
                    .err()
                    .map(|why| format!("a train's CI holds the runners ({h}); {why}"))
            }
        };
        let why = match blocked {
            None => match self.dispatch_part(part, git_ref, base)? {
                Ok(started) => return Ok(TryStart::Started(started)),
                Err(busy) => busy,
            },
            Some(why) => why,
        };
        // Held or busy: wait while the train's CI makes progress; end only after the stall limit with none (ER-32).
        let now = self.clock.elapsed();
        let signature = format!(
            "{}|{}",
            holder.as_deref().unwrap_or(""),
            progress_signature(&mut self.http, &self.server, &self.repo, &self.token).unwrap_or_default()
        );
        let stall = self
            .stalls
            .entry(part)
            .or_insert_with(|| reservation::Stall::new(RESERVATION_STALL, RESERVATION_HARD_MAX, now));
        Ok(match stall.observe(now, signature) {
            Err(limit) => TryStart::Stalled(format!("{why}; {limit}")),
            Ok(_) => TryStart::Wait(why),
        })
    }

    fn find_run(&mut self, _part: Os, git_ref: &str, commit: &str, started: &Started) -> Result<Option<u64>, String> {
        let after = match started {
            Started::Run(id) => return Ok(Some(*id)),
            Started::After(a) => *a,
        };
        // The run exists within seconds of the dispatch; wait for it with the shared bounded wait (ER-32).
        let mut probe = FindRun {
            http: &mut self.http,
            server: &self.server,
            repo: &self.repo,
            token: &self.token,
            after,
            git_ref,
            commit,
        };
        let mut clock = crate::wait::SystemClock::new();
        match crate::wait::wait_for(
            &mut probe,
            &mut clock,
            Duration::from_secs(180),
            Duration::from_secs(5),
            &mut std::io::stderr(),
        ) {
            crate::wait::Outcome::Done { detail, .. } => detail.parse::<u64>().map(Some).map_err(|e| format!("run id `{detail}`: {e}")),
            crate::wait::Outcome::TimedOut { .. } => Ok(None),
            crate::wait::Outcome::Failed { detail, .. } => Err(detail),
            crate::wait::Outcome::Broken { error, .. } => Err(error),
        }
    }

    fn finished(&mut self, run: u64) -> Result<bool, String> {
        let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
        c.run(run)?.map(|r| r.finished()).ok_or_else(|| format!("run {run} is not listed"))
    }

    fn conclusion(&mut self, run: u64) -> Result<String, String> {
        let mut c = Client::new(&mut self.http, &self.server, &self.repo, &self.token);
        c.run(run)?.map(|r| r.conclusion).ok_or_else(|| format!("run {run} is not listed"))
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
                labels: j.labels,
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

    fn tick(&mut self) -> Duration {
        self.clock.sleep(TICK);
        self.clock.elapsed()
    }

    fn report(&mut self, line: &str) {
        eprintln!("{line}");
    }

    fn delete(&mut self, git_ref: &str) {
        if let Err(e) = trial::delete(&self.git(), git_ref) {
            eprintln!("::warning::other-os: {git_ref} could not be deleted ({e}); run `./dev ci trial-prune`");
        }
    }
}

/// How long the train's reservation may go without progress before the gate says "not measured", and its hard limit (ER-32).
const RESERVATION_STALL: Duration = Duration::from_secs(45 * 60);
const RESERVATION_HARD_MAX: Duration = Duration::from_secs(8 * 3600);

/// What moves while a train's CI works: the ids of the unfinished runs. A change is progress (a workflow started or ended).
fn progress_signature(http: &mut dyn Http, server: &str, repo: &str, token: &str) -> Result<String, String> {
    let mut c = Client::new(http, server, repo, token);
    let mut ids: Vec<u64> = c.runs_page(1)?.runs.iter().filter(|r| !r.finished()).map(|r| r.id).collect();
    ids.sort_unstable();
    Ok(format!("{ids:?}"))
}

/// The probe of [`Runs::find_run`]: the one run newer than `after` that this dispatch made ([`pick_run`]).
struct FindRun<'a> {
    http: &'a mut dyn Http,
    server: &'a str,
    repo: &'a str,
    token: &'a str,
    after: u64,
    git_ref: &'a str,
    commit: &'a str,
}

impl crate::wait::Probe for FindRun<'_> {
    fn describe(&self) -> String {
        format!(
            "the {WORKFLOW} run this dispatch made on {} (newer than run {})",
            self.git_ref, self.after
        )
    }

    fn poll(&mut self) -> Result<crate::wait::Poll, String> {
        let mut c = Client::new(self.http, self.server, self.repo, self.token);
        let page = c.runs_page_where(1, "event=workflow_dispatch")?;
        Ok(match pick_run(&page.runs, self.after, self.git_ref, self.commit) {
            Ok(Some(id)) => crate::wait::Poll::Done(id.to_string()),
            Ok(None) => crate::wait::Poll::Pending(format!("no run newer than {} on {} yet", self.after, self.git_ref)),
            Err(e) => crate::wait::Poll::Failed(e),
        })
    }

    fn next_step(&self) -> String {
        format!(
            "read the runs of {WORKFLOW} on {} in Gitea; the dispatch may not have been accepted",
            self.git_ref
        )
    }
}

/// Asks the tool's own planner (`dev plan <base> <head>`) what the member reaches: the selection CI uses (rule 55).
fn ask_plan(root: &Path, base: &str, head: &str) -> Result<Value, String> {
    // The planner is this tool, or `PADDOCK_MERGE_CHECK_PLAN` (tests point it at a stand-in, as `PADDOCK_MERGE_CHECK_GATES` does for the
    // fast gates); its answer still decides whether the member reaches anything.
    // The seam exists in debug builds only (the tests' binary), never in a release build of the tool.
    #[cfg(debug_assertions)]
    let seam = std::env::var_os("PADDOCK_MERGE_CHECK_PLAN").map(PathBuf::from);
    #[cfg(not(debug_assertions))]
    let seam: Option<PathBuf> = None;
    let program = match seam {
        Some(p) => p,
        None => Gitea::me()?,
    };
    let mut cmd = command(program);
    cmd.args(["plan", base, head, "--explain"]).current_dir(root);
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
    Ok(plan)
}

/// The real step for `merge check --gates`: a missing token or trial folder is `not measured` (the exit 6 when enforced), with what
/// to set. The plan decides first whether the member reaches anything, so a documentation-only member needs no token and is never
/// dispatched. A member whose fast gates already failed (`gates_failed`) is not dispatched either: the row is `not measured`.
pub fn run_real(root: &Path, _git: &Git, merge: &Merge, enforced: bool, gates_failed: bool) -> Step {
    let Some(commit) = merge.commit.as_deref() else {
        return not_run("the merge has no commit");
    };
    let docs_only = || match merge.parents.first().map(|b| ask_plan(root, b, commit)) {
        Some(Ok(plan)) => reach_of(&plan) == Reach::Nothing,
        _ => false,
    };
    if gates_failed {
        return if docs_only() {
            not_run("the plan reaches no project for this member (documentation only)")
        } else {
            not_dispatched_after_failed_gates(enforced)
        };
    }
    match Gitea::open(root, &merge.label, commit) {
        Err(_) if docs_only() => not_run("the plan reaches no project for this member (documentation only)"),
        Err(why) => unmeasured(enforced, &why),
        Ok(mut g) => run(merge, &mut g, enforced),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    // The gate is enforced since 2026-11-21 (user): a train merge it could not measure must not go on. Un-enforcing it is a decision of
    // the user, so this fails the build until that is decided.
    const _: () = assert!(ENFORCED);

    fn job(name: &str, conclusion: &str) -> JobRow {
        JobRow {
            name: name.into(),
            conclusion: conclusion.into(),
            phases: None,
            labels: Vec::new(),
        }
    }

    fn job_on(name: &str, conclusion: &str, label: &str) -> JobRow {
        JobRow {
            labels: vec![label.into()],
            ..job(name, conclusion)
        }
    }

    fn fail_in(job: &str, test: &str, blame: Blame, projects: &[&str]) -> Failure {
        Failure {
            what: format!("{job}: {test}"),
            projects: projects.iter().map(|p| p.to_string()).collect::<BTreeSet<_>>(),
            files: Vec::new(),
            blame,
            retried: false,
        }
    }

    fn fail(job: &str, test: &str, blame: Blame) -> Failure {
        fail_in(job, test, blame, &[])
    }

    /// A test that failed, then passed on its one retry (FLAKY).
    fn flaky(job: &str, test: &str, projects: &[&str]) -> Failure {
        Failure {
            retried: true,
            ..fail_in(job, test, Blame::Flaky, projects)
        }
    }

    fn row(rows: &[Row], os: Os) -> &Row {
        rows.iter().find(|r| r.os == os).unwrap()
    }

    /// The plan of a member that changes the paddock-dev crate: what `dev plan --explain` prints for it.
    fn plan_paddock_dev() -> Value {
        json!({
            "all": false, "crates": false, "ui": false, "tools": false, "paddockDev": true, "conformance": true, "docs": false,
            "apps": [], "packages": {"tools/paddock-dev": ["paddock-dev"]},
            "why": {"conformance": ["conformance always runs"], "paddock-dev": ["tools/paddock-dev/src/x.rs"]}
        })
    }

    fn ctx<'a>(changed: &'a [String], plan: &'a Value) -> Ctx<'a> {
        Ctx {
            changed,
            plan,
            run_conclusion: "success",
        }
    }

    fn rows_ok(jobs: &[JobRow], failures: &[Failure]) -> Vec<Row> {
        let plan = plan_paddock_dev();
        rows(7, "abc", jobs, failures, &ctx(&[], &plan))
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn jobs_belong_to_the_os_their_matrix_or_their_runner_label_names() {
        assert_eq!(Os::of_job("paddock-dev (windows-latest)"), Some(Os::Windows));
        assert_eq!(Os::of_job("crates (ubuntu-latest)"), Some(Os::Linux));
        assert_eq!(Os::of_job("app dyno (macos-latest)"), None);
        assert_eq!(Os::of_job("plan (affected projects)"), None);
        // A job whose name names no OS (conformance, tools, ui) is on its runner label's OS.
        assert_eq!(job_on("conformance", "success", "ubuntu-latest").os(), Some(Os::Linux));
        assert_eq!(job_on("ui", "success", "macos-latest").os(), None);
        assert_eq!(job("ui", "success").os(), None);
    }

    /// The plan decides, from its own output, whether a member reaches anything: no hand list of its project keys (ER-22, ER-42).
    #[test]
    fn the_plan_decides_what_a_member_reaches_from_its_own_output() {
        assert_eq!(reach_of(&plan_paddock_dev()), Reach::Projects);
        assert_eq!(reach_of(&json!({"all": true})), Reach::Projects);
        assert_eq!(reach_of(&json!({"apps": ["dyno"], "crates": false})), Reach::Projects);
        // A key the planner may gain tomorrow counts without a change here.
        assert_eq!(reach_of(&json!({"somethingNew": true, "docs": true})), Reach::Projects);
        // Documentation only: conformance is in every plan for its one standing reason, and docs is not a project.
        let docs_only = json!({"docs": true, "conformance": true, "crates": false, "apps": [], "why": {"conformance": [crate::plan::CONFORMANCE_ALWAYS]}});
        assert_eq!(reach_of(&docs_only), Reach::Nothing);
        // Conformance reached for a reason of its own, or a plan that does not say why: the gate runs.
        let own = json!({"docs": false, "conformance": true, "apps": [], "why": {"conformance": [crate::plan::CONFORMANCE_ALWAYS, "conformance/x.mjs"]}});
        assert_eq!(reach_of(&own), Reach::Projects);
        assert_eq!(reach_of(&json!({"conformance": true})), Reach::Projects);
        assert_eq!(reach_of(&json!("not an object")), Reach::Projects);
        assert_eq!(reach_of(&json!({"docs": true, "crates": false, "apps": []})), Reach::Nothing);
    }

    #[test]
    fn a_green_run_is_clean_with_evidence_and_an_os_without_jobs_is_not_applicable() {
        let r = rows_ok(&[job("plan", "success"), job("crates (ubuntu-latest)", "success")], &[]);
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::Clean(_)));
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::NotApplicable(_)));
    }

    /// ER-12 (user, 2026-11-20): a test that fails both tries blocks even when main fails it too. The label stays; it is a finding.
    #[test]
    fn a_test_that_fails_both_tries_blocks_even_when_it_also_fails_on_main() {
        let jobs = [
            job("plan", "success"),
            job("paddock-dev (windows-latest)", "failure"),
            job("crates (ubuntu-latest)", "failure"),
        ];
        let fails = [
            fail("paddock-dev (windows-latest)", "places::a", Blame::OnMain),
            fail("paddock-dev (windows-latest)", "wrapper::b", Blame::OnMain),
            fail("crates (ubuntu-latest)", "proc::c", Blame::New),
        ];
        let r = rows_ok(&jobs, &fails);
        let win = row(&r, Os::Windows);
        match &win.outcome {
            Outcome::Findings(f) => {
                assert_eq!(f.findings().as_slice().len(), 2);
                assert!(
                    f.findings().as_slice()[0].message.contains("also failing on main"),
                    "{:?}",
                    f.findings()
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(win.known.len(), 2, "the label stays");
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::Findings(_)));
    }

    /// P1: a run that never reached a test says nothing. The plan job must have succeeded, the run must not be cancelled, and a row
    /// whose jobs were all skipped is `not applicable` only after that.
    #[test]
    fn a_run_whose_plan_did_not_succeed_or_was_cancelled_is_not_measured_never_clean() {
        // The plan job failed: every other job was skipped, which is not "nothing to run".
        for plan_state in ["failure", "cancelled", ""] {
            let jobs = [
                job("plan", plan_state),
                job("crates (ubuntu-latest)", "skipped"),
                job("crates (windows-latest)", "skipped"),
            ];
            for r in rows_ok(&jobs, &[]) {
                assert!(matches!(r.outcome, Outcome::NotMeasured(_)), "{plan_state}: {:?}", r.outcome);
            }
        }
        // No plan job at all.
        for r in rows_ok(&[job("crates (ubuntu-latest)", "success")], &[]) {
            assert!(matches!(r.outcome, Outcome::NotMeasured(_)), "{:?}", r.outcome);
        }
        // A cancelled or unfinished run, plan green.
        let plan = plan_paddock_dev();
        for concl in ["cancelled", ""] {
            let c = Ctx {
                changed: &[],
                plan: &plan,
                run_conclusion: concl,
            };
            let r = rows(7, "a", &[job("plan", "success"), job("crates (ubuntu-latest)", "success")], &[], &c);
            assert!(matches!(row(&r, Os::Linux).outcome, Outcome::NotMeasured(_)), "{concl}");
        }
        // Plan green, every job of an OS skipped: not applicable, never clean.
        let r = rows_ok(&[job("plan", "success"), job("crates (ubuntu-latest)", "skipped")], &[]);
        assert!(
            matches!(row(&r, Os::Linux).outcome, Outcome::NotApplicable(_)),
            "{:?}",
            row(&r, Os::Linux).outcome
        );
    }

    #[test]
    fn what_cannot_be_blamed_is_not_measured_never_a_pass() {
        // Unknown blame, a failed job with no test read, a cancelled job, an unfinished job.
        let r = rows_ok(
            &[job("plan", "success"), job("x (windows-latest)", "failure")],
            &[fail("x (windows-latest)", "t", Blame::Unknown)],
        );
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::NotMeasured(_)));
        let r = rows_ok(&[job("plan", "success"), job("x (windows-latest)", "failure")], &[]);
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::NotMeasured(_)));
        let r = rows_ok(&[job("plan", "success"), job("x (ubuntu-latest)", "cancelled")], &[]);
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::NotMeasured(_)));
        let r = rows_ok(&[job("plan", "success"), job("x (ubuntu-latest)", "")], &[]);
        assert!(matches!(row(&r, Os::Linux).outcome, Outcome::NotMeasured(_)));
    }

    /// P2: Linux jobs that name no OS (conformance, tools, ui) are judged by their runner label; other machines' jobs are listed as
    /// not judged, and a row never says "clean" over jobs it ignored.
    #[test]
    fn jobs_without_an_os_in_their_name_are_judged_by_their_label_and_the_rest_are_listed() {
        let jobs = [
            job("plan", "success"),
            job_on("conformance", "failure", "ubuntu-latest"),
            job_on("ui", "success", "ubuntu-latest"),
            job_on("app dyno (macos-latest)", "success", "macos-latest"),
        ];
        let fails = [fail("conformance", "C12: something", Blame::New)];
        let r = rows_ok(&jobs, &fails);
        match &row(&r, Os::Linux).outcome {
            Outcome::Findings(f) => assert_eq!(f.findings().as_slice().len(), 1),
            other => panic!("{other:?}"),
        }
        assert_eq!(row(&r, Os::Linux).jobs, vec!["conformance", "ui"]);
        assert_eq!(not_judged(&jobs), vec!["app dyno (macos-latest)"]);
        let s = step_of(&r, &not_judged(&jobs), &[(Os::Linux, Some(7))], "trial/x", "1111111", true);
        assert!(
            s.text.join("\n").contains("not judged") && s.text.join("\n").contains("app dyno (macos-latest)"),
            "{:?}",
            s.text
        );
        assert_eq!(s.data["notJudged"][0], "app dyno (macos-latest)");
    }

    // ---- The handle of the run (ER-51) ----

    fn run_info(id: u64, event: &str, branch: &str, sha: &str) -> RunInfo {
        RunInfo {
            id,
            path: format!("ci.yml@refs/heads/{branch}"),
            event: event.into(),
            status: "completed".into(),
            conclusion: "success".into(),
            head_sha: sha.into(),
            head_branch: branch.into(),
            created_at: String::new(),
            run_attempt: 1,
        }
    }

    /// P1: only the run this dispatch made is judged. A run already on the commit (a train's, a rerun, an earlier trial of the same
    /// merge) is never taken for it, whatever its result.
    #[test]
    fn only_a_run_newer_than_the_dispatch_with_its_own_event_and_ref_is_taken() {
        let r = "trial/member-abcdef012345";
        let sha = "abc";
        // Stale: an older run on the same commit and ref.
        assert_eq!(pick_run(&[run_info(90, "workflow_dispatch", r, sha)], 100, r, sha), Ok(None));
        // A newer run of another event (a push), another ref (the train), another commit.
        assert_eq!(pick_run(&[run_info(101, "push", r, sha)], 100, r, sha), Ok(None));
        assert_eq!(
            pick_run(&[run_info(101, "workflow_dispatch", "train/x", sha)], 100, r, sha),
            Ok(None)
        );
        assert_eq!(pick_run(&[run_info(101, "workflow_dispatch", r, "other")], 100, r, sha), Ok(None));
        // Ours, among stale ones.
        let all = [
            run_info(90, "workflow_dispatch", r, sha),
            run_info(101, "workflow_dispatch", r, sha),
        ];
        assert_eq!(pick_run(&all, 100, r, sha), Ok(Some(101)));
        // Two that look like ours: nothing proves which, so none.
        let two = [
            run_info(101, "workflow_dispatch", r, sha),
            run_info(102, "workflow_dispatch", r, sha),
        ];
        assert!(pick_run(&two, 100, r, sha).unwrap_err().contains("none is proven"));
    }

    // ---- The feature of a test ----

    #[test]
    fn a_rust_tests_feature_is_the_files_of_its_own_crate_named_like_its_modules() {
        let plan = plan_paddock_dev();
        let f = |test: &str| fail_in("paddock-dev (windows-latest)", test, Blame::Flaky, &["paddock-dev"]);
        let t = f("paddock-dev sccache_eviction::tests::evicts");
        let touched = |files: &[&str], t: &Failure| feature_touched(t, &ctx(&strs(files), &plan));
        assert!(touched(&["tools/paddock-dev/tests/it/sccache_eviction.rs"], &t));
        assert!(touched(&["tools/paddock-dev/src/sccache_eviction/mod.rs"], &t));
        // Not the same crate: a file of another folder with the same name is not its feature.
        assert!(!touched(&["crates/other/src/sccache_eviction.rs"], &t));
        // Another feature of the same crate, and docs, are not.
        assert!(!touched(&["tools/paddock-dev/src/plan.rs", "docs/rules.md"], &t));
        // Unknown module: no `::`, or only `tests`: blocks.
        assert!(touched(&[], &f("some spec name")));
        assert!(touched(&[], &f("tests::x")));
        assert!(touched(&[], &f("the job failed outside any test")));
    }

    /// The plan's own graph first: a test whose project the plan does not reach is untouched, whatever the files are called.
    #[test]
    fn a_test_in_a_project_the_plan_does_not_reach_is_untouched() {
        let plan = plan_paddock_dev();
        let t = fail_in(
            "crates (ubuntu-latest)",
            "paddock-ipc endings::drain",
            Blame::Flaky,
            &["paddock-ipc"],
        );
        assert!(!feature_touched(&t, &ctx(&strs(&["crates/paddock-ipc/src/endings.rs"]), &plan)));
        // The same project when the plan reaches it (through its packages).
        let mut p2 = plan.clone();
        p2["packages"] = json!({"crates": ["paddock-ipc"]});
        assert!(feature_touched(&t, &ctx(&strs(&["crates/paddock-ipc/src/endings.rs"]), &p2)));
        // `all` reaches everything.
        let all = json!({"all": true});
        assert!(feature_touched(&t, &ctx(&strs(&["crates/paddock-ipc/src/endings.rs"]), &all)));
    }

    /// User 2026-11-19: UI flakes in features the member does not touch do not block. Playwright and Vitest names carry the spec file.
    #[test]
    fn a_playwright_or_vitest_test_maps_to_its_spec_files_feature() {
        let plan = json!({"ui": true, "apps": ["debrief"], "conformance": true});
        let pw = fail_in(
            "ui e2e (ubuntu-latest)",
            "[webkit] \u{203a} e2e/home-dashboard.spec.ts:12:5 \u{203a} Home \u{203a} refreshes",
            Blame::Flaky,
            &["ui"],
        );
        let vt = fail_in("ui (ubuntu-latest)", "ui/src/lib/explorer-grid.spec.ts", Blame::Flaky, &["ui"]);
        let touched = |files: &[&str], t: &Failure| feature_touched(t, &ctx(&strs(files), &plan));
        // The spec itself, or the component it is named for, is the feature.
        assert!(touched(&["ui/e2e/home-dashboard.spec.ts"], &pw));
        assert!(touched(&["ui/src/app/home-dashboard/home-dashboard.component.ts"], &pw));
        assert!(touched(&["ui/src/lib/explorer-grid.component.ts"], &vt));
        // Other features of the same UI project do not block.
        assert!(!touched(
            &["ui/src/lib/explorer-grid.component.ts", "ui/src/app/filter-bar/filter-bar.ts"],
            &pw
        ));
        assert!(!touched(&["ui/src/app/home-dashboard/home-dashboard.component.ts"], &vt));
    }

    /// ER-12: a FLAKY test blocks only when the member touches its feature; a test that failed both tries always blocks.
    #[test]
    fn a_flaky_test_blocks_only_when_the_member_touches_its_feature_and_a_both_tries_failure_always() {
        let plan = plan_paddock_dev();
        let jobs = [job("plan", "success"), job("paddock-dev (windows-latest)", "failure")];
        let fl = flaky(
            "paddock-dev (windows-latest)",
            "paddock-dev home_dashboard::refreshes",
            &["paddock-dev"],
        );
        // Untouched feature: passes, recorded.
        let ch = strs(&["tools/paddock-dev/src/sccache_eviction.rs"]);
        let r = rows(7, "a", &jobs, std::slice::from_ref(&fl), &ctx(&ch, &plan));
        assert!(
            matches!(row(&r, Os::Windows).outcome, Outcome::Clean(_)),
            "{:?}",
            row(&r, Os::Windows).outcome
        );
        assert_eq!(row(&r, Os::Windows).flaky.len(), 1);
        // Touched feature: blocks.
        let ch = strs(&["tools/paddock-dev/src/home_dashboard/mod.rs"]);
        let r = rows(7, "a", &jobs, std::slice::from_ref(&fl), &ctx(&ch, &plan));
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::Findings(_)));
        // Flaky and also failing on main: still the same rule (not blamed on main, not blocked either, when untouched).
        let on_main = Failure {
            blame: Blame::OnMain,
            ..fl.clone()
        };
        let ch = strs(&["docs/rules.md"]);
        let r = rows(7, "a", &jobs, &[on_main], &ctx(&ch, &plan));
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::Clean(_)));
        // Failed both tries (not retried), untouched feature, even a test of an untouched project: blocks.
        let both = fail_in(
            "paddock-dev (windows-latest)",
            "paddock-dev home_dashboard::refreshes",
            Blame::New,
            &["paddock-dev"],
        );
        let r = rows(7, "a", &jobs, &[both], &ctx(&ch, &plan));
        assert!(matches!(row(&r, Os::Windows).outcome, Outcome::Findings(_)));
    }

    /// A table CI: records what was asked, answers from fields. Time is a counter: `tick` is one step, and a run finishes at the tick its
    /// table names.
    struct Fake {
        plan: Result<Value, String>,
        /// The answers of `try_dispatch` per part, in order; the last one repeats.
        linux: Vec<TryStart>,
        windows: Vec<TryStart>,
        /// What `find_run` answers per part when the server named no run.
        found: BTreeMap<Os, Option<u64>>,
        /// The tick at which each run is over.
        ends: BTreeMap<u64, u64>,
        conclusion: BTreeMap<u64, String>,
        jobs: BTreeMap<u64, Vec<JobRow>>,
        failures: BTreeMap<u64, Vec<Failure>>,
        changed: Vec<String>,
        ticks: u64,
        asked: Vec<String>,
    }

    impl Fake {
        /// The Linux part starts at once (run 101, over at tick 1); the Windows part waits two ticks for the train, then runs
        /// (run 102, over at tick 4). Both are green.
        fn green() -> Fake {
            let ok = |os_job: &str| vec![job("plan", "success"), job(os_job, "success")];
            Fake {
                plan: Ok(plan_paddock_dev()),
                linux: vec![TryStart::Started(Started::After(100))],
                windows: vec![
                    TryStart::Wait("a train's CI sequence holds the runners".into()),
                    TryStart::Wait("a train's CI sequence holds the runners".into()),
                    TryStart::Started(Started::After(100)),
                ],
                found: [(Os::Linux, Some(101)), (Os::Windows, Some(102))].into(),
                ends: [(101, 1), (102, 4)].into(),
                conclusion: BTreeMap::new(),
                jobs: [(101, ok("crates (ubuntu-latest)")), (102, ok("crates (windows-latest)"))].into(),
                failures: BTreeMap::new(),
                changed: strs(&["tools/paddock-dev/src/x.rs"]),
                ticks: 0,
                asked: Vec::new(),
            }
        }

        fn answer(list: &mut Vec<TryStart>) -> TryStart {
            if list.len() > 1 { list.remove(0) } else { list[0].clone() }
        }
    }

    impl Runs for Fake {
        fn plan(&mut self, base: &str, head: &str) -> Result<Value, String> {
            self.asked.push(format!("plan {base}..{head}"));
            self.plan.clone()
        }
        fn changed_files(&mut self, _base: &str, _head: &str) -> Result<Vec<String>, String> {
            Ok(self.changed.clone())
        }
        fn publish(&mut self, commit: &str, git_ref: &str) -> Result<(), String> {
            self.asked.push(format!("publish {commit} {git_ref}"));
            Ok(())
        }
        fn try_dispatch(&mut self, part: Os, git_ref: &str, base: &str) -> Result<TryStart, String> {
            let t = match part {
                Os::Linux => Fake::answer(&mut self.linux),
                Os::Windows => Fake::answer(&mut self.windows),
            };
            if let TryStart::Started(_) = t {
                self.asked.push(format!("dispatch {} {git_ref} base={base}", part.name()));
            }
            Ok(t)
        }
        fn find_run(&mut self, part: Os, _git_ref: &str, _commit: &str, started: &Started) -> Result<Option<u64>, String> {
            Ok(match started {
                Started::Run(id) => Some(*id),
                Started::After(_) => self.found.get(&part).copied().flatten(),
            })
        }
        fn finished(&mut self, run: u64) -> Result<bool, String> {
            let done = self.ticks >= self.ends.get(&run).copied().unwrap_or(0);
            self.asked.push(format!("finished {run} {done}"));
            Ok(done)
        }
        fn conclusion(&mut self, run: u64) -> Result<String, String> {
            Ok(self.conclusion.get(&run).cloned().unwrap_or_else(|| "success".into()))
        }
        fn jobs(&mut self, run: u64) -> Result<Vec<JobRow>, String> {
            self.asked.push(format!("jobs {run}"));
            Ok(self.jobs.get(&run).cloned().unwrap_or_default())
        }
        fn failures(&mut self, run: u64) -> Result<Vec<Failure>, String> {
            Ok(self.failures.get(&run).cloned().unwrap_or_default())
        }
        fn tick(&mut self) -> Duration {
            self.ticks += 1;
            Duration::from_secs(15 * self.ticks)
        }
        fn report(&mut self, line: &str) {
            self.asked.push(format!("report {line}"));
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

    fn at(asked: &[String], prefix: &str) -> usize {
        asked
            .iter()
            .position(|a| a.starts_with(prefix))
            .unwrap_or_else(|| panic!("`{prefix}` is not in {asked:?}"))
    }

    #[test]
    fn a_member_the_plan_does_not_reach_is_not_run_and_nothing_is_pushed() {
        let mut f = Fake::green();
        f.plan = Ok(
            json!({"docs": true, "conformance": true, "crates": false, "apps": [], "why": {"conformance": [crate::plan::CONFORMANCE_ALWAYS]}}),
        );
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["outcome"], "not run");
        assert_eq!(f.asked.len(), 1, "{:?}", f.asked);
    }

    /// The user's rule (2026-11-21): while a train holds the Windows runner, the Linux part runs and is judged at once, Windows waits
    /// and is judged after, and the member is clean only after both. The Linux verdict is said the moment it is in, before the
    /// Windows part is even dispatched.
    #[test]
    fn linux_runs_and_is_judged_while_windows_waits_and_the_member_is_clean_only_after_both() {
        let mut f = Fake::green();
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["outcome"], "passed", "{:?}", s.text);
        assert_eq!(s.exit, Exit::Ok);
        let a = &f.asked;
        // Order: Linux dispatched first, its verdict reported (member pending) before Windows is dispatched, Windows judged last.
        let (l_dispatch, l_verdict) = (at(a, "dispatch linux"), at(a, "report other-os: linux clean"));
        let (w_dispatch, w_jobs) = (at(a, "dispatch windows"), at(a, "jobs 102"));
        assert!(l_dispatch < l_verdict && l_verdict < w_dispatch && w_dispatch < w_jobs, "{a:?}");
        assert!(a[l_verdict].contains("stays pending"), "{}", a[l_verdict]);
        assert!(
            !a[at(a, "report other-os: windows")].contains("stays pending"),
            "windows is the last part in: {a:?}"
        );
        // Each part was judged from its own run.
        assert_eq!(s.data["runs"], json!({"linux": 101, "windows": 102}));
        assert_eq!(s.data["rows"][0]["run"], 101);
        assert_eq!(s.data["rows"][1]["run"], 102);
        let sref = "trial/member-abcdef012345";
        assert_eq!(a.last().unwrap(), &format!("delete {sref}"));
    }

    /// Positive control (ER-33): Linux done alone never passes. When the train never gives the Windows runner back (nothing progresses
    /// for the stall limit), the Linux row is reported clean but the member is `not measured`, exit 6; and a Windows failure after a
    /// clean Linux fails the member.
    #[test]
    fn linux_done_alone_never_passes() {
        let mut f = Fake::green();
        f.windows = vec![
            TryStart::Wait("a train's CI sequence holds the runners".into()),
            TryStart::Stalled("a train's CI sequence holds the runners; nothing progressed for 45m".into()),
        ];
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.data["rows"][0]["outcome"]["status"], "clean", "Linux itself was judged");
        assert_eq!(s.data["rows"][1]["outcome"]["status"], "not-measured", "{:?}", s.data["rows"][1]);
        assert_eq!(s.data["outcome"], "not measured");
        assert_eq!(s.exit, Exit::NotMeasured);
        assert!(!f.asked.iter().any(|a| a.starts_with("dispatch windows")), "{:?}", f.asked);
        // Windows judged and red after a clean Linux: the member fails.
        let mut f = Fake::green();
        f.jobs
            .insert(102, vec![job("plan", "success"), job("crates (windows-latest)", "failure")]);
        f.failures
            .insert(102, vec![fail("crates (windows-latest)", "sccache_eviction::x", Blame::New)]);
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.data["rows"][0]["outcome"]["status"], "clean");
        assert_eq!(s.data["rows"][1]["outcome"]["status"], "findings");
        assert_eq!(s.exit, Exit::Findings);
    }

    /// No Linux lane is spare while the train works: the Linux part waits too (it never takes a lane the train uses), then goes.
    #[test]
    fn the_linux_part_waits_for_a_spare_lane_and_then_goes() {
        let mut f = Fake::green();
        f.linux = vec![
            TryStart::Wait("a train's CI holds the runners and no Linux lane is spare (3 of 3 in use)".into()),
            TryStart::Started(Started::After(100)),
        ];
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["outcome"], "passed", "{:?}", s.text);
        assert!(
            f.asked
                .iter()
                .any(|a| a.contains("the linux part waits") && a.contains("no Linux lane is spare")),
            "{:?}",
            f.asked
        );
    }

    /// ER-51: when the server names the run it created, that id and no other is judged for the part.
    #[test]
    fn the_run_the_dispatch_named_is_the_only_one_judged() {
        let mut f = Fake::green();
        f.linux = vec![TryStart::Started(Started::Run(777))];
        f.found.insert(Os::Linux, Some(101)); // another run on the commit: never asked about
        f.ends.insert(777, 1);
        f.jobs
            .insert(777, vec![job("plan", "success"), job("crates (ubuntu-latest)", "success")]);
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["runs"]["linux"], 777);
        assert!(
            f.asked.contains(&"jobs 777".to_string()) && !f.asked.contains(&"jobs 101".to_string()),
            "{:?}",
            f.asked
        );
    }

    /// Positive control (ER-33): a run this step did not make cannot pass. With no run of ours for a part (only stale runs on the
    /// commit, which `pick_run` refuses), that part is `not measured` with the exit 6 and nothing of it is judged.
    #[test]
    fn without_a_run_of_its_own_the_part_cannot_pass() {
        let mut f = Fake::green();
        f.found.insert(Os::Linux, None);
        let s = run(&merge(), &mut f, ENFORCED);
        assert_eq!(s.data["rows"][0]["outcome"]["status"], "not-measured");
        assert_eq!(s.exit, Exit::NotMeasured);
        assert!(
            !f.asked.iter().any(|a| a == "jobs 101"),
            "nothing of an unmade run was judged: {:?}",
            f.asked
        );
        assert_ne!(s.data["outcome"], "passed");
    }

    /// A planted new failure on Windows is red there and green on Linux; a still-running run is not measured; a cancelled or failed
    /// plan is not measured; and the exit changes only when enforced.
    #[test]
    fn a_new_failure_on_one_os_is_red_there_and_the_exit_changes_only_when_enforced() {
        for (enforced, want) in [(false, Exit::Ok), (true, Exit::Findings)] {
            let mut f = Fake::green();
            f.jobs
                .insert(102, vec![job("plan", "success"), job("crates (windows-latest)", "failure")]);
            f.failures
                .insert(102, vec![fail("crates (windows-latest)", "sh_fixture", Blame::New)]);
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
        // A run still going at the bound: not measured (its part).
        let mut f = Fake::green();
        f.ends.insert(102, u64::MAX);
        assert_eq!(run(&merge(), &mut f, true).exit, Exit::NotMeasured);
        let mut f = Fake::green();
        f.plan = Err("no plan".into());
        assert_eq!(run(&merge(), &mut f, true).data["outcome"], "not measured");
        let mut f = Fake::green();
        f.conclusion.insert(101, "cancelled".into());
        let s = run(&merge(), &mut f, true);
        assert_eq!(s.data["rows"][0]["outcome"]["status"], "not-measured");
    }

    #[test]
    fn a_busy_runner_and_a_missing_token_or_folder_are_not_measured_with_exit_6() {
        let mut f = Fake::green();
        f.windows = vec![TryStart::Stalled("the windows runner is busy; nothing progressed for 45m".into())];
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

    fn tj(name: &str, status: &str, labels: &[&str]) -> TimedJob {
        TimedJob {
            id: 1,
            name: name.into(),
            runner_id: 1,
            runner: None,
            conclusion: String::new(),
            start: None,
            end: None,
            steps: Vec::new(),
            status: status.into(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
        }
    }

    /// A lane is in use by a Linux job that runs, and also by one that is queued or waiting (it takes the lane first); a Windows or Mac
    /// job, or a finished one, uses none.
    #[test]
    fn lanes_in_use_are_the_linux_jobs_running_queued_or_waiting() {
        let jobs = [
            tj("crate (ubuntu-latest)", "in_progress", &[]),
            tj("conformance", "in_progress", &["ubuntu-latest"]),
            tj("ui", "queued", &["ubuntu-latest"]),
            tj("tools", "waiting", &["ubuntu-latest"]),
            tj("paddock-dev (windows-latest)", "in_progress", &[]),
            tj("tools-macos", "queued", &["macos-latest"]),
            tj("gitleaks", "completed", &["ubuntu-latest"]),
        ];
        assert_eq!(busy_linux_jobs(&jobs), LaneUse { running: 2, pending: 2 });
    }

    /// ER-24 (P1): a member trial never jumps a running train. Between two stages (nothing in progress, the next stage queued) it does not
    /// dispatch; nor while any Linux job is queued or waiting, nor without room for its own jobs; it goes only beside a working train
    /// with the lanes free for its own jobs.
    #[test]
    fn the_linux_part_never_jumps_a_running_train() {
        let lanes = 6;
        let idle = LaneUse::default();
        // Between two stages: nothing running, the next stage queued.
        let gap = LaneUse { running: 0, pending: 3 };
        let e = linux_may_start(lanes, &gap, false, 3).unwrap_err();
        assert!(e.contains("between two stages"), "{e}");
        // The gap with nothing queued yet is the same.
        assert!(linux_may_start(lanes, &idle, false, 1).unwrap_err().contains("between two stages"));
        // The train works but a Linux job of it is queued or waiting.
        let e = linux_may_start(lanes, &LaneUse { running: 2, pending: 1 }, true, 1).unwrap_err();
        assert!(e.contains("queued or waiting"), "{e}");
        // No room for the member's own jobs: 4 of 6 lanes in use, the member needs 3.
        let e = linux_may_start(lanes, &LaneUse { running: 4, pending: 0 }, true, 3).unwrap_err();
        assert!(e.contains("no room for the member's 3"), "{e}");
        // Room: 3 of 6 in use, the member needs 3; and a member bigger than the host is capped at the lanes (all must be free).
        assert_eq!(linux_may_start(lanes, &LaneUse { running: 3, pending: 0 }, true, 3), Ok(()));
        assert!(linux_may_start(lanes, &LaneUse { running: 1, pending: 0 }, true, usize::MAX).is_err());
        assert_eq!(linux_may_start(lanes, &idle, true, usize::MAX), Ok(()));
    }

    /// The member's own Linux job count comes from the plan's own output.
    #[test]
    fn the_members_own_linux_jobs_are_read_from_the_plan() {
        assert_eq!(own_linux_jobs(&json!({"all": true})), usize::MAX);
        assert_eq!(own_linux_jobs(&json!("x")), usize::MAX);
        // paddock-dev only: the plan job, its flag, and conformance's own job (plus one for the matrix leg's setup margin).
        let n = own_linux_jobs(&plan_paddock_dev());
        assert!((3..=4).contains(&n), "{n}");
        // More projects reached, more jobs.
        let bigger = json!({"crates": true, "ui": true, "tools": true, "apps": ["a", "b"], "conformance": true, "why": {"conformance": ["conformance always runs"]}});
        assert!(own_linux_jobs(&bigger) > n, "{} {n}", own_linux_jobs(&bigger));
    }

    /// The Mac is said out loud, never silent: a row in the text and the data (it is the dev OS and not judged by the trial).
    #[test]
    fn the_mac_is_said_out_loud_as_not_judged() {
        let r = rows_ok(&[job("plan", "success"), job("crates (ubuntu-latest)", "success")], &[]);
        let s = step_of(&r, &[], &[(Os::Linux, Some(1))], "trial/x", "1111111", true);
        let text = s.text.join("\n");
        assert!(
            text.contains("macos: not judged (dev OS; covered by the local pre-push run, not a CI-like clone)"),
            "{text}"
        );
        assert_eq!(s.data["macos"]["outcome"], "not judged");
        assert!(s.data["macos"]["why"].as_str().unwrap().contains("local pre-push run"));
        // It is a statement, not a pass: it changes neither the outcome word nor the exit.
        assert_eq!(s.data["outcome"], "passed");
    }

    /// P3: a member whose fast gates failed is not dispatched; the row is not measured (never a pass).
    #[test]
    fn a_member_whose_fast_gates_failed_is_not_dispatched_and_is_not_measured() {
        let s = not_dispatched_after_failed_gates(ENFORCED);
        assert_eq!(s.exit, Exit::NotMeasured);
        assert_eq!(s.data["outcome"], "not measured");
        assert!(s.text.join("\n").contains("fast gates already failed"), "{:?}", s.text);
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
        let r = rows_ok(&[job("plan", "success"), j, w], &[]);
        let s = step_of(
            &r,
            &[],
            &[(Os::Linux, Some(77)), (Os::Windows, Some(78))],
            "trial/member-x",
            "1111111",
            true,
        );
        let text = s.text.join("\n");
        assert!(text.contains("linux run 77, windows run 78 on trial/member-x"), "{text}");
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
