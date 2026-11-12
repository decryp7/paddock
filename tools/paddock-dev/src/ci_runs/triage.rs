// ci-runs-reader: ci triage: names the failed tests of one run and compares with main's last run
//! `ci triage <run>`: for each test that failed in a CI run, is the failure NEW, ALREADY FAILING ON MAIN, or FLAKY?
//!
//! It reads, and never writes: the failed jobs of the run and their logs (through [`logfind`], the reader `ci flakes` uses, which
//! sits on the nextest, Playwright and vitest readers of `plan --since-run`), the last finished run of the same workflow on
//! `refs/heads/main` (or the run named with `--against`), and the flake history of `ci flakes`. It never quarantines a test and
//! never turns a red run green (ER-12): it only says what the evidence is.
//!
//! The order of the answer for one failed test:
//!
//! 1. **FLAKY (n of m runs)**: `ci flakes` lists it as flaky or as failed-then-passed in the window. If it also failed on the baseline
//!    run the row says so.
//! 2. **ALREADY FAILING ON MAIN**: the job of the same name failed in the baseline run and its log names this test (or, for a job
//!    that failed outside any test runner, the job failed there too).
//! 3. **NEW**: the job of the same name passed in the baseline run, or failed there without this test.
//! 4. **UNKNOWN**: nothing proves either way (no baseline run, the job did not run or its log cannot be read, a log the reader
//!    cannot account for). Never "new" and never "old" (ER-33).
//!
//! `ci flakes` reads one app at a time, so the history covers the app of the run's workflow and of its `app <name>` jobs. Failures
//! of shared jobs (`crate`, `paddock-dev`) have no flake history; the report says so.
//!
//! The classification ([`status_of`]) is pure; [`collect`] reads the server.

use super::client::{Client, RunInfo, TimedJob};
use super::flakes::{self, Kind};
use super::logfind;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The most pages of the run list read to find the last main run (50 runs a page).
const BASELINE_PAGES: u32 = 10;
/// Rows printed in the text report; `--json` has all.
const TEXT_ROWS: usize = 200;

/// What one failure of the run is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    /// A test as its runner names it (`binary test`, a Playwright title, a vitest spec file).
    Named(String),
    /// The job failed outside any test runner (a build, a lint, a step) or its log names no test.
    WholeJob,
    /// The job's log could not prove which tests failed (unreadable, or a runner that did not finish).
    Unread(String),
}

impl Test {
    fn label(&self) -> String {
        match self {
            Test::Named(n) => n.clone(),
            Test::WholeJob => "(the job failed outside any test)".into(),
            Test::Unread(_) => "(failing tests not readable)".into(),
        }
    }
}

/// What a job did in the baseline run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// It passed (or was skipped by design).
    Passed,
    /// It failed; the tests its log names (empty set: it failed outside any test runner), or why they are unknown.
    Failed(Result<BTreeSet<String>, String>),
    /// It has no result to compare with: the baseline has no job of that name, or it never ran or was cancelled.
    NoResult(String),
}

/// The run the failures are compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline {
    /// Its id.
    pub run: u64,
    /// The commit it tested.
    pub head: String,
    /// Whether it ran on main (the last main run, or a main run named with `--against`).
    pub main: bool,
    /// Its jobs by display name, those the triaged run failed in.
    pub jobs: BTreeMap<String, JobState>,
}

/// The answer for one failed test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Not on the baseline and not known flaky.
    New,
    /// It failed on the baseline run too.
    OnBaseline,
    /// `ci flakes` lists it: `count` of `runs` runs in the window.
    Flaky {
        /// Runs it was listed in.
        count: usize,
        /// Runs of the app in the window.
        runs: usize,
        /// Whether it also failed on the baseline run.
        also_on_baseline: bool,
    },
    /// Nothing proves new or old, with the reason.
    Unknown(String),
}

/// Flake evidence by test name: runs listed, runs examined.
pub type Flaky = BTreeMap<String, (usize, usize)>;

