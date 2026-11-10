//! `ci flakes`: the tests CI marked flaky or saw fail and then pass, over a window of runs (ADR 0082 D6).
//!
//! What counts, per test and per run (a run counts once however many jobs or attempts show the test):
//!
//! - **flaky**: nextest marked it `FLAKY` (`flaky-result = "fail"` makes such a run fail, ER-12) or it failed an attempt and
//!   ended passed; Playwright listed it under `N flaky` (`failOnFlakyTests`);
//! - **vitest-failed**: vitest has no flaky marker, so every failed spec file of a failed run counts;
//! - **failed-then-passed**: a test that failed in a run whose workflow later passed on the same commit (a rerun).
//!
//! Only the logs of jobs that did not pass are read: with flaky-as-failure a flaky test fails its job. A failed job whose log
//! cannot be read is **not measured**, never "no flakes" (ER-33), and the report then says it is incomplete.

use super::client::{Client, RunInfo};
use super::logfind;
use super::time::{date_of, parse_ms};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// The most pages of the run list read (50 runs a page); a window that needs more is reported incomplete.
pub const MAX_PAGES: u32 = 60;

/// How a test got on the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Marked flaky by the runner.
    Flaky,
    /// A failed vitest spec file.
    VitestFailed,
    /// Failed, then the same commit passed on a rerun.
    FailedThenPassed,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Flaky => "flaky",
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
    /// The runs examined in the window (the same for every test).
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
    /// The finished runs examined.
    pub runs: usize,
    /// Failed jobs whose log was read.
    pub logs_read: usize,
    /// The listed tests, most often first.
    pub rows: Vec<Row>,
    /// Failed jobs that failed outside any test runner (a build, a lint).
    pub non_test_failures: usize,
    /// What could not be measured.
    pub not_measured: Vec<NotMeasured>,
    /// Why the window itself could not be listed completely.
    pub incomplete: Option<String>,
}

impl Report {
    /// Whether every run of the window was measured.
    pub fn complete(&self) -> bool {
        self.not_measured.is_empty() && self.incomplete.is_none() && self.runs > 0
    }

