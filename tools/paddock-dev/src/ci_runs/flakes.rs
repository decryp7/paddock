//! `ci flakes`: the tests CI marked flaky or saw fail, over a window of runs (ADR 0082 D6).
//!
//! What counts, per test and per run (a run counts once however many jobs, attempts or retries show the test):
//!
//! - **flaky**: nextest ended it `FLAKY`/`FLKY-*` or it passed on a retry; Playwright listed it under `N flaky`;
//! - **failed**: nextest or Playwright ended it failed;
//! - **vitest-failed**: vitest has no flaky marker, so every failed spec file counts;
//! - **failed-then-passed**: it failed in a job that passed on a later attempt of the same run (Gitea reruns are attempts), or
//!   in a run whose workflow later passed on the same commit with the same job passing.
//!
//! Only the logs of jobs that did not pass are read; a job with no step timings (no runner, or every step starts and ends
//! together) is a gap. A failed job whose log cannot be read, or whose log the reader cannot account for, is **not measured**,
//! never "no flakes" (ER-33), and so is a precondition that makes flakes invisible in passing jobs (Playwright retries without
//! `failOnFlakyTests`).

use super::client::{Client, RunInfo, TimedJob};
use super::logfind;
use super::time::{date_of, parse_ms};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The most pages of the run list read (50 runs a page); a window that needs more is reported incomplete.
pub const MAX_PAGES: u32 = 60;

/// How a test got on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Marked flaky by the runner.
    Flaky,
    /// Ended failed (nextest, Playwright).
    Failed,
    /// A failed vitest spec file.
    VitestFailed,
    /// Failed, then passed on a later attempt or a rerun of the same commit.
    FailedThenPassed,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Flaky => "flaky",
            Kind::Failed => "failed",
            Kind::VitestFailed => "vitest-failed",
            Kind::FailedThenPassed => "failed-then-passed",
        }
    }
}

/// One test of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The test as its runner names it (`binary test`, a Playwright title, a vitest spec file).
    pub name: String,
    /// Why it is listed.
    pub kinds: BTreeSet<Kind>,
    /// The runs it was listed in.
    pub count: usize,
    /// The runs of the app examined in the window (the same for every test).
    pub runs: usize,
    /// The date of the newest run it was listed in.
    pub last_seen: String,
}

/// A failed job whose log could not prove anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotMeasured {
    /// The run.
    pub run: u64,
    /// The job's id and name.
    pub job: String,
    /// Why.
    pub why: String,
}

/// Where the window ends (`--until`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Until {
    /// Now.
    Now,
    /// The end of this `YYYY-MM-DD` (UTC).
    Date(String),
    /// The newest run whose commit starts with this.
    Commit(String),
}

impl Until {
    /// `--until` as typed: a date, else a commit prefix (at least 7 hex digits).
    pub fn parse(s: &str) -> Result<Until, String> {
        if s.len() == 10 && parse_ms(&format!("{s}T00:00:00Z")).is_some() {
            Ok(Until::Date(s.to_string()))
        } else if s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(Until::Commit(s.to_ascii_lowercase()))
        } else {
            Err(format!(
                "--until \"{s}\" is neither a date (YYYY-MM-DD) nor a commit of at least 7 hex digits"
            ))
        }
    }
}

/// What to measure.
#[derive(Debug, Clone)]
pub struct Request {
    /// The app.
    pub app: String,
    /// The window in days.
    pub days: u32,
    /// The window's end.
    pub until: Until,
    /// The repository.
    pub repo: String,
    /// The clock, seconds since the epoch.
    pub now_secs: i64,
    /// Gaps in what the runs can show at all, found before the runs are read (for example a Playwright setting).
    pub preconditions: Vec<String>,
    /// Where the app's settings (Playwright, nextest) were read from, for the report (`the working tree`, `commit abc1234`).
    pub config_source: String,
}

/// The report of a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The app asked for.
    pub app: String,
    /// The repository read.
    pub repo: String,
    /// The window in days.
    pub days: u32,
    /// The first day of the window.
    pub since: String,
    /// The last day of the window.
    pub until: String,
    /// The finished runs of the app in the window.
    pub runs: usize,
    /// Failed job logs read.
    pub logs_read: usize,
    /// The listed tests, most often first.
    pub rows: Vec<Row>,
    /// Failed jobs that failed outside any test runner (a build, a lint).
    pub non_test_failures: usize,
    /// Failed jobs with no step timings (no runner took them, or every step starts and ends together): nothing to read.
    pub never_ran: usize,
    /// Cancelled runs of the app's own workflows, whose jobs are not read.
    pub cancelled_not_read: usize,
    /// Where the app's settings were read from.
    pub config_source: String,
    /// What could not be measured, job by job.
    pub not_measured: Vec<NotMeasured>,
    /// What could not be measured about the window as a whole (a setting, the run list).
    pub gaps: Vec<String>,
}

impl Report {
    /// Whether every run of the window was measured.
    pub fn complete(&self) -> bool {
        self.not_measured.is_empty() && self.gaps.is_empty() && self.runs > 0
    }