/// Classifies one failure; pure.
pub fn status_of(job: &str, test: &Test, baseline: Option<&Baseline>, flaky: &Flaky) -> Status {
    let name = match test {
        Test::Unread(why) => return Status::Unknown(why.clone()),
        Test::Named(n) => Some(n),
        Test::WholeJob => None,
    };
    let on_baseline = match baseline.map(|b| b.jobs.get(job)) {
        None => Err("there is no finished run of this workflow on main to compare with".to_string()),
        Some(None) => Err("the baseline run has no result for this job".to_string()),
        Some(Some(JobState::NoResult(why))) => Err(why.clone()),
        Some(Some(JobState::Passed)) => Ok(false),
        Some(Some(JobState::Failed(Err(why)))) => Err(why.clone()),
        // The baseline job failed before any test ran (a build, a lint): it says nothing about one test.
        Some(Some(JobState::Failed(Ok(names)))) if names.is_empty() && name.is_some() => {
            Err("the baseline job failed before any test ran, so it cannot say whether this test failed there".to_string())
        }
        // This job failed outside any test, but the baseline job failed in tests: not the same failure.
        Some(Some(JobState::Failed(Ok(names)))) if !names.is_empty() && name.is_none() => {
            Err("this job failed outside any test, but the baseline job failed in tests: not the same failure".to_string())
        }
        Some(Some(JobState::Failed(Ok(names)))) => Ok(name.is_none_or(|n| names.contains(n))),
    };
    if let Some((count, runs)) = name.and_then(|n| flaky.get(n)) {
        return Status::Flaky {
            count: *count,
            runs: *runs,
            also_on_baseline: matches!(on_baseline, Ok(true)),
        };
    }
    match on_baseline {
        Ok(true) => Status::OnBaseline,
        Ok(false) => Status::New,
        Err(why) => Status::Unknown(why),
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The failed job's display name.
    pub job: String,
    /// The failure.
    pub test: Test,
    /// The answer.
    pub status: Status,
}

/// The report of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The run triaged.
    pub run: u64,
    /// Its workflow file and ref (`debrief.yml@refs/heads/train/x`).
    pub path: String,
    /// The commit it tested.
    pub head: String,
    /// The comparison run, when one was found.
    pub baseline: Option<(u64, String, bool)>,
    /// Failed jobs that never ran a step (cancelled while queued) or call a reusable workflow: nothing to read.
    pub skipped_jobs: usize,
    /// One row per failed test (or per failed job with no named test).
    pub rows: Vec<Row>,
    /// What the history could not cover, and why.
    pub gaps: Vec<String>,
}

impl Status {
    fn word(&self, baseline_main: bool, baseline_run: Option<u64>) -> String {
        match self {
            Status::New => "NEW".into(),
            Status::OnBaseline if baseline_main => "ALREADY FAILING ON MAIN".into(),
            Status::OnBaseline => format!("ALREADY FAILING ON RUN {}", baseline_run.unwrap_or(0)),
            Status::Flaky {
                count,
                runs,
                also_on_baseline,
            } => {
                format!(
                    "FLAKY ({count} of {runs} runs){}",
                    if *also_on_baseline { ", also failed on the baseline" } else { "" }
                )
            }
            Status::Unknown(_) => "UNKNOWN".into(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Status::New => "new",
            Status::OnBaseline => "already-failing",
            Status::Flaky { .. } => "flaky",
            Status::Unknown(_) => "unknown",
        }
    }
}

impl Report {
    fn count(&self, kind: &str) -> usize {
        self.rows.iter().filter(|r| r.status.kind() == kind).count()
    }

    /// Whether every failure got an answer other than UNKNOWN and nothing the report relies on is missing (ER-33).
    pub fn complete(&self) -> bool {
        self.count("unknown") == 0 && self.gaps.is_empty()
    }

    /// 0 for a complete answer, 5 when a row is UNKNOWN or something is missing.
    pub fn exit(&self) -> paddock_cli::Exit {
        if self.complete() {
            paddock_cli::Exit::Ok
        } else {
            paddock_cli::Exit::Findings
        }
    }

    /// The report as a short table.
    pub fn text(&self) -> String {
        let (main, brun) = self.baseline.as_ref().map_or((false, None), |b| (b.2, Some(b.0)));
        let mut out = format!(
            "run {} {} at {}: {} failing test(s) or job(s): {} new, {} already failing, {} flaky, {} unknown\n",
            self.run,
            self.path,
            self.head.chars().take(9).collect::<String>(),
            self.rows.len(),
            self.count("new"),
            self.count("already-failing"),
            self.count("flaky"),
            self.count("unknown")
        );
        match &self.baseline {
            Some((id, head, true)) => out.push_str(&format!("compared with the last main run {id} ({})\n", &head[..head.len().min(9)])),
            Some((id, head, false)) => out.push_str(&format!("compared with run {id} ({})\n", &head[..head.len().min(9)])),
            None => out.push_str("no run to compare with was found\n"),
        }
        let words: Vec<String> = self.rows.iter().map(|r| r.status.word(main, brun)).collect();
        let w = words.iter().map(String::len).max().unwrap_or(6).max(6);
        let jw = self.rows.iter().map(|r| r.job.chars().count()).max().unwrap_or(3).clamp(3, 40);
        out.push_str(&format!("{:<w$}  {:<jw$}  TEST\n", "STATUS", "JOB"));
        for (r, word) in self.rows.iter().zip(&words).take(TEXT_ROWS) {
            out.push_str(&format!("{word:<w$}  {:<jw$}  {}\n", r.job, r.test.label()));
        }
        if self.rows.len() > TEXT_ROWS {
            out.push_str(&format!("... {} more (use --json)\n", self.rows.len() - TEXT_ROWS));
        }
        let mut why: BTreeSet<&str> = BTreeSet::new();
        for r in &self.rows {
            match (&r.status, &r.test) {
                (Status::Unknown(w), _) => {
                    why.insert(w);
                }
                (_, Test::Unread(w)) => {
                    why.insert(w);
                }
                _ => {}
            }
        }
        for w in why {
            out.push_str(&format!("unknown: {w}\n"));
        }
        for g in &self.gaps {
            out.push_str(&format!("note: {g}\n"));
        }
        if self.skipped_jobs > 0 {
            out.push_str(&format!(
                "note: {} failed job(s) never ran a step and are not listed\n",
                self.skipped_jobs
            ));
        }
        out
    }