    /// The report as text for people.
    pub fn text(&self) -> String {
        let mut l = vec![format!(
            "{}: flaky and failed-then-passed tests since {} ({} days, {}); {} finished runs, {} failed job logs read",
            self.app, self.since, self.days, self.repo, self.runs, self.logs_read
        )];
        if self.rows.is_empty() {
            l.push(if self.complete() {
                "no flaky test found in the measured runs".into()
            } else {
                "no flaky test found in the runs that could be measured (see below)".into()
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
        if self.runs == 0 {
            l.push("NOT MEASURED: no finished run of this app in the window, so this says nothing about flakes".into());
        }
        if let Some(why) = &self.incomplete {
            l.push(format!("NOT MEASURED: {why}"));
        }
        for n in &self.not_measured {
            l.push(format!("NOT MEASURED: run {} job {}: {}", n.run, n.job, n.why));
        }
        if !self.complete() {
            l.push("The list is incomplete: a gap above is not evidence of no flakes (ER-33).".into());
        }
        l.join("\n")
    }

    /// The report as JSON.
    pub fn json(&self) -> Value {
        json!({
            "app": self.app,
            "repo": self.repo,
            "days": self.days,
            "since": self.since,
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
            "notMeasured": self.not_measured.iter().map(|n| json!({"run": n.run, "job": n.job, "why": n.why})).collect::<Vec<_>>(),
            "incomplete": self.incomplete,
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

/// The runs of the window, newest first, and why the listing stopped short (`None` when it reached the window's start).
fn list_window(client: &mut Client, cutoff_ms: f64) -> Result<(Vec<RunInfo>, Option<String>), String> {
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        let p = client.runs_page(page)?;
        let n = p.runs.len();
        let mut reached = false;
        for r in p.runs {
            match parse_ms(&r.created_at) {
                Some(t) if t < cutoff_ms => reached = true,
                Some(_) => out.push(r),
                None => {
                    return Ok((
                        out,
                        Some(format!(
                            "run {} has a creation time that cannot be read (\"{}\")",
                            r.id, r.created_at
                        )),
                    ));
                }
            }
        }
        if reached || n < 50 {
            return Ok((out, None));
        }
    }
    Ok((out, Some(format!("the window needs more than {MAX_PAGES} pages of runs"))))
}

/// Measures the window: lists the runs, reads the logs of the app's failed jobs and counts the tests.
pub fn collect(client: &mut Client, app: &str, days: u32, now_secs: i64, repo: &str, log: &mut dyn FnMut(&str)) -> Result<Report, String> {
    let cutoff = now_secs - i64::from(days) * 86_400;
    let (listed, incomplete) = list_window(client, cutoff as f64 * 1000.0)?;
    let finished: Vec<&RunInfo> = listed
        .iter()
        .filter(|r| r.finished() && matches!(r.conclusion.as_str(), "success" | "failure"))
        .collect();
    // A workflow that passed on a commit after (or besides) a failed run of it: the failure was a flake or fixed by a rerun.
    let passed: BTreeSet<(&str, &str)> = finished
        .iter()
        .filter(|r| r.conclusion == "success")
        .map(|r| (r.workflow(), r.head_sha.as_str()))
        .collect();
    let mut report = Report {
        app: app.to_string(),
        repo: repo.to_string(),
        days,
        since: date_of(cutoff),
        runs: 0,
        logs_read: 0,
        rows: Vec::new(),
        non_test_failures: 0,
        not_measured: Vec::new(),
        incomplete,
    };
    // name -> (kinds, run id -> created_at)
    let mut seen: BTreeMap<String, (BTreeSet<Kind>, BTreeMap<u64, String>)> = BTreeMap::new();
    log(&format!("{} runs listed, {} finished", listed.len(), finished.len()));
    for run in finished {
        let own = own_workflow(app, run.workflow());
        if run.conclusion == "success" {
            if own {
                report.runs += 1;
            }
            continue;
        }
        let jobs = match client.jobs(run.id) {
            Ok(j) => j,
            Err(e) => {
                if own {
                    report.runs += 1;
                }
                report.not_measured.push(NotMeasured {
                    run: run.id,
                    job: "(the job list)".into(),
                    why: format!("the jobs could not be read: {e}"),
                });
                continue;
            }
        };
        let mine: Vec<_> = jobs.iter().filter(|j| own || app_job(app, &j.name)).collect();
        if mine.is_empty() {
            continue; // a shared workflow's run with no job of this app
        }
        report.runs += 1;
        let rerun_passed = passed.contains(&(run.workflow(), run.head_sha.as_str()));
        for job in mine.into_iter().filter(|j| j.failed()) {
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
            log(&format!(
                "run {} job {}: {} flaky, {} failed, {} vitest spec(s) failed",
                run.id,
                job.id,
                f.flaky.len(),
                f.failed.len(),
                f.vitest_failed.len()
            ));
            if !f.any() {
                report.non_test_failures += 1;
                continue;
            }
            let mut add = |name: &str, kind: Kind| {
                let e = seen.entry(name.to_string()).or_default();
                e.0.insert(kind);
                e.1.insert(run.id, run.created_at.clone());
            };
            for n in &f.flaky {
                add(n, Kind::Flaky);
            }
            for n in &f.vitest_failed {
                add(n, Kind::VitestFailed);
            }
            if rerun_passed {
                for n in f.failed.iter().chain(&f.vitest_failed) {
                    add(n, Kind::FailedThenPassed);
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_runs::testutil::Fake;

    const NEXTEST_FLAKY: &str = include_str!("../../tests/ci-runs/nextest-flaky.log");
    const NEXTEST_HARD: &str = include_str!("../../tests/ci-runs/nextest-hardfail.log");
    const PLAYWRIGHT: &str = include_str!("../../tests/ci-runs/playwright-flaky.log");
    const VITEST: &str = include_str!("../../tests/ci-runs/vitest-failed.log");
    const CLIPPY: &str = include_str!("../../tests/ci-runs/clippy-failed.log");

    /// 2026-11-10T12:00:00Z.
    const NOW: i64 = 1_794_268_800 + 12 * 3600;
    const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";

    fn run(id: u64, path: &str, conclusion: &str, sha: &str, created: &str) -> Value {
        json!({"id": id, "path": format!("{path}@refs/heads/main"), "event": "push", "status": "completed", "conclusion": conclusion,
               "head_sha": sha, "created_at": created})
    }

    fn jobs(list: &[(u64, &str, &str)]) -> Value {
        json!({"total_count": list.len(), "jobs": list.iter().map(|(id, name, c)| json!({"id": id, "name": name, "status": "completed", "conclusion": c,
            "steps": [{"name": "Tests", "conclusion": c}]})).collect::<Vec<_>>()})
    }

    /// The window of the scenario: eleven runs, of which the app's failed ones carry the logs above.
    fn scenario() -> Fake {
        let mut f = Fake::default();
        let runs = vec![
            run(109, "dyno-release.yml", "", "i", "2026-11-10T09:00:00+08:00"), // in progress (status below)
            run(101, "dyno-release.yml", "failure", "aaa", "2026-11-09T10:00:00+08:00"),
            run(102, "ci.yml", "failure", "bbb", "2026-11-08T10:00:00+08:00"),
            run(103, "dyno-release.yml", "failure", "ccc", "2026-11-07T10:00:00+08:00"),
            run(104, "dyno-release.yml", "success", "ccc", "2026-11-07T12:00:00+08:00"),
            run(105, "ci.yml", "failure", "ddd", "2026-11-06T10:00:00+08:00"),
            run(106, "dyno-release.yml", "failure", "eee", "2026-11-05T10:00:00+08:00"),
            run(107, "dyno-release.yml", "failure", "fff", "2026-11-04T10:00:00+08:00"),
            run(108, "dyno-release.yml", "cancelled", "ggg", "2026-11-03T10:00:00+08:00"),
            run(111, "dyno-release.yml", "failure", "hhh", "2026-11-02T10:00:00+08:00"),
            run(110, "dyno-release.yml", "failure", "old", "2026-09-01T10:00:00+08:00"), // before the window
        ];
        let mut runs = runs;
        runs[0]["status"] = json!("in_progress");
        f.json(
            &format!("{API}/runs?limit=50&page=1"),
            json!({"total_count": 11, "workflow_runs": runs}),
        );
        f.json(
            &format!("{API}/runs/101/jobs?limit=50&page=1"),
            jobs(&[(1011, "test (macos)", "failure"), (1012, "bundle", "success")]),
        );
        f.log(1011, NEXTEST_FLAKY);
        f.json(
            &format!("{API}/runs/102/jobs?limit=50&page=1"),
            jobs(&[
                (1021, "app dyno (ubuntu-latest, e2e-1)", "failure"),
                (1022, "app pitwall (ubuntu-latest, e2e-1)", "failure"),
                (1023, "crate (macos-latest)", "failure"),
            ]),
        );
        f.log(1021, PLAYWRIGHT);
        f.json(
            &format!("{API}/runs/103/jobs?limit=50&page=1"),
            jobs(&[(1031, "test (macos)", "failure")]),
        );
        f.log(1031, NEXTEST_HARD);
        f.json(
            &format!("{API}/runs/105/jobs?limit=50&page=1"),
            jobs(&[(1051, "app dyno (ubuntu-latest)", "failure")]),
        );
        f.log(1051, VITEST);
        f.json(
            &format!("{API}/runs/106/jobs?limit=50&page=1"),
            jobs(&[(1061, "test (windows)", "failure")]),
        );
        f.fail(1061, "curl exited with 22: The requested URL returned error: 500");
        f.json(&format!("{API}/runs/107/jobs?limit=50&page=1"), jobs(&[(1071, "lint", "failure")]));
        f.log(1071, CLIPPY);
        f.json(
            &format!("{API}/runs/111/jobs?limit=50&page=1"),
            jobs(&[(1111, "test (macos)", "failure")]),
        );
        f.log(1111, NEXTEST_FLAKY);
        f
    }

    fn measure(f: &mut Fake) -> Report {
        let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "s3cr3t");
        c.backoff_ms = 0;
        collect(&mut c, "dyno", 30, NOW, "decryp7/paddock", &mut |_| {}).unwrap()
    }

    #[test]
    fn the_window_lists_flaky_vitest_failed_and_failed_then_passed_tests_with_counts_and_last_seen() {
        let mut f = scenario();
        let r = measure(&mut f);
        let by_name = |n: &str| {
            r.rows
                .iter()
                .find(|x| x.name == n)
                .unwrap_or_else(|| panic!("{n} missing in {:?}", r.rows))
        };
        let flaky = by_name("dyno-app link::tuning::the_agent_follows_the_service");
        assert_eq!((flaky.count, flaky.last_seen.as_str()), (2, "2026-11-09"));
        assert_eq!(flaky.kinds, [Kind::Flaky].into());
        assert_eq!(by_name("dyno-app power::tests::a_pending_action_survives_a_restart").count, 2);
        let pw = by_name("[chromium] › e2e/tray.spec.ts:21:5 › tray › opens the popup");
        assert_eq!((pw.count, pw.last_seen.as_str()), (1, "2026-11-08"));
        let reran = by_name("dyno-app a::b::c");
        assert_eq!(reran.kinds, [Kind::FailedThenPassed].into());
        assert_eq!(reran.last_seen, "2026-11-07");
        let v = by_name("src/app/watched/watched-folders.spec.ts");
        assert_eq!(v.kinds, [Kind::VitestFailed].into());
        // The Playwright test that failed in run 102 never passed on that commit: not listed.
        assert!(r.rows.iter().all(|x| !x.name.contains("disks.spec")), "{:?}", r.rows);
        assert_eq!(r.rows.len(), 5);
        // The most frequent first.
        assert_eq!(r.rows[0].count, 2);
    }

    #[test]
    fn runs_are_counted_by_the_app_and_the_window_only() {
        let mut f = scenario();
        let r = measure(&mut f);
        // Finished runs of dyno-release.yml in the window: 101, 103, 104, 106, 107, 111 (108 cancelled, 109 running, 110 too old);
        // ci.yml runs 102 and 105 have app dyno jobs.
        assert_eq!(r.runs, 8);
        assert!(r.rows.iter().all(|x| x.runs == 8));
        assert_eq!(r.since, "2026-10-11");
        // Other apps' and other crates' jobs, a run outside the window and a cancelled run are never fetched.
        for url in f.requested() {
            assert!(
                !url.contains("/1022/") && !url.contains("/1023/") && !url.contains("/runs/110/") && !url.contains("/runs/108/"),
                "{url}"
            );
        }
    }

    #[test]
    fn a_failed_job_whose_log_cannot_be_read_is_not_measured_and_makes_the_report_incomplete() {
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
        // A lint failure is a failure outside the tests, not a flake and not unmeasured.
        assert_eq!(r.non_test_failures, 1);
        assert_eq!(r.logs_read, 6);
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
        assert!(r.text().contains("no flaky test found in the measured runs"), "{}", r.text());
    }

    #[test]
    fn paging_stops_at_the_first_page_that_reaches_the_window_start() {
        let mut f = Fake::default();
        let page = |from: u64, created: &str| json!({"total_count": 500, "workflow_runs": (0..50).map(|i| run(from + i, "dyno-release.yml", "success", "s", created)).collect::<Vec<_>>()});
        f.json(&format!("{API}/runs?limit=50&page=1"), page(1000, "2026-11-09T10:00:00+08:00"));
        let mut p2 = page(2000, "2026-11-08T10:00:00+08:00");
        p2["workflow_runs"][49]["created_at"] = json!("2026-09-01T10:00:00+08:00");
        f.json(&format!("{API}/runs?limit=50&page=2"), p2);
        f.json(&format!("{API}/runs?limit=50&page=3"), page(3000, "2026-09-01T10:00:00+08:00"));
        let r = measure(&mut f);
        assert_eq!(r.runs, 99);
        assert!(r.incomplete.is_none());
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
        assert!(
            r.incomplete.as_deref().unwrap_or("").contains("more than 60 pages"),
            "{:?}",
            r.incomplete
        );
        assert!(!r.complete());
    }

    #[test]
    fn the_token_is_never_in_the_text_or_the_json() {
        let mut f = scenario();
        let r = measure(&mut f);
        assert!(!r.text().contains("s3cr3t") && !r.json().to_string().contains("s3cr3t"));
    }
}