    /// The report as text for people.
    pub fn text(&self) -> String {
        let mut l = vec![format!(
            "{}: flaky and failed tests {} to {} ({} days, {}); {} runs, {} failed job logs read",
            self.app, self.since, self.until, self.days, self.repo, self.runs, self.logs_read
        )];
        if self.rows.is_empty() {
            l.push(if self.complete() {
                "no flaky or failed test found in the measured runs".into()
            } else {
                "no flaky or failed test found in the runs that could be measured (see below)".into()
            });
        } else {
            let width = self.rows.iter().map(|r| r.name.chars().count()).max().unwrap_or(4).min(110);
            l.push(format!(
                "{:<width$}  {:<28}  {:>5}  {:>5}  last seen",
                "test", "kinds", "count", "runs"
            ));
            for r in &self.rows {
                let kinds = r.kinds.iter().map(|k| k.name()).collect::<Vec<_>>().join(",");
                l.push(format!(
                    "{:<width$}  {:<28}  {:>5}  {:>5}  {}",
                    r.name, kinds, r.count, r.runs, r.last_seen
                ));
            }
        }
        if self.non_test_failures > 0 {
            l.push(format!(
                "{} failed job(s) failed outside any test runner (build, lint, setup) and are not counted",
                self.non_test_failures
            ));
        }
        if self.cancelled_not_read > 0 {
            l.push(format!(
                "{} cancelled run(s) not read (counted and listed here; they do not change the exit code)",
                self.cancelled_not_read
            ));
        }
        if !self.config_source.is_empty() {
            l.push(format!("settings read from {}", self.config_source));
        }
        if self.runs == 0 {
            l.push("NOT MEASURED: no finished run of this app in the window, so this says nothing about flakes".into());
        }
        for g in &self.gaps {
            l.push(format!("NOT MEASURED: {g}"));
        }
        for n in &self.not_measured {
            l.push(format!("NOT MEASURED: run {} job {}: {}", n.run, n.job, n.why));
        }
        if !self.complete() {
            l.push("The list is incomplete: a gap above is not evidence of no flakes (ER-33).".into());
        }
        l.join("\n")
    }

    /// The text of the `::error::` annotation of an incomplete measurement: the cause and the next step.
    pub fn error_line(&self) -> String {
        let mut causes: Vec<String> = Vec::new();
        if self.runs == 0 {
            causes.push("no finished run of the app in the window".into());
        }
        causes.extend(self.gaps.iter().cloned());
        if let Some(first) = self.not_measured.first() {
            causes.push(format!(
                "{} failed job(s) could not be measured (first: run {} job {}: {})",
                self.not_measured.len(),
                first.run,
                first.job,
                first.why
            ));
        }
        format!(
            "ci flakes {}: not measured: {}. Next step: fix the cause or widen --days, then run it again; \
             an incomplete list is not evidence of no flakes.",
            self.app,
            causes.join("; ")
        )
    }

    /// The report as JSON.
    pub fn json(&self) -> Value {
        json!({
            "app": self.app,
            "repo": self.repo,
            "days": self.days,
            "since": self.since,
            "until": self.until,
            "runs": self.runs,
            "logsRead": self.logs_read,
            "complete": self.complete(),
            "flakes": self.rows.iter().map(|r| json!({
                "name": r.name,
                "kinds": r.kinds.iter().map(|k| k.name()).collect::<Vec<_>>(),
                "count": r.count,
                "runs": r.runs,
                "lastSeen": r.last_seen,
            })).collect::<Vec<_>>(),
            "nonTestFailures": self.non_test_failures,
            "neverRan": self.never_ran,
            "cancelledNotRead": self.cancelled_not_read,
            "configSource": self.config_source,
            "notMeasured": self.not_measured.iter().map(|n| json!({"run": n.run, "job": n.job, "why": n.why})).collect::<Vec<_>>(),
            "gaps": self.gaps,
        })
    }
}

/// Whether a workflow file is the app's own (`dyno.yml`, `dyno-release.yml`): all its jobs are the app's.
fn own_workflow(app: &str, file: &str) -> bool {
    file == format!("{app}.yml") || file.starts_with(&format!("{app}-"))
}

/// Whether a job of a shared workflow is the app's (`app dyno (ubuntu-latest, e2e-1)`): its name has the app as a word.
fn app_job(app: &str, name: &str) -> bool {
    name.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).any(|w| w == app)
}

struct Window {
    runs: Vec<RunInfo>,
    since_ms: f64,
    until_ms: f64,
    gap: Option<String>,
}

/// The runs of the window, newest first. Pages are read until one reaches the window's start; for a commit the end of the
/// window is found on the way.
fn list_window(client: &mut Client, req: &Request) -> Result<Window, String> {
    let day = 86_400_000.0;
    let mut until_ms = match &req.until {
        Until::Now => Some(req.now_secs as f64 * 1000.0),
        Until::Date(d) => parse_ms(&format!("{d}T00:00:00Z")).map(|t| t + day),
        Until::Commit(_) => None,
    };
    let span = f64::from(req.days) * day;
    let mut all: Vec<RunInfo> = Vec::new();
    let mut gap = None;
    let mut done = false;
    for page in 1..=MAX_PAGES {
        let p = client.runs_page(page)?;
        let n = p.runs.len();
        let mut oldest = f64::INFINITY;
        for r in p.runs {
            let Some(t) = parse_ms(&r.created_at) else {
                gap = Some(format!(
                    "run {} has a creation time that cannot be read (\"{}\")",
                    r.id, r.created_at
                ));
                done = true;
                break;
            };
            if let Until::Commit(c) = &req.until
                && until_ms.is_none()
                && r.head_sha.starts_with(c.as_str())
            {
                until_ms = Some(t); // the newest run of the commit ends the window (inclusive)
            }
            oldest = oldest.min(t);
            all.push(r);
        }
        if done || n < 50 || until_ms.is_some_and(|u| oldest < u - span) {
            done = true;
            break;
        }
    }
    let Some(until_ms) = until_ms else {
        return Err(match &req.until {
            Until::Commit(c) => format!("no run of a commit starting with {c} was found in the run list"),
            _ => "the end of the window cannot be read".into(),
        });
    };
    if !done {
        gap = Some(format!("the window needs more than {MAX_PAGES} pages of runs"));
    }
    let since_ms = until_ms - span;
    let runs = all
        .into_iter()
        .filter(|r| parse_ms(&r.created_at).is_some_and(|t| t >= since_ms && t <= until_ms))
        .collect();
    Ok(Window {
        runs,
        since_ms,
        until_ms,
        gap,
    })
}