    /// The report as JSON.
    pub fn json(&self) -> Value {
        json!({
            "run": self.run,
            "path": self.path,
            "head": self.head,
            "baseline": self.baseline.as_ref().map(|(id, head, main)| json!({"run": id, "head": head, "main": main})),
            "counts": {
                "new": self.count("new"),
                "already_failing": self.count("already-failing"),
                "flaky": self.count("flaky"),
                "unknown": self.count("unknown"),
            },
            "complete": self.complete(),
            "skipped_jobs": self.skipped_jobs,
            "gaps": self.gaps,
            "rows": self.rows.iter().map(|r| {
                let mut v = json!({
                    "job": r.job,
                    "test": match &r.test { Test::Named(n) => json!(n), _ => Value::Null },
                    "whole_job": r.test == Test::WholeJob,
                    "status": r.status.kind(),
                });
                if let Status::Flaky { count, runs, also_on_baseline } = &r.status {
                    v["flaky"] = json!({"count": count, "runs": runs, "also_on_baseline": also_on_baseline});
                }
                match (&r.status, &r.test) {
                    (Status::Unknown(w), _) | (_, Test::Unread(w)) => v["why"] = json!(w),
                    _ => {}
                }
                v
            }).collect::<Vec<_>>(),
        })
    }
}

/// What to triage.
pub struct Request {
    /// The run.
    pub run: u64,
    /// Compare with this run instead of the last main run.
    pub against: Option<u64>,
    /// The app whose flake history is read (default: from the run's workflow and its `app <name>` jobs).
    pub app: Option<String>,
    /// The window of the flake history, days.
    pub days: u32,
    /// Skip the flake history.
    pub history: bool,
    /// The repository.
    pub repo: String,
    /// The clock, seconds since the epoch.
    pub now_secs: i64,
}

/// The names a log lists as failed in this run: failed tests, failed vitest specs and flaky tests (flaky fails the step).
fn names_of(f: &logfind::Findings) -> BTreeSet<String> {
    f.failed.iter().chain(&f.vitest_failed).chain(&f.flaky).cloned().collect()
}

/// A failed job's tests: the names, `WholeJob` when the log has none, or why it cannot be said.
fn read_job(client: &mut Client, job: &TimedJob) -> Result<BTreeSet<String>, String> {
    match client.log(job.id) {
        Ok(Some(t)) if !t.trim().is_empty() => {
            let f = logfind::read(&t);
            match f.broken {
                Some(why) => Err(format!("job \"{}\": {why}", job.name)),
                None => Ok(names_of(&f)),
            }
        }
        Ok(_) => Err(format!("job \"{}\": the server has no log for it", job.name)),
        Err(e) => Err(format!("job \"{}\": its log could not be read ({e})", job.name)),
    }
}

/// The last finished run of `workflow` on main, other than `not`.
fn last_main_run(client: &mut Client, workflow: &str, not: u64) -> Result<Option<RunInfo>, String> {
    for page in 1..=BASELINE_PAGES {
        let runs = client.runs_page(page)?.runs;
        if runs.is_empty() {
            break;
        }
        if let Some(r) = runs.into_iter().find(|r| {
            r.id != not
                && r.workflow() == workflow
                && r.git_ref() == "refs/heads/main"
                && matches!(r.conclusion.as_str(), "success" | "failure")
        }) {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

fn baseline_jobs(
    client: &mut Client,
    run: &RunInfo,
    failed: &BTreeSet<String>,
    log: &mut dyn FnMut(&str),
) -> Result<BTreeMap<String, JobState>, String> {
    let jobs = client
        .timed_jobs(run.id)?
        .ok_or_else(|| format!("the server has no job list for run {}", run.id))?;
    let mut out = BTreeMap::new();
    for name in failed {
        let job = jobs.iter().find(|j| &j.name == name);
        let state = match job {
            None => JobState::NoResult(format!("run {} has no job named \"{name}\"", run.id)),
            Some(j) if !j.failed() => JobState::Passed,
            Some(j) if j.is_caller() => JobState::NoResult(format!("run {}: \"{name}\" only calls a reusable workflow", run.id)),
            Some(j) if j.never_ran() || j.conclusion == "cancelled" => {
                JobState::NoResult(format!("run {}: job \"{name}\" ended {} without running", run.id, j.conclusion))
            }
            Some(j) => {
                log(&format!("baseline run {} job {}: reading the log", run.id, j.id));
                JobState::Failed(read_job(client, j))
            }
        };
        out.insert(name.clone(), state);
    }
    Ok(out)
}

/// The workflow's own app (`debrief.yml`, `dyno-release.yml`), none for the shared workflows.
fn own_app(workflow: &str) -> Option<String> {
    let stem = workflow.trim_end_matches(".yml").split('-').next().unwrap_or("");
    (!stem.is_empty() && !matches!(stem, "ci" | "nightly" | "targeted" | "publish")).then(|| stem.to_string())
}

/// Apps whose flake history covers this run: the one named, else the workflow's own and every `app <name>` job; and the failed
/// jobs of shared code that no history covers.
fn history_apps(req: &Request, workflow: &str, failed_jobs: &[String]) -> (Vec<String>, Vec<String>) {
    if let Some(a) = &req.app {
        return (vec![a.clone()], Vec::new());
    }
    let own = own_app(workflow);
    let mut apps: BTreeSet<String> = own.iter().cloned().collect();
    let mut shared = BTreeSet::new();
    for j in failed_jobs {
        match j.strip_prefix("app ") {
            Some(rest) => {
                if let Some(a) = rest.split(|c: char| c.is_whitespace() || c == '(').next().filter(|a| !a.is_empty()) {
                    apps.insert(a.to_string());
                }
            }
            None if own.is_none() => {
                shared.insert(j.clone());
            }
            None => {}
        }
    }
    (apps.into_iter().collect(), shared.into_iter().collect())
}

/// The flake evidence of a `ci flakes` report: the tests listed as flaky or as failed-then-passed (a test that only failed is not
/// flaky).
pub fn flaky_from(r: &flakes::Report) -> Flaky {
    r.rows
        .iter()
        .filter(|x| x.kinds.contains(&Kind::Flaky) || x.kinds.contains(&Kind::FailedThenPassed))
        .map(|x| (x.name.clone(), (x.count, x.runs)))
        .collect()
}

/// Triages a run: reads the run, its failed jobs, the baseline and the history.
pub fn collect(client: &mut Client, req: &Request, log: &mut dyn FnMut(&str)) -> Result<Report, String> {
    let run = client
        .run(req.run)?
        .ok_or_else(|| format!("run {} does not exist on this server", req.run))?;
    if !run.finished() {
        return Err(format!(
            "run {} is {}: wait until it finishes (`./dev wait --ci-run {} --max 2h`), then triage it",
            run.id, run.status, run.id
        ));
    }
    let jobs = client
        .timed_jobs(run.id)?
        .ok_or_else(|| format!("the server has no job list for run {}", run.id))?;
    let mut skipped_jobs = 0;
    let mut failed: Vec<(&TimedJob, Result<BTreeSet<String>, String>)> = Vec::new();
    for j in jobs.iter().filter(|j| j.failed()) {
        if j.is_caller() || j.never_ran() {
            skipped_jobs += 1;
            continue;
        }
        log(&format!("run {} job {} \"{}\": reading the log", run.id, j.id, j.name));
        failed.push((j, read_job(client, j)));
    }
    let failed_names: BTreeSet<String> = failed.iter().map(|(j, _)| j.name.clone()).collect();
    let mut gaps = Vec::new();
    // The baseline.
    let base_run = match req.against {
        Some(id) => Some(
            client
                .run(id)?
                .map(|r| {
                    let main = r.git_ref() == "refs/heads/main";
                    (r, main)
                })
                .ok_or_else(|| format!("--against {id}: no such run"))?,
        ),
        None => last_main_run(client, run.workflow(), run.id)?.map(|r| (r, true)),
    };
    let baseline = match base_run {
        None => {
            gaps.push(format!(
                "no finished run of {} on main was found in the newest {} runs",
                run.workflow(),
                BASELINE_PAGES * 50
            ));
            None
        }
        Some((b, main)) => {
            let jobs = baseline_jobs(client, &b, &failed_names, log)?;
            Some(Baseline {
                run: b.id,
                head: b.head_sha.clone(),
                main,
                jobs,
            })
        }
    };
    // The history.
    let mut flaky: Flaky = BTreeMap::new();
    if req.history {
        let names: Vec<String> = failed.iter().map(|(j, _)| j.name.clone()).collect();
        let (apps, shared) = history_apps(req, run.workflow(), &names);
        if apps.is_empty() {
            gaps.push("flake history: none (ci flakes reads one app at a time, and this run's failures belong to no app); pass --app to choose one".into());
        } else if !shared.is_empty() {
            gaps.push(format!(
                "flake history covers {}; shared jobs ({}) have none, so their NEW is not checked against it",
                apps.join(", "),
                shared.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        for app in &apps {
            let freq = flakes::Request {
                app: app.clone(),
                days: req.days,
                until: flakes::Until::Now,
                repo: req.repo.clone(),
                now_secs: req.now_secs,
                preconditions: Vec::new(),
                config_source: "ci triage".into(),
            };
            log(&format!("flake history of {app}, {} days", req.days));
            match flakes::collect(client, &freq, log) {
                Ok(r) => {
                    if !r.complete() {
                        gaps.push(format!("flake history of {app} is incomplete: {}", r.error_line()));
                    }
                    flaky.extend(flaky_from(&r));
                }
                Err(e) => gaps.push(format!("flake history of {app} could not be read: {e}")),
            }
        }
    }
    let mut rows = Vec::new();
    for (j, tests) in &failed {
        let tests: Vec<Test> = match tests {
            Err(why) => vec![Test::Unread(why.clone())],
            Ok(names) if names.is_empty() => vec![Test::WholeJob],
            Ok(names) => names.iter().cloned().map(Test::Named).collect(),
        };
        for t in tests {
            let status = status_of(&j.name, &t, baseline.as_ref(), &flaky);
            rows.push(Row {
                job: j.name.clone(),
                test: t,
                status,
            });
        }
    }
    Ok(Report {
        run: run.id,
        path: run.path.clone(),
        head: run.head_sha.clone(),
        baseline: baseline.map(|b| (b.run, b.head, b.main)),
        skipped_jobs,
        rows,
        gaps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;
    use std::path::Path;

    const MAC: &str = "rust tests (macOS)";
    const LINUX: &str = "rust tests";
    /// Failing in run 7963 (macOS) and in main run 7941 (macOS).
    const CHALLENGE: &str =
        "debrief-app browser::renderer::tests::a_challenge_page_comes_to_the_front_and_the_work_continues_when_it_is_solved";
    /// Failing in run 7963 (macOS) only.
    const SCRIPTED: &str = "debrief-app browser::renderer::tests::reads_a_scripted_page_through_the_ports_with_nothing_of_ours_in_it";
    /// Failing in run 7963 (Linux) only.
    const TICKER: &str = "debrief-app jobs::service::tests::probes_and_reports_reach_the_snapshot_through_the_ticker";

    fn fake() -> Fake {
        // Recorded from the real server with a read token (runs 7941, 7961, 7963, 7965 of debrief.yml; the logs keep only the
        // lines of the nextest reader).
        Fake::from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ci-triage"), "example/paddock")
    }

    fn triage(run: u64, against: Option<u64>) -> Report {
        let mut f = fake();
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        c.backoff_ms = 0;
        let req = Request {
            run,
            against,
            app: None,
            days: 14,
            history: false,
            repo: "example/paddock".into(),
            now_secs: 0,
        };
        collect(&mut c, &req, &mut |_| {}).unwrap()
    }

    fn row<'a>(r: &'a Report, job: &str, test: &str) -> &'a Row {
        r.rows
            .iter()
            .find(|x| x.job == job && x.test == Test::Named(test.into()))
            .unwrap_or_else(|| panic!("no row {job} {test}: {:?}", r.rows))
    }

    fn base(jobs: &[(&str, JobState)]) -> Baseline {
        Baseline {
            run: 9,
            head: "abc".into(),
            main: true,
            jobs: jobs.iter().map(|(n, s)| (n.to_string(), s.clone())).collect(),
        }
    }

    fn names(n: &[&str]) -> JobState {
        JobState::Failed(Ok(n.iter().map(|s| s.to_string()).collect()))
    }

    fn named(n: &str) -> Test {
        Test::Named(n.into())
    }

    // The classification.

    #[test]
    fn a_test_whose_job_passed_on_the_baseline_is_new() {
        let b = base(&[("j", JobState::Passed)]);
        assert_eq!(status_of("j", &named("t"), Some(&b), &Flaky::new()), Status::New);
    }

    #[test]
    fn a_test_the_baseline_job_also_failed_is_already_failing_and_another_failure_in_that_job_is_new() {
        let b = base(&[("j", names(&["t"]))]);
        assert_eq!(status_of("j", &named("t"), Some(&b), &Flaky::new()), Status::OnBaseline);
        assert_eq!(status_of("j", &named("u"), Some(&b), &Flaky::new()), Status::New);
    }

    #[test]
    fn flaky_history_wins_and_says_when_the_test_also_failed_on_the_baseline() {
        let flaky: Flaky = [("t".to_string(), (3, 40))].into();
        let b = base(&[("j", JobState::Passed)]);
        assert_eq!(
            status_of("j", &named("t"), Some(&b), &flaky),
            Status::Flaky {
                count: 3,
                runs: 40,
                also_on_baseline: false
            }
        );
        let b = base(&[("j", names(&["t"]))]);
        assert_eq!(
            status_of("j", &named("t"), Some(&b), &flaky),
            Status::Flaky {
                count: 3,
                runs: 40,
                also_on_baseline: true
            }
        );
    }

    #[test]
    fn a_missing_comparison_is_unknown_never_new() {
        let none = Flaky::new();
        assert!(matches!(status_of("j", &named("t"), None, &none), Status::Unknown(_)));
        let b = base(&[]);
        assert!(matches!(status_of("j", &named("t"), Some(&b), &none), Status::Unknown(_)));
        let b = base(&[("j", JobState::NoResult("cancelled".into()))]);
        assert!(matches!(status_of("j", &named("t"), Some(&b), &none), Status::Unknown(w) if w == "cancelled"));
        // The baseline job failed and its log could not say which tests: not "new".
        let b = base(&[("j", JobState::Failed(Err("log gone".into())))]);
        assert!(matches!(status_of("j", &named("t"), Some(&b), &none), Status::Unknown(w) if w == "log gone"));
        assert!(matches!(
            status_of("j", &Test::Unread("broken".into()), Some(&base(&[("j", JobState::Passed)])), &none),
            Status::Unknown(w) if w == "broken"
        ));
    }

    #[test]
    fn a_baseline_that_failed_before_any_test_ran_cannot_make_a_named_failure_new() {
        let b = base(&[("j", names(&[]))]);
        assert!(matches!(status_of("j", &named("t"), Some(&b), &Flaky::new()), Status::Unknown(w) if w.contains("before any test ran")));
    }

    #[test]
    fn a_failure_outside_tests_is_not_the_baselines_failure_in_tests() {
        let b = base(&[("j", names(&["t"]))]);
        assert!(
            matches!(status_of("j", &Test::WholeJob, Some(&b), &Flaky::new()), Status::Unknown(w) if w.contains("not the same failure"))
        );
    }

    #[test]
    fn a_job_that_failed_outside_any_test_is_compared_by_job() {
        let none = Flaky::new();
        assert_eq!(
            status_of("build", &Test::WholeJob, Some(&base(&[("build", names(&[]))])), &none),
            Status::OnBaseline
        );
        assert_eq!(
            status_of("build", &Test::WholeJob, Some(&base(&[("build", JobState::Passed)])), &none),
            Status::New
        );
    }

    #[test]
    fn only_flaky_and_failed_then_passed_count_as_flaky_history() {
        let row = |n: &str, k: &[Kind]| flakes::Row {
            name: n.into(),
            kinds: k.iter().copied().collect(),
            count: 2,
            runs: 10,
            last_seen: "2026-11-11".into(),
        };
        let r = flakes::Report {
            app: "debrief".into(),
            repo: "r".into(),
            days: 14,
            since: String::new(),
            until: String::new(),
            runs: 10,
            logs_read: 0,
            rows: vec![
                row("flaky", &[Kind::Flaky]),
                row("ftp", &[Kind::Failed, Kind::FailedThenPassed]),
                row("only-failed", &[Kind::Failed]),
                row("spec", &[Kind::VitestFailed]),
            ],
            non_test_failures: 0,
            never_ran: 0,
            cancelled_not_read: 0,
            config_source: String::new(),
            not_measured: Vec::new(),
            gaps: Vec::new(),
        };
        let f = flaky_from(&r);
        assert_eq!(f.keys().cloned().collect::<Vec<_>>(), ["flaky", "ftp"]);
        assert_eq!(f["flaky"], (2, 10));
    }

    #[test]
    fn history_covers_the_workflows_own_app_and_app_jobs_and_names_shared_jobs() {
        let req = |app: Option<&str>| Request {
            run: 1,
            against: None,
            app: app.map(str::to_string),
            days: 14,
            history: true,
            repo: "r".into(),
            now_secs: 0,
        };
        let jobs = vec![LINUX.to_string(), "app dyno (ubuntu-latest, e2e-1)".to_string()];
        assert_eq!(
            history_apps(&req(None), "debrief.yml", &jobs),
            (vec!["debrief".to_string(), "dyno".to_string()], vec![])
        );
        assert_eq!(history_apps(&req(None), "dyno-release.yml", &[]).0, ["dyno"]);
        assert_eq!(
            history_apps(&req(None), "ci.yml", &jobs),
            (vec!["dyno".to_string()], vec![LINUX.to_string()])
        );
        assert_eq!(
            history_apps(&req(Some("pitwall")), "ci.yml", &jobs),
            (vec!["pitwall".to_string()], vec![])
        );
    }

    // The recorded runs.

    #[test]
    fn run_7963_against_the_last_main_run_is_all_new_because_main_passed_those_jobs() {
        let r = triage(7963, None);
        // The last finished main run of debrief.yml in the recording is 7965; its rust tests passed.
        assert_eq!(r.baseline.as_ref().map(|b| (b.0, b.2)), Some((7965, true)));
        assert_eq!(r.rows.len(), 3, "{:?}", r.rows);
        for (job, test) in [(MAC, CHALLENGE), (MAC, SCRIPTED), (LINUX, TICKER)] {
            assert_eq!(row(&r, job, test).status, Status::New, "{job} {test}");
        }
        assert!(r.complete());
        assert_eq!(r.exit(), paddock_cli::Exit::Ok);
    }

    #[test]
    fn the_baseline_is_the_newest_main_run_even_when_a_newer_run_of_the_workflow_is_on_a_branch() {
        let mut f = fake();
        // A newer, finished run of the same workflow on a train branch: it must not be taken for the baseline (it has no job
        // list in the recording, so taking it would fail the read).
        let mut runs = f.take_runs();
        let mut branch = runs[0].clone();
        branch["id"] = json!(7966);
        branch["path"] = json!("debrief.yml@refs/heads/train/2026-11-12-6");
        branch["conclusion"] = json!("success");
        runs.insert(0, branch);
        f.set_runs(runs);
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        c.backoff_ms = 0;
        let req = Request {
            run: 7963,
            against: None,
            app: None,
            days: 14,
            history: false,
            repo: "example/paddock".into(),
            now_secs: 0,
        };
        let r = collect(&mut c, &req, &mut |_| {}).unwrap();
        assert_eq!(r.baseline.as_ref().map(|b| (b.0, b.2)), Some((7965, true)));
    }

    #[test]
    fn an_incomplete_answer_exits_5_even_when_every_row_is_answered() {
        let mut r = triage(7963, None);
        assert_eq!(r.exit(), paddock_cli::Exit::Ok);
        r.gaps.push("flake history of x is incomplete".into());
        assert!(!r.complete());
        assert_eq!(r.exit(), paddock_cli::Exit::Findings);
        assert_eq!(r.json()["complete"], false);
    }

    #[test]
    fn run_7963_against_main_run_7941_finds_the_test_that_main_already_failed() {
        let r = triage(7963, Some(7941));
        assert_eq!(row(&r, MAC, CHALLENGE).status, Status::OnBaseline);
        // 7941's macOS job failed in other tests, so these are new.
        assert_eq!(row(&r, MAC, SCRIPTED).status, Status::New);
        // 7941's Linux job passed.
        assert_eq!(row(&r, LINUX, TICKER).status, Status::New);
        let text = r.text();
        assert!(
            text.contains("ALREADY FAILING ON MAIN") && text.contains("compared with the last main run 7941"),
            "{text}"
        );
        let j = r.json();
        assert_eq!(j["counts"]["already_failing"], 1);
        assert_eq!(j["counts"]["new"], 2);
        assert_eq!(j["baseline"]["run"], 7941);
    }

    #[test]
    fn run_7961_has_one_timed_out_test_and_it_is_new_against_main() {
        let r = triage(7961, None);
        assert_eq!(r.rows.len(), 1);
        assert_eq!(
            row(
                &r,
                MAC,
                "debrief-store-duck::it regress_1_2::building_the_catalogue_keeps_its_unsaved_rows_bounded"
            )
            .status,
            Status::New
        );
    }

    #[test]
    fn a_baseline_that_is_not_on_main_is_named_by_its_run() {
        // 7961 is a train run: compared with it, the 7963 failures are failures of another run.
        let r = triage(7963, Some(7961));
        let text = r.text();
        assert!(text.contains("compared with run 7961"), "{text}");
        assert_eq!(row(&r, MAC, CHALLENGE).status, Status::New);
    }

    #[test]
    fn the_text_is_a_short_table_with_the_counts_first() {
        let t = triage(7963, Some(7941)).text();
        let lines: Vec<&str> = t.lines().collect();
        assert!(
            lines[0].starts_with("run 7963 debrief.yml@refs/heads/train/2026-11-11-5 at 607bb7928: 3 failing"),
            "{t}"
        );
        assert!(lines[0].contains("2 new, 1 already failing, 0 flaky, 0 unknown"), "{t}");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("STATUS") && l.contains("JOB") && l.ends_with("TEST")),
            "{t}"
        );
    }

    #[test]
    fn a_run_that_is_still_going_is_refused_with_the_wait_command() {
        let mut f = fake();
        let mut v: Value = serde_json::from_str(include_str!("../../tests/ci-triage/run-7963.json")).unwrap();
        v["status"] = json!("in_progress");
        v["conclusion"] = json!("");
        f.json("https://gitea.invalid/api/v1/repos/example/paddock/actions/runs/7963", v);
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        let req = Request {
            run: 7963,
            against: None,
            app: None,
            days: 14,
            history: false,
            repo: "example/paddock".into(),
            now_secs: 0,
        };
        let e = collect(&mut c, &req, &mut |_| {}).unwrap_err();
        assert!(e.contains("is in_progress") && e.contains("./dev wait --ci-run 7963"), "{e}");
    }

    #[test]
    fn an_unreadable_failed_log_is_unknown_for_that_job_and_the_others_are_still_answered() {
        let mut f = fake();
        f.fail(36182, "curl exited with 22: 500");
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        c.backoff_ms = 0;
        let req = Request {
            run: 7963,
            against: Some(7941),
            app: None,
            days: 14,
            history: false,
            repo: "example/paddock".into(),
            now_secs: 0,
        };
        let r = collect(&mut c, &req, &mut |_| {}).unwrap();
        assert!(!r.complete());
        let mac: Vec<&Row> = r.rows.iter().filter(|x| x.job == MAC).collect();
        assert_eq!(mac.len(), 1);
        assert!(
            matches!(&mac[0].status, Status::Unknown(w) if w.contains("could not be read")),
            "{:?}",
            mac[0]
        );
        assert_eq!(row(&r, LINUX, TICKER).status, Status::New);
        assert_eq!(r.json()["complete"], false);
    }

    fn history_request() -> Request {
        Request {
            run: 7963,
            against: None,
            app: None,
            days: 14,
            history: true,
            repo: "example/paddock".into(),
            now_secs: crate::ci_runs::time::ts("2026-11-12T08:00:00Z").map_or(0, |ms| (ms / 1000.0) as i64),
        }
    }

    #[test]
    fn the_history_is_read_and_a_clean_window_leaves_the_answer_complete() {
        let mut f = fake();
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        c.backoff_ms = 0;
        let r = collect(&mut c, &history_request(), &mut |_| {}).unwrap();
        assert!(r.gaps.is_empty(), "{:?}", r.gaps);
        assert_eq!(r.rows.len(), 3);
        assert!(r.rows.iter().all(|x| x.status == Status::New), "{:?}", r.rows);
    }

    #[test]
    fn a_history_window_that_cannot_be_read_is_a_gap_not_flaky_or_clean() {
        let mut f = fake();
        // The log of a failed job of run 7941, which is in the window: the history cannot account for it.
        f.fail(35684, "curl exited with 22: 500");
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        c.backoff_ms = 0;
        let r = collect(&mut c, &history_request(), &mut |_| {}).unwrap();
        assert!(r.gaps.iter().any(|g| g.contains("flake history of debrief")), "{:?}", r.gaps);
        assert!(!r.complete());
        assert_eq!(r.rows.len(), 3);
    }

    #[test]
    fn the_command_is_read_only_it_only_gets() {
        // Every request the recorded fake saw is a GET by construction (the Http trait has no other verb); the client never
        // asks for a rerun, a cancel or a delete URL.
        let mut f = fake();
        let mut c = Client::new(&mut f, "https://gitea.invalid", "example/paddock", "t");
        let req = Request {
            run: 7963,
            against: None,
            app: None,
            days: 14,
            history: false,
            repo: "example/paddock".into(),
            now_secs: 0,
        };
        collect(&mut c, &req, &mut |_| {}).unwrap();
        for u in f.requested() {
            assert!(!u.contains("/rerun") && !u.contains("/cancel") && !u.contains("/delete"), "{u}");
        }
    }
}