/// A test's evidence: the kinds, and the runs (id to creation time) it was seen in.
type Seen = BTreeMap<String, (BTreeSet<Kind>, BTreeMap<u64, String>)>;

/// Measures the window: lists the runs, reads the logs of the app's failed jobs (every attempt) and counts the tests.
pub fn collect(client: &mut Client, req: &Request, log: &mut dyn FnMut(&str)) -> Result<Report, String> {
    let app = req.app.as_str();
    let w = list_window(client, req)?;
    let finished: Vec<&RunInfo> = w
        .runs
        .iter()
        .filter(|r| r.finished() && matches!(r.conclusion.as_str(), "success" | "failure"))
        .collect();
    // Runs of a workflow that passed on a commit: a failed run of the same workflow and commit was a flake or fixed by a rerun
    // when the job of the same name passed in one of them.
    let mut passed: BTreeMap<(&str, &str), Vec<u64>> = BTreeMap::new();
    for r in finished.iter().filter(|r| r.conclusion == "success") {
        passed.entry((r.workflow(), r.head_sha.as_str())).or_default().push(r.id);
    }
    let mut report = Report {
        app: app.to_string(),
        repo: req.repo.clone(),
        days: req.days,
        since: date_of((w.since_ms / 1000.0) as i64),
        until: date_of((w.until_ms / 1000.0) as i64 - 1),
        runs: 0,
        logs_read: 0,
        rows: Vec::new(),
        non_test_failures: 0,
        never_ran: 0,
        cancelled_not_read: 0,
        config_source: req.config_source.clone(),
        not_measured: Vec::new(),
        gaps: req.preconditions.clone(),
    };
    report.gaps.extend(w.gap);
    let mut seen: Seen = BTreeMap::new();
    let mut later_jobs: HashMap<u64, Result<Option<Vec<TimedJob>>, String>> = HashMap::new();
    log(&format!("{} runs listed, {} finished", w.runs.len(), finished.len()));
    // Cancelled runs are counted, not read: of the app's own workflows at once, of shared ones when a job of the app is in them.
    for r in w.runs.iter().filter(|r| r.finished() && r.conclusion == "cancelled") {
        if own_workflow(app, r.workflow()) {
            report.cancelled_not_read += 1;
            continue;
        }
        match client.attempt_jobs(r.id, None) {
            Ok(Some(jobs)) => report.cancelled_not_read += usize::from(jobs.iter().any(|j| app_job(app, &j.name))),
            Ok(None) | Err(_) => report.not_measured.push(NotMeasured {
                run: r.id,
                job: "(a cancelled run's job list)".into(),
                why: "whether the cancelled run has jobs of the app could not be read".into(),
            }),
        }
    }
    let mut never_ran_jobs: BTreeSet<(u64, u64)> = BTreeSet::new();
    for run in finished {
        let own = own_workflow(app, run.workflow());
        let last = run.run_attempt;
        if own && run.conclusion == "success" && last == 1 {
            report.runs += 1; // nothing failed, nothing to read
            continue;
        }
        let mut attempts: Vec<Vec<TimedJob>> = Vec::new();
        let mut unread = false;
        for n in 1..=last {
            match client.attempt_jobs(run.id, (n < last).then_some(n)) {
                Ok(Some(jobs)) => attempts.push(jobs.into_iter().filter(|j| own || app_job(app, &j.name)).collect()),
                Ok(None) => {
                    unread = true;
                    attempts.push(Vec::new());
                    report.not_measured.push(NotMeasured {
                        run: run.id,
                        job: format!("(the job list of attempt {n})"),
                        why: "the server has no job list for it (404)".into(),
                    });
                }
                Err(e) => {
                    unread = true;
                    attempts.push(Vec::new());
                    report.not_measured.push(NotMeasured {
                        run: run.id,
                        job: format!("(the job list of attempt {n})"),
                        why: format!("the jobs could not be read: {e}"),
                    });
                }
            }
        }
        if attempts.iter().all(Vec::is_empty) && !unread {
            continue; // a shared workflow's run with no job of this app
        }
        report.runs += 1;
        for (k, mine) in attempts.iter().enumerate() {
            for job in mine.iter().filter(|j| j.failed()) {
                if job.is_caller() {
                    continue; // a reusable workflow's caller: its children are listed beside it
                }
                if job.never_ran() {
                    report.never_ran += 1;
                    never_ran_jobs.insert((run.id, job.id));
                    continue;
                }
                let label = format!("{} \"{}\"", job.id, job.name);
                let text = match client.log(job.id) {
                    Ok(Some(t)) if !t.trim().is_empty() => t,
                    Ok(_) => {
                        report.not_measured.push(NotMeasured {
                            run: run.id,
                            job: label,
                            why: format!(
                                "the server has no log for it (the job ended {})",
                                if job.conclusion.is_empty() { "unknown" } else { &job.conclusion }
                            ),
                        });
                        continue;
                    }
                    Err(e) => {
                        report.not_measured.push(NotMeasured {
                            run: run.id,
                            job: label,
                            why: format!("its log could not be read: {e}"),
                        });
                        continue;
                    }
                };
                report.logs_read += 1;
                let f = logfind::read(&text);
                if let Some(why) = &f.broken {
                    report.not_measured.push(NotMeasured {
                        run: run.id,
                        job: label,
                        why: format!("check broken: {why}"),
                    });
                    continue;
                }
                log(&format!(
                    "run {} attempt {} job {}: {} flaky, {} failed, {} vitest spec(s) failed",
                    run.id,
                    k + 1,
                    job.id,
                    f.flaky.len(),
                    f.failed.len(),
                    f.vitest_failed.len()
                ));
                if !f.any() {
                    report.non_test_failures += 1;
                    continue;
                }
                let later_pass = if k + 1 < attempts.len() {
                    attempts[k + 1..]
                        .iter()
                        .flatten()
                        .any(|j| j.name == job.name && j.conclusion == "success")
                } else {
                    rerun_passed(client, &passed, &mut later_jobs, run, &job.name, &mut report.not_measured)
                };
                let mut add = |name: &str, kind: Kind| {
                    let e = seen.entry(name.to_string()).or_default();
                    e.0.insert(kind);
                    e.1.insert(run.id, run.created_at.clone());
                };
                for n in &f.flaky {
                    add(n, Kind::Flaky);
                }
                for n in &f.failed {
                    add(n, Kind::Failed);
                }
                for n in &f.vitest_failed {
                    add(n, Kind::VitestFailed);
                }
                if later_pass {
                    for n in f.failed.iter().chain(&f.vitest_failed) {
                        add(n, Kind::FailedThenPassed);
                    }
                }
            }
        }
    }
    if report.never_ran > 0 {
        report.gaps.push(format!(
            "{} failed job(s) have no step timings ({}), so what they would have shown is not measured",
            report.never_ran,
            never_ran_jobs
                .iter()
                .map(|(r, j)| format!("job {j} of run {r}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let runs = report.runs;
    let newest = |m: &BTreeMap<u64, String>| {
        m.values()
            .filter_map(|c| parse_ms(c).map(|t| (t as i64, c.clone())))
            .max()
            .map(|(_, c)| c)
    };
    report.rows = seen
        .into_iter()
        .map(|(name, (kinds, in_runs))| Row {
            name,
            kinds,
            count: in_runs.len(),
            runs,
            last_seen: newest(&in_runs).map(|c| c.chars().take(10).collect()).unwrap_or_default(),
        })
        .collect();
    report.rows.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.last_seen.cmp(&a.last_seen))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(report)
}

/// Whether a later run of the same workflow and commit passed the job of this name (its job list is read once).
fn rerun_passed(
    client: &mut Client,
    passed: &BTreeMap<(&str, &str), Vec<u64>>,
    cache: &mut HashMap<u64, Result<Option<Vec<TimedJob>>, String>>,
    run: &RunInfo,
    job: &str,
    not_measured: &mut Vec<NotMeasured>,
) -> bool {
    let Some(ids) = passed.get(&(run.workflow(), run.head_sha.as_str())) else {
        return false;
    };
    for id in ids.iter().filter(|id| **id > run.id) {
        let jobs = cache.entry(*id).or_insert_with(|| client.attempt_jobs(*id, None));
        match jobs {
            Ok(Some(jobs)) if jobs.iter().any(|j| j.name == job && j.conclusion == "success") => return true,
            Ok(Some(_)) => {}
            Ok(None) => not_measured.push(NotMeasured {
                run: run.id,
                job: format!("\"{job}\""),
                why: format!("whether run {id} passed it could not be read: the server has no job list for it (404)"),
            }),
            Err(e) => not_measured.push(NotMeasured {
                run: run.id,
                job: format!("\"{job}\""),
                why: format!("whether run {id} passed it could not be read: {e}"),
            }),
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::exit_of;
    use crate::ci_runs::testutil::Fake;
    use paddock_cli::Exit;

    // Real captured output of cargo-nextest 0.9.151 and @playwright/test 1.64.1 (see each file's first line).
    const R1_FAIL: &str = include_str!("../../tests/ci-runs/nextest-r1-fail.log");
    const R1_PASS: &str = include_str!("../../tests/ci-runs/nextest-r1-pass.log");
    const R0_PASS: &str = include_str!("../../tests/ci-runs/nextest-r0-pass.log");
    const R0_FAIL: &str = include_str!("../../tests/ci-runs/nextest-r0-fail.log");
    const PLAYWRIGHT: &str = include_str!("../../tests/ci-runs/playwright-project.log");
    const VITEST: &str = include_str!("../../tests/since-run/vitest-frontend.log");
    const CLIPPY: &str = include_str!("../../tests/ci-runs/clippy-failed.log");

    /// 2026-11-10T12:00:00Z.
    const NOW: i64 = 1_794_268_800 + 12 * 3600;
    const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";
    const ONCE: &str = "nxflaky t::flaky_once";
    const ALWAYS: &str = "nxflaky t::always_fails";
    const PW_POPUP: &str = "[chromium] › e2e/tray.spec.js:4:3 › tray › opens the popup";
    const PW_FAILS: &str = "[chromium] › e2e/tray.spec.js:5:3 › tray › always fails";

    fn run(id: u64, path: &str, conclusion: &str, sha: &str, created: &str) -> Value {
        json!({"id": id, "path": format!("{path}@refs/heads/main"), "event": "push", "status": "completed", "conclusion": conclusion,
               "head_sha": sha, "created_at": created, "run_attempt": 1})
    }

    fn job(id: u64, name: &str, conclusion: &str, runner_id: u64) -> Value {
        // A job that never ran has no runner and steps that start and end together.
        let end = if runner_id == 0 {
            "2026-11-09T10:00:00+08:00"
        } else {
            "2026-11-09T10:00:05+08:00"
        };
        json!({"id": id, "name": name, "status": "completed", "conclusion": conclusion, "runner_id": runner_id,
               "steps": [{"name": "Tests", "conclusion": conclusion, "started_at": "2026-11-09T10:00:00+08:00", "completed_at": end}]})
    }

    fn jobs(list: &[(u64, &str, &str)]) -> Value {
        let all: Vec<Value> = list.iter().map(|(id, name, c)| job(*id, name, c, 31)).collect();
        json!({"total_count": all.len(), "jobs": all})
    }

    fn jobs_url(id: u64) -> String {
        format!("{API}/runs/{id}/jobs?limit=50&page=1")
    }

    /// The window of the scenario; the logs are real captured output.
    fn scenario() -> Fake {
        let mut f = Fake::default();
        let mut r109 = run(109, "dyno-release.yml", "", "i", "2026-11-10T09:00:00+08:00");
        r109["status"] = json!("in_progress");
        let mut r112 = run(112, "dyno-release.yml", "success", "iii", "2026-11-01T10:00:00+08:00");
        r112["run_attempt"] = json!(2);
        let runs = vec![
            r109,
            run(101, "dyno-release.yml", "failure", "aaa", "2026-11-09T10:00:00+08:00"),
            run(102, "ci.yml", "failure", "bbb", "2026-11-08T10:00:00+08:00"),
            run(103, "dyno-release.yml", "failure", "ccc", "2026-11-07T10:00:00+08:00"),
            run(104, "dyno-release.yml", "success", "ccc", "2026-11-07T12:00:00+08:00"),
            run(105, "ci.yml", "failure", "ddd", "2026-11-06T10:00:00+08:00"),
            run(106, "dyno-release.yml", "failure", "eee", "2026-11-05T10:00:00+08:00"),
            run(107, "dyno-release.yml", "failure", "fff", "2026-11-04T10:00:00+08:00"),
            run(108, "dyno-release.yml", "cancelled", "ggg", "2026-11-03T10:00:00+08:00"),
            run(111, "dyno-release.yml", "failure", "hhh", "2026-11-02T10:00:00+08:00"),
            r112,
            run(113, "ci.yml", "success", "jjj", "2026-11-01T09:00:00+08:00"),
            run(114, "ci.yml", "success", "kkk", "2026-11-01T08:00:00+08:00"),
            run(110, "dyno-release.yml", "failure", "old", "2026-09-01T10:00:00+08:00"), // before the window
        ];
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 14, "workflow_runs": runs}),
        );
        f.json(
            &jobs_url(101),
            jobs(&[(1011, "test (macos)", "failure"), (1012, "bundle", "success")]),
        );
        f.log(1011, R1_FAIL);
        f.json(
            &jobs_url(102),
            jobs(&[
                (1021, "app dyno (ubuntu-latest, e2e-1)", "failure"),
                (1022, "app pitwall (ubuntu-latest, e2e-1)", "failure"),
                (1023, "crate (macos-latest)", "failure"),
            ]),
        );
        f.log(1021, PLAYWRIGHT);
        f.json(&jobs_url(103), jobs(&[(1031, "test (macos)", "failure")]));
        f.log(1031, R0_PASS);
        f.json(&jobs_url(104), jobs(&[(1041, "test (macos)", "success")]));
        f.json(&jobs_url(105), jobs(&[(1051, "app dyno (ubuntu-latest)", "failure")]));
        f.log(1051, VITEST);
        f.json(&jobs_url(106), jobs(&[(1061, "test (windows)", "failure")]));
        f.fail(1061, "curl exited with 22: The requested URL returned error: 500");
        f.json(&jobs_url(107), jobs(&[(1071, "lint", "failure")]));
        f.log(1071, CLIPPY);
        let mut never = jobs(&[(1111, "test (macos)", "failure")]);
        never["jobs"].as_array_mut().unwrap().push(job(1112, "bundle", "failure", 0));
        f.json(&jobs_url(111), never);
        f.log(1111, R1_PASS);
        // Run 112: a rerun in Gitea is attempt 2 of the same run; attempt 1 failed, attempt 2 passed.
        f.json(
            &format!("{API}/runs/112/attempts/1/jobs?limit=50&page=1"),
            jobs(&[(1121, "test (windows)", "failure")]),
        );
        f.log(1121, R0_FAIL);
        f.json(&jobs_url(112), jobs(&[(1122, "test (windows)", "success")]));
        f.json(
            &jobs_url(113),
            jobs(&[
                (1131, "app dyno (ubuntu-latest)", "success"),
                (1132, "crate (macos-latest)", "success"),
            ]),
        );
        f.json(&jobs_url(114), jobs(&[(1141, "crate (macos-latest)", "success")]));
        f
    }

    fn request(preconditions: Vec<String>) -> Request {
        Request {
            app: "dyno".into(),
            days: 30,
            until: Until::Now,
            repo: "decryp7/paddock".into(),
            now_secs: NOW,
            preconditions,
            config_source: "the working tree".into(),
        }
    }

    fn measure(f: &mut Fake) -> Report {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "s3cr3t");
        c.backoff_ms = 0;
        collect(&mut c, &request(Vec::new()), &mut |_| {}).unwrap()
    }

    fn row<'a>(r: &'a Report, n: &str) -> &'a Row {
        r.rows
            .iter()
            .find(|x| x.name == n)
            .unwrap_or_else(|| panic!("{n} missing in {:?}", r.rows))
    }

    #[test]
    fn real_nextest_output_with_retries_lists_the_hard_failure_as_failed_and_not_flaky() {
        let mut f = scenario();
        let r = measure(&mut f);
        // Run 101 (retries = 1, flaky-result = "fail"): the hard failure ends with `TRY 2 FAIL` and never a bare FAIL.
        assert!(row(&r, ONCE).kinds.contains(&Kind::Flaky));
        assert!(row(&r, ALWAYS).kinds.contains(&Kind::Failed));
        // Run 111 (retries = 1, flaky-result = "pass") is the only other source of `flaky`; ALWAYS never is.
        assert!(!row(&r, ALWAYS).kinds.contains(&Kind::Flaky), "{:?}", row(&r, ALWAYS).kinds);
    }

    #[test]
    fn counts_kinds_and_last_seen_per_test() {
        let mut f = scenario();
        let r = measure(&mut f);
        // ONCE: run 101 (flaky), 103 (failed, no retries), 111 (flaky), 112 (failed on attempt 1).
        let once = row(&r, ONCE);
        assert_eq!((once.count, once.last_seen.as_str()), (4, "2026-11-09"));
        assert_eq!(once.kinds, [Kind::Flaky, Kind::Failed, Kind::FailedThenPassed].into());
        assert_eq!(row(&r, ALWAYS).count, 4);
        let pw = row(&r, PW_POPUP);
        assert_eq!(
            (pw.count, pw.last_seen.as_str(), pw.kinds.clone()),
            (1, "2026-11-08", [Kind::Flaky].into())
        );
        // Failed in run 102 and never passed on that commit: failed, not failed-then-passed.
        assert_eq!(row(&r, PW_FAILS).kinds, [Kind::Failed].into());
        assert_eq!(
            row(&r, "src/app/watched/watched-folders.spec.ts").kinds,
            [Kind::VitestFailed].into()
        );
        assert_eq!(r.rows.len(), 7, "{:?}", r.rows.iter().map(|x| &x.name).collect::<Vec<_>>());
        assert_eq!(r.rows[0].count, 4);
    }

    #[test]
    fn a_failure_that_passed_on_a_later_attempt_of_the_same_run_is_failed_then_passed() {
        let mut f = scenario();
        // Without run 103's rerun and 112's attempts nothing would be failed-then-passed; take 103's rerun away so only the
        // attempts of run 112 can make ALWAYS failed-then-passed.
        f.json(&jobs_url(104), jobs(&[(1041, "test (macos)", "skipped")]));
        let r = measure(&mut f);
        assert!(
            f.requested().iter().any(|u| u.contains("/runs/112/attempts/1/jobs")),
            "{:?}",
            f.requested()
        );
        assert!(row(&r, ALWAYS).kinds.contains(&Kind::FailedThenPassed));
        assert!(f.requested().iter().any(|u| u.contains("/jobs/1121/logs")));
    }

    #[test]
    fn a_same_commit_rerun_counts_only_when_the_job_of_the_same_name_passed() {
        // Run 104 passed on run 103's commit and its `test (macos)` passed: run 103's failures are failed-then-passed. The only
        // other source is run 112, so take 112's attempt away to see 103 alone.
        let mut f = scenario();
        let mut runs = f.take_runs();
        runs.retain(|r| r["id"] != 112);
        f.set_runs(runs);
        let r = measure(&mut f);
        assert!(
            row(&r, ALWAYS).kinds.contains(&Kind::FailedThenPassed),
            "{:?}",
            row(&r, ALWAYS).kinds
        );
        // The same, but run 104's job of that name did not pass.
        let mut f = scenario();
        let mut runs = f.take_runs();
        runs.retain(|r| r["id"] != 112);
        f.set_runs(runs);
        f.json(&jobs_url(104), jobs(&[(1041, "test (macos)", "skipped")]));
        let r = measure(&mut f);
        assert!(
            !row(&r, ALWAYS).kinds.contains(&Kind::FailedThenPassed),
            "{:?}",
            row(&r, ALWAYS).kinds
        );
    }

    #[test]
    fn runs_are_counted_by_the_app_and_the_window_only() {
        let mut f = scenario();
        let r = measure(&mut f);
        // Own finished runs: 101, 103, 104, 106, 107, 111, 112 (108 cancelled, 109 running, 110 too old); shared runs with an
        // app dyno job, failed or passed: 102, 105, 113 (114 has none).
        assert_eq!(r.runs, 10);
        assert!(r.rows.iter().all(|x| x.runs == 10));
        assert_eq!(r.since, "2026-10-11");
        assert_eq!(r.until, "2026-11-10");
        for url in f.requested() {
            assert!(
                !url.contains("/1022/") && !url.contains("/1023/") && !url.contains("/runs/110/") && !url.contains("/runs/108/"),
                "{url}"
            );
        }
    }

    #[test]
    fn a_failed_job_that_never_ran_a_step_is_a_gap_that_makes_the_report_incomplete() {
        let mut f = scenario();
        let r = measure(&mut f);
        assert_eq!(r.never_ran, 1);
        assert!(f.requested().iter().all(|u| !u.contains("/jobs/1112/")), "its log was asked for");
        assert!(
            r.gaps
                .iter()
                .any(|g| g.contains("have no step timings") && g.contains("job 1112 of run 111")),
            "{:?}",
            r.gaps
        );
        assert!(!r.complete());
        assert!(
            r.text().contains("NOT MEASURED: 1 failed job(s) have no step timings"),
            "{}",
            r.text()
        );
    }

    #[test]
    fn a_reusable_workflow_caller_job_is_skipped_not_reported_as_never_ran() {
        // Run 7880's real job list: the failed `installers` caller (34496) has no runner and no steps but real times.
        let mut f = Fake::default();
        let runs = vec![run(7880, "dyno-release.yml", "failure", "abc", "2026-11-09T10:00:00+08:00")];
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 1, "workflow_runs": runs}),
        );
        f.json(
            &jobs_url(7880),
            serde_json::from_str(include_str!("../../tests/ci-runs/run-7880-jobs.json")).unwrap(),
        );
        let r = measure(&mut f);
        assert_eq!(r.never_ran, 0, "{:?}", r.gaps);
        assert!(r.gaps.is_empty(), "{:?}", r.gaps);
        assert!(
            f.requested().iter().all(|u| !u.contains("/jobs/34496/")),
            "the caller's log was asked for"
        );
    }

    #[test]
    fn a_missing_job_list_is_not_measured_for_an_own_run_and_for_an_earlier_attempt() {
        // 404 on the job list of own run 101.
        let mut f = scenario();
        f.remove_json(&jobs_url(101));
        let r = measure(&mut f);
        let n = r.not_measured.iter().find(|n| n.run == 101).expect("run 101 is not measured");
        assert!(n.why.contains("no job list"), "{}", n.why);
        // 404 on attempt 1 of run 112 (the attempt that failed).
        let mut f = scenario();
        f.remove_json(&format!("{API}/runs/112/attempts/1/jobs?limit=50&page=1"));
        let r = measure(&mut f);
        let n = r.not_measured.iter().find(|n| n.run == 112).expect("run 112 is not measured");
        assert!(n.why.contains("no job list") && n.job.contains("attempt 1"), "{n:?}");
        assert!(!r.complete());
    }

    #[test]
    fn a_later_attempt_that_did_not_pass_does_not_make_a_failure_failed_then_passed() {
        let mut f = scenario();
        f.json(&jobs_url(104), jobs(&[(1041, "test (macos)", "skipped")])); // take run 103's rerun away
        f.json(&jobs_url(112), jobs(&[(1122, "test (windows)", "failure")])); // attempt 2 failed too
        f.log(1122, R0_FAIL);
        let r = measure(&mut f);
        assert!(
            !row(&r, ALWAYS).kinds.contains(&Kind::FailedThenPassed),
            "{:?}",
            row(&r, ALWAYS).kinds
        );
    }

    #[test]
    fn a_blank_log_is_not_measured() {
        let mut f = scenario();
        f.log(1031, "  \n\n");
        let r = measure(&mut f);
        let n = r.not_measured.iter().find(|n| n.run == 103).expect("run 103 is not measured");
        assert!(n.why.contains("no log"), "{}", n.why);
    }

    #[test]
    fn cancelled_runs_are_counted_and_said_to_be_unread() {
        let mut f = scenario();
        // Run 115: a cancelled shared run with a job of the app; run 116: one without.
        let mut runs = f.take_runs();
        runs.push(run(115, "ci.yml", "cancelled", "lll", "2026-11-03T09:00:00+08:00"));
        runs.push(run(116, "ci.yml", "cancelled", "mmm", "2026-11-03T08:00:00+08:00"));
        f.set_runs(runs);
        f.json(&jobs_url(115), jobs(&[(1151, "app dyno (ubuntu-latest)", "cancelled")]));
        f.json(&jobs_url(116), jobs(&[(1161, "crate (macos-latest)", "cancelled")]));
        let r = measure(&mut f);
        assert_eq!(r.cancelled_not_read, 2, "runs 108 (own) and 115 (shared, with an app job)");
        assert!(r.text().contains("2 cancelled run(s) not read"), "{}", r.text());
        assert_eq!(r.json()["cancelledNotRead"], 2);
    }

    #[test]
    fn the_exit_code_follows_completeness_first_and_findings_only_on_request() {
        let mut f = scenario();
        let incomplete = measure(&mut f);
        assert!(!incomplete.rows.is_empty() && !incomplete.complete());
        assert_eq!(exit_of(&incomplete, true), Exit::Failure);
        assert_eq!(exit_of(&incomplete, false), Exit::Failure);
        let mut complete = incomplete.clone();
        complete.not_measured.clear();
        complete.gaps.clear();
        assert_eq!(exit_of(&complete, true), Exit::Findings);
        assert_eq!(exit_of(&complete, false), Exit::Ok);
        complete.rows.clear();
        assert_eq!(exit_of(&complete, true), Exit::Ok);
        let line = incomplete.error_line();
        assert!(
            line.starts_with("ci flakes dyno: not measured: ") && line.contains("Next step: "),
            "{line}"
        );
    }

    #[test]
    fn a_failed_job_whose_log_cannot_be_read_is_not_measured_and_the_report_says_why() {
        let mut f = scenario();
        let r = measure(&mut f);
        assert_eq!(r.not_measured.len(), 1);
        assert_eq!(r.not_measured[0].run, 106);
        assert!(r.not_measured[0].why.contains("log could not be read"), "{:?}", r.not_measured);
        assert!(!r.complete());
        let text = r.text();
        assert!(text.contains("NOT MEASURED: run 106"), "{text}");
        assert!(text.contains("not evidence of no flakes"), "{text}");
        assert_eq!(r.json()["complete"], false);
        let e = r.error_line();
        assert!(e.contains("run 106") && e.contains("Next step"), "{e}");
        assert_eq!(r.non_test_failures, 1, "the lint failure is outside the tests");
    }

    #[test]
    fn a_log_the_reader_cannot_account_for_is_a_broken_check_not_a_clean_job() {
        let mut f = scenario();
        // The summary says two failed; the lines that name them are gone.
        let nameless: String = R0_PASS
            .lines()
            .filter(|l| !crate::log_text::plain(l).trim_start().starts_with("FAIL "))
            .map(|l| format!("{l}\n"))
            .collect();
        f.log(1031, &nameless);
        let r = measure(&mut f);
        let n = r.not_measured.iter().find(|n| n.run == 103).expect("run 103 is not measured");
        assert!(n.why.starts_with("check broken:"), "{}", n.why);
    }

    #[test]
    fn a_precondition_that_hides_flakes_in_passing_jobs_makes_the_report_incomplete() {
        let mut f = scenario();
        let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "s3cr3t");
        c.backoff_ms = 0;
        let msg = "Playwright flakes in passing jobs are not visible (no failOnFlakyTests)";
        let r = collect(&mut c, &request(vec![msg.into()]), &mut |_| {}).unwrap();
        assert!(r.text().contains(&format!("NOT MEASURED: {msg}")));
        assert!(!r.complete());
    }

    #[test]
    fn a_window_without_any_finished_run_is_not_measured_rather_than_clean() {
        let mut f = Fake::default();
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 0, "workflow_runs": []}),
        );
        let r = measure(&mut f);
        assert_eq!(r.runs, 0);
        assert!(!r.complete());
        assert!(r.text().contains("NOT MEASURED: no finished run"), "{}", r.text());
    }

    #[test]
    fn a_clean_window_says_so_only_when_every_run_was_measured() {
        let mut f = Fake::default();
        let runs = vec![run(1, "dyno-release.yml", "success", "a", "2026-11-09T10:00:00+08:00")];
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 1, "workflow_runs": runs}),
        );
        let r = measure(&mut f);
        assert!(r.complete());
        assert!(
            r.text().contains("no flaky or failed test found in the measured runs"),
            "{}",
            r.text()
        );
    }

    #[test]
    fn paging_stops_at_the_first_page_that_reaches_the_window_start() {
        let mut f = Fake::default();
        let page = |from: u64, created: &str| {
            let runs: Vec<Value> = (0..50)
                .map(|i| run(from + i, "dyno-release.yml", "success", "s", created))
                .collect();
            json!({"total_count": 500, "workflow_runs": runs})
        };
        f.json(&format!("{API}/runs?limit=50&page=1"), page(1000, "2026-11-09T10:00:00+08:00"));
        let mut p2 = page(2000, "2026-11-08T10:00:00+08:00");
        p2["workflow_runs"][49]["created_at"] = json!("2026-09-01T10:00:00+08:00");
        f.json(&format!("{API}/runs?limit=50&page=2"), p2);
        f.json(&format!("{API}/runs?limit=50&page=3"), page(3000, "2026-09-01T10:00:00+08:00"));
        let r = measure(&mut f);
        assert_eq!(r.runs, 99);
        assert!(r.gaps.is_empty());
        assert!(f.requested().iter().all(|u| !u.contains("page=3")), "{:?}", f.requested());
    }

    #[test]
    fn a_window_longer_than_the_page_cap_is_reported_incomplete() {
        let mut f = Fake::default();
        for p in 1..=MAX_PAGES {
            let runs: Vec<Value> = (0..50)
                .map(|i| {
                    run(
                        u64::from(p) * 100 + i,
                        "dyno-release.yml",
                        "success",
                        "s",
                        "2026-11-09T10:00:00+08:00",
                    )
                })
                .collect();
            f.json(
                &format!("{API}/runs?limit=50&page={p}"),
                json!({"total_count": 99999, "workflow_runs": runs}),
            );
        }
        let r = measure(&mut f);
        assert!(r.gaps.iter().any(|g| g.contains("more than 60 pages")), "{:?}", r.gaps);
        assert!(!r.complete());
    }

    #[test]
    fn until_a_date_or_a_commit_moves_the_window_for_the_tag_gate() {
        let mut f = scenario();
        let mut c = Client::new(&mut f, "https://gitea.invalid", "decryp7/paddock", "s3cr3t");
        c.backoff_ms = 0;
        // Until the end of 2026-11-05 (UTC), three days: runs 106 (02:00 UTC on the 5th) and 107 (the 4th).
        let mut req = request(Vec::new());
        req.until = Until::parse("2026-11-05").unwrap();
        req.days = 3;
        let r = collect(&mut c, &req, &mut |_| {}).unwrap();
        assert_eq!((r.since.as_str(), r.until.as_str()), ("2026-11-03", "2026-11-05"));
        assert_eq!(r.runs, 2, "runs 106 and 107 only");
        // Until a commit: the window ends at that commit's newest run (104, 2026-11-07 04:00 UTC); one day back: 103 and 104.
        req.until = Until::Commit("ccc".into());
        req.days = 1;
        let r = collect(&mut c, &req, &mut |_| {}).unwrap();
        assert_eq!(r.runs, 2, "runs 103 and 104");
        req.until = Until::Commit("deadbeef".into());
        assert!(
            collect(&mut c, &req, &mut |_| {})
                .unwrap_err()
                .contains("no run of a commit starting with deadbeef")
        );
        assert!(Until::parse("tomorrow").is_err());
        assert_eq!(Until::parse("2026-11-05").unwrap(), Until::Date("2026-11-05".into()));
        assert_eq!(Until::parse("67A633EF3").unwrap(), Until::Commit("67a633ef3".into()));
    }

    #[test]
    fn the_token_is_never_in_the_text_or_the_json() {
        let mut f = scenario();
        let r = measure(&mut f);
        assert!(!r.text().contains("s3cr3t") && !r.json().to_string().contains("s3cr3t"));
    }
}
