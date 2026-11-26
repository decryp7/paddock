// ci-runs-reader: the tests of the one definition of green, over the fake runs API
//! The one definition of green (ADR 0117 WP-4), over the fake server of `testutil`.
//!
//! Provenance of the fixtures (ER-14: scrubbed of hostnames, runner names and paths):
//! - `tests/ci-runs/run-8610-jobs.json`: the jobs of train 19's landing run 8610 (the plan job, conformance, and the two failed jobs
//!   `crate (windows-latest)` and `app spotter (macos-latest)`), read through the Actions API on 2026-11-23; server and repository
//!   renamed, runner names replaced. The records have the API's own keys, with each step's `name`, `number` and `conclusion`.
//! - `nextest-run8610-crate-flaky-only.log` and `nextest-run8610-app-gates-flaky-only.log`: excerpts of those two jobs' logs (the
//!   header of each file says what was left out). Each shows one FLAKY test, `FLKY-FL 2/2`, and nothing else; the app log carries
//!   app-gates' real `::error::failed (exit 100): cargo nextest run` and `##[error]` lines.
//! - `nextest-r1-pass.log`: a test that failed both tries beside a FLAKY one (cargo-nextest 0.9.149).
//! - `tests/ci-runs/run-9001-jobs.json` and `nextest-run9001-crates-coverage-flaky-only.log`: the crates job of train 2026-11-26-c's
//!   run 9001 (its coverage step, `cargo llvm-cov nextest`, failed on one FLAKY paddock-ipc test) and an excerpt of its log, read
//!   through the Actions API on 2026-11-25 and scrubbed like the 8610 ones (the header of the log says what was left out).
//! - the plan logs are `run_scope::TEST_COMPLETE`, `TEST_PARTIAL` and `TEST_OS_ONLY` (the line `./dev plan --ci` prints), with the
//!   old `plan verdict: complete=true` line beside it where a test asks for the report-only phase.

use super::*;
use crate::ci_runs::testutil::Fake;
use crate::run_scope;
use serde_json::{Value, json};

const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";
const JOBS_8610: &str = include_str!("../../../tests/ci-runs/run-8610-jobs.json");
const CRATE_FLAKY: &str = include_str!("../../../tests/ci-runs/nextest-run8610-crate-flaky-only.log");
const APP_FLAKY: &str = include_str!("../../../tests/ci-runs/nextest-run8610-app-gates-flaky-only.log");
const BOTH_TRIES: &str = include_str!("../../../tests/ci-runs/nextest-r1-pass.log");
const JOBS_9001: &str = include_str!("../../../tests/ci-runs/run-9001-jobs.json");
const CRATES_COVERAGE_FLAKY: &str = include_str!("../../../tests/ci-runs/nextest-run9001-crates-coverage-flaky-only.log");
const CRATES: &str = "crates (fmt, workspace hack, unused deps, coverage, deny)";
const CRATE: &str = "crate (windows-latest)";
const APP: &str = "app spotter (macos-latest)";

fn sha(n: u32) -> String {
    format!("{n:040x}")
}

/// The plan log of a run that declares a complete scope and, as the report-only phase still reads, the old line.
fn complete() -> String {
    format!("{}plan verdict: complete=true\n", run_scope::TEST_COMPLETE)
}

/// A job of the real run 8610 by name, given another id and page.
fn real_job(name: &str, run: u64, n: u64) -> Value {
    real_job_in(JOBS_8610, name, run, n)
}

/// A job of a recorded run's jobs (`fixture`) by name, given another id and page.
fn real_job_in(fixture: &str, name: &str, run: u64, n: u64) -> Value {
    let all: Value = serde_json::from_str(fixture).unwrap();
    let mut job = all["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["name"].as_str().is_some_and(|x| x == name || x.starts_with(&format!("{name} "))))
        .unwrap_or_else(|| panic!("no job {name} in the fixture"))
        .clone();
    let id = run * 100 + n;
    job["id"] = json!(id);
    job["html_url"] = json!(format!("https://gitea.invalid/example/paddock/actions/runs/{run}/jobs/{id}"));
    job
}

/// A run of the fake server: the run record, its jobs (the real shape) and their logs.
struct Run {
    id: u64,
    sha: String,
    event: &'static str,
    branch: &'static str,
    conclusion: &'static str,
    status: &'static str,
    attempt: u64,
    plan: String,
    jobs: Vec<Value>,
    logs: Vec<(u64, String)>,
    /// Jobs of earlier attempts, by attempt number.
    earlier: Vec<(u64, Vec<Value>)>,
}

impl Run {
    fn train(id: u64, sha: &str, conclusion: &'static str) -> Run {
        Run {
            id,
            sha: sha.to_string(),
            event: "workflow_dispatch",
            branch: "train/2026-11-21-19",
            conclusion,
            status: "completed",
            attempt: 1,
            plan: complete(),
            jobs: vec![real_job("plan", id, 0), real_job("conformance", id, 1)],
            logs: Vec::new(),
            earlier: Vec::new(),
        }
    }
    fn push(id: u64, sha: &str, conclusion: &'static str) -> Run {
        Run {
            event: "push",
            branch: "main",
            ..Run::train(id, sha, conclusion)
        }
    }
    /// The real failed job `name` with the log it carries.
    fn failing(self, name: &str, log: &str) -> Run {
        self.failing_in(JOBS_8610, name, log)
    }
    /// The failed job `name` of the recorded jobs `fixture`, with the log it carries.
    fn failing_in(mut self, fixture: &str, name: &str, log: &str) -> Run {
        let job = real_job_in(fixture, name, self.id, 2 + self.jobs.len() as u64);
        self.logs.push((job["id"].as_u64().unwrap(), log.to_string()));
        self.jobs.push(job);
        self
    }
    /// Edits the job `name` of this run.
    fn edit(mut self, name: &str, f: impl FnOnce(&mut Value)) -> Run {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j["name"].as_str().unwrap().starts_with(name))
            .expect("the job");
        f(job);
        self
    }
    fn partial(mut self, plan: &str) -> Run {
        self.plan = plan.to_string();
        self
    }
    fn json(&self) -> Value {
        json!({"id": self.id, "path": format!("ci.yml@refs/heads/{}", self.branch), "event": self.event, "head_branch": self.branch,
               "head_sha": self.sha, "status": self.status, "conclusion": self.conclusion, "run_attempt": self.attempt,
               "created_at": "2026-11-22T00:00:00Z"})
    }
    fn info(&self) -> RunInfo {
        RunInfo {
            id: self.id,
            path: format!("ci.yml@refs/heads/{}", self.branch),
            event: self.event.into(),
            status: self.status.into(),
            conclusion: self.conclusion.into(),
            head_sha: self.sha.clone(),
            head_branch: self.branch.into(),
            created_at: "2026-11-22T00:00:00Z".into(),
            run_attempt: self.attempt,
            title: String::new(),
        }
    }
}

/// The server: the run list in pages of 50 (newest first, as given), and each run's jobs and logs.
fn server(runs: &[Run]) -> Fake {
    let mut f = Fake::default();
    for (i, chunk) in runs.chunks(50).enumerate() {
        f.json(
            &format!("{API}/runs?limit=50&page={}", i + 1),
            json!({"total_count": runs.len(), "workflow_runs": chunk.iter().map(Run::json).collect::<Vec<_>>()}),
        );
    }
    for r in runs {
        f.json(
            &format!("{API}/runs/{}/jobs?limit=50&page=1", r.id),
            json!({"total_count": r.jobs.len(), "jobs": r.jobs}),
        );
        for (n, jobs) in &r.earlier {
            f.json(
                &format!("{API}/runs/{}/attempts/{n}/jobs?limit=50&page=1", r.id),
                json!({"total_count": jobs.len(), "jobs": jobs}),
            );
        }
        f.log(r.jobs[0]["id"].as_u64().unwrap(), &r.plan);
        for (id, log) in &r.logs {
            f.log(*id, log);
        }
    }
    f
}

/// The plan reached these apps.
fn touched(apps: &[&str]) -> Touched {
    Touched {
        apps: apps.iter().map(|a| a.to_string()).collect(),
        ..Touched::default()
    }
}

/// The plan reached every crate (the strict interim reading: a FLAKY test of a reached crate is red).
fn reached() -> Touched {
    Touched {
        flags: ["crates".to_string()].into(),
        ..Touched::default()
    }
}

fn client(f: &mut Fake) -> Client<'_> {
    let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
    c.backoff_ms = 0;
    c
}

fn base(f: &mut Fake, first_parent: &[String], t: Touched) -> Option<Base> {
    find_base(&mut client(f), "ci.yml", first_parent, true, &mut |_, _| Some(t.clone())).unwrap()
}

/// The outcome of a run on the basis that decides (`decisive`: the declared scope, else the old line, as in production today).
fn grade_on(f: &mut Fake, r: &Run, t: Touched, decisive: bool) -> Outcome {
    grade_run(&mut client(f), &r.info(), decisive, &mut |_| Some(t.clone()))
}

fn grade(f: &mut Fake, r: &Run, t: Touched) -> Outcome {
    grade_on(f, r, t, true)
}

/// The runs are moved into the server: a copy of one, made from its parts.
fn clone_run(r: &Run) -> Run {
    Run {
        id: r.id,
        sha: r.sha.clone(),
        event: r.event,
        branch: r.branch,
        conclusion: r.conclusion,
        status: r.status,
        attempt: r.attempt,
        plan: r.plan.clone(),
        jobs: r.jobs.clone(),
        logs: r.logs.clone(),
        earlier: r.earlier.clone(),
    }
}

fn older_clone(r: &Run) -> Run {
    clone_run(r)
}

fn fix_items(o: &Outcome) -> &[FixItem] {
    match &o.result {
        GradedResult::GreenWithFixItems(items) => items,
        other => panic!("not green with fix items: {other:?}"),
    }
}

#[test]
fn a_landing_run_with_flaky_only_failures_in_untouched_targets_is_complete_green_with_fix_items_and_the_base() {
    // Train 19's 8610 shape, from its real job records and log: the landing run is red, every failed job is a FLAKY test, and the
    // change reached other targets.
    let c = sha(2);
    let mut f = server(&[Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY)]);
    let b = base(&mut f, std::slice::from_ref(&c), touched(&["debrief"])).expect("a base");
    assert_eq!((b.sha.as_str(), b.run), (c.as_str(), 8610));
    assert_eq!(b.outcome.scope, Scope::Complete);
    let items = fix_items(&b.outcome);
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].job.as_str(), items[0].job_id), (CRATE, 861004));
    assert!(
        items[0].test.contains("a_browser_ended_by_force_leaves_nothing_behind"),
        "{items:?}"
    );
    assert!(items[0].url.ends_with("/actions/runs/8610/jobs/861004"), "{items:?}");
    assert!(b.outcome.is_base() && !b.outcome.is_tag_green(), "a tag needs plain green");
}

#[test]
fn the_base_is_found_among_first_parent_commits_whatever_their_push_run_concluded() {
    // The 8554/8555 shape: the train's run 8554 is green; the pushes after it were cancelled (8341) or failed with a FLAKY only (8555);
    // a push that reused a run declared less. The newest commit has only a cancelled push and a reusing one.
    let (newest, landed, older) = (sha(3), sha(2), sha(1));
    let runs = [
        Run::push(8556, &newest, "success").partial(run_scope::TEST_PARTIAL),
        Run::push(8555, &landed, "failure").failing(CRATE, CRATE_FLAKY),
        Run::push(8341, &newest, "cancelled"),
        Run::train(8554, &landed, "success"),
        Run::push(8045, &older, "success"),
    ];
    let mut f = server(&runs);
    let b = base(&mut f, &[newest, landed.clone(), older], touched(&["debrief"])).expect("a base");
    assert_eq!(
        b.sha, landed,
        "the cancelled and reusing pushes are no evidence; the old green 8045 is not the base"
    );
    assert_eq!(
        b.run, 8555,
        "the newest complete run of the commit decides, and its FLAKY-only failure is fix items"
    );
    assert!(matches!(b.outcome.result, GradedResult::GreenWithFixItems(_)));
}

#[test]
fn a_flaky_test_in_a_crate_or_app_the_plan_reached_is_red_interim_strict() {
    // User decision 2026-11-23: any crate or app the plan reached makes a FLAKY red, until ADR 0117's module edges land.
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
    let mut f = server(std::slice::from_ref(&r));
    assert_eq!(base(&mut f, std::slice::from_ref(&c), reached()), None);
    let o = grade(&mut f, &r, reached());
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("reached")), "{o:?}");
    // The same log is fix items when the plan reached something else; an app job likewise by its app.
    assert!(matches!(
        grade(&mut f, &r, touched(&["debrief"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
    let app = Run::train(8611, &c, "failure").failing(APP, APP_FLAKY);
    let mut f = server(std::slice::from_ref(&app));
    assert!(matches!(grade(&mut f, &app, touched(&["spotter"])).result, GradedResult::Red(_)));
    assert!(matches!(
        grade(&mut f, &app, touched(&["dyno"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
}

#[test]
fn a_test_that_failed_both_tries_is_red_even_in_an_untouched_target() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, &format!("Job: crate\n{BOTH_TRIES}"));
    let mut f = server(std::slice::from_ref(&r));
    assert_eq!(base(&mut f, std::slice::from_ref(&c), touched(&["debrief"])), None);
    let o = grade(&mut f, &r, touched(&["debrief"]));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("both tries")), "{o:?}");
}

#[test]
fn a_step_that_is_not_a_test_failing_is_red() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, "Job: crate\nerror: could not compile `paddock-x`\n");
    let mut f = server(std::slice::from_ref(&r));
    assert_eq!(base(&mut f, std::slice::from_ref(&c), touched(&["debrief"])), None);
    let o = grade(&mut f, &r, touched(&["debrief"]));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("not a test")), "{o:?}");
}

#[test]
fn a_flaky_job_with_another_failed_step_is_red_and_one_with_no_listed_failed_step_is_not_measured() {
    // ER-33: the log shows a FLAKY test, but the API lists a second failed step in the same job (here the real job with a lint step
    // failed too, or with no failed step at all).
    let c = sha(2);
    let with_steps = |steps: &[(&str, &str)]| {
        let steps: Vec<Value> = steps
            .iter()
            .enumerate()
            .map(|(i, (n, c))| json!({"name": n, "number": i, "status": "completed", "conclusion": c}))
            .collect();
        Run::train(8610, &c, "failure")
            .failing(CRATE, CRATE_FLAKY)
            .edit(CRATE, |j| j["steps"] = json!(steps))
    };
    let test = "Tests (workspace, every feature)";
    let both = with_steps(&[(test, "failure"), ("Clippy", "failure")]);
    let mut f = server(std::slice::from_ref(&both));
    let o = grade(&mut f, &both, touched(&["debrief"]));
    assert!(
        matches!(&o.result, GradedResult::Red(why) if why.contains("Clippy") && why.contains("not a test step")),
        "{o:?}"
    );
    let ok = with_steps(&[("Clippy", "success"), (test, "failure")]);
    let mut f = server(std::slice::from_ref(&ok));
    assert!(matches!(
        grade(&mut f, &ok, touched(&["debrief"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
    let none = with_steps(&[(test, "success")]);
    let mut f = server(std::slice::from_ref(&none));
    assert!(matches!(
        grade(&mut f, &none, touched(&["debrief"])).result,
        GradedResult::NotMeasured(_)
    ));
}

#[test]
fn a_flaky_app_job_with_a_lint_annotation_inside_app_gates_is_red_and_an_unreadable_composite_failure_is_not_measured() {
    // ER-44: app-gates is one step in the API; which command failed inside it is only in the `::error::failed` annotations.
    let c = sha(2);
    let grade_log = |log: &str| {
        let r = Run::train(8610, &c, "failure").failing(APP, log);
        let mut f = server(std::slice::from_ref(&r));
        grade(&mut f, &r, touched(&["dyno"]))
    };
    // Control: the real capture, whose one failed command is the nextest run and whose app is not reached: fix items.
    assert!(matches!(grade_log(APP_FLAKY).result, GradedResult::GreenWithFixItems(_)));
    // A lint annotation (the real line with its command replaced) is red, though the log also shows only FLAKY tests.
    let lint = APP_FLAKY.replace(
        "cargo nextest run --workspace --profile ci",
        "cargo clippy --workspace --all-targets -- -D warnings",
    );
    assert_ne!(lint, APP_FLAKY);
    let o = grade_log(&lint);
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("clippy")), "{o:?}");
    // A composite failure that does not show the failed command and the composite is not measured: one `##[error]` line less.
    let one: String = APP_FLAKY
        .lines()
        .take(APP_FLAKY.lines().count() - 1)
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(matches!(grade_log(&one).result, GradedResult::NotMeasured(_)));
}

#[test]
fn a_step_skipped_because_of_a_failure_makes_the_run_partial_not_green_on_either_basis() {
    // The declared job (conformance) was skipped after another job failed: the run did less than it declared. In the report-only
    // phase the old line (complete=true) still says complete, but fix items need the declared scope to have run (P1-1).
    let c = sha(2);
    let skipped = Run::train(8610, &c, "failure")
        .failing(CRATE, CRATE_FLAKY)
        .edit("conformance", |j| j["conclusion"] = json!("skipped"));
    let ok = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
    for decisive in [true, false] {
        let mut f = server(std::slice::from_ref(&skipped));
        let o = grade_on(&mut f, &skipped, touched(&["debrief"]), decisive);
        assert!(!o.is_base(), "decisive={decisive}: {o:?}");
        assert!(
            !matches!(o.result, GradedResult::GreenWithFixItems(_)),
            "decisive={decisive}: {o:?}"
        );
        assert!(o.scope == Scope::Partial, "decisive={decisive}: {o:?}");
        // Positive control: with the job run, the same failure is a base on both bases.
        let mut f = server(std::slice::from_ref(&ok));
        assert!(
            grade_on(&mut f, &ok, touched(&["debrief"]), decisive).is_base(),
            "decisive={decisive}"
        );
    }
}

#[test]
fn a_newer_failed_full_run_blocks_an_older_green_one_even_when_it_is_graded_but_not_granted() {
    // P1 of review round 2: the older run is complete and green; the newer one failed with only a FLAKY test but a declared job was
    // skipped (or its declared scope cannot be shown). The newest full run decides (ER-15): the commit is not green.
    let c = sha(2);
    let older = Run::push(8600, &c, "success");
    let skipped = Run::train(8601, &c, "failure")
        .failing(CRATE, CRATE_FLAKY)
        .edit("conformance", |j| j["conclusion"] = json!("skipped"));
    // The report-only phase with a plan log that has the old line only (no declared scope): fix items are refused.
    let no_scope = Run::train(8602, &c, "failure")
        .failing(CRATE, CRATE_FLAKY)
        .partial("plan verdict: complete=true\n");
    for (newer, decisive) in [(&skipped, true), (&skipped, false), (&no_scope, false)] {
        let runs = [older_clone(&older), clone_run(newer)];
        let mut f = server(&runs);
        let refs: Vec<RunInfo> = runs.iter().map(Run::info).collect();
        let refs: Vec<&RunInfo> = refs.iter().collect();
        let got = commit_outcome(&mut client(&mut f), &refs, decisive, &mut |_| Some(touched(&["debrief"]))).expect("an outcome");
        assert_eq!(got.run, newer.id, "decisive={decisive}: the newer run decides");
        assert!(got.outcome.blocking && !got.outcome.is_base(), "decisive={decisive}: {got:?}");
        let found = find_base(&mut client(&mut f), "ci.yml", std::slice::from_ref(&c), decisive, &mut |_, _| {
            Some(touched(&["debrief"]))
        });
        assert_eq!(found.unwrap(), None, "decisive={decisive}: no base");
    }
    // Positive control: a newer run that is only smaller (a smoke push) does not block: the older green decides.
    let smoke = Run::push(8603, &c, "success").partial(run_scope::TEST_PARTIAL);
    let runs = [older_clone(&older), smoke];
    let mut f = server(&runs);
    assert_eq!(base(&mut f, std::slice::from_ref(&c), touched(&[])).map(|b| b.run), Some(8600));
}

#[test]
fn a_declared_job_that_did_not_run_makes_a_run_partial_on_both_bases() {
    // P2-0: ER-41's report-only phase decides on the old line, which never checked the jobs. A skipped declared job is partial on
    // either basis, green or not, until new runs carry the scope line and the first green scope run is recorded.
    let c = sha(2);
    for conclusion in ["success", "failure"] {
        let r = Run::train(8610, &c, conclusion).edit("conformance", |j| j["conclusion"] = json!("skipped"));
        for decisive in [true, false] {
            let mut f = server(std::slice::from_ref(&r));
            let o = grade_on(&mut f, &r, touched(&[]), decisive);
            assert!(o.scope == Scope::Partial && !o.is_base(), "{conclusion} decisive={decisive}: {o:?}");
        }
        // The old line on its own (no scope line, as the runs before ADR 0113) still decides: complete and green.
        let old = Run::train(8611, &c, conclusion).partial("plan verdict: complete=true\n");
        let mut f = server(std::slice::from_ref(&old));
        let o = grade_on(&mut f, &old, touched(&[]), false);
        assert_eq!(o.scope, Scope::Complete, "{conclusion}: {o:?}");
    }
}

#[test]
fn a_cancelled_job_in_a_failed_run_is_not_measured_on_either_basis() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure")
        .failing(CRATE, CRATE_FLAKY)
        .edit("conformance", |j| j["conclusion"] = json!("cancelled"));
    let ok = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
    for decisive in [true, false] {
        let mut f = server(std::slice::from_ref(&r));
        let o = grade_on(&mut f, &r, touched(&["debrief"]), decisive);
        assert!(!o.is_base(), "decisive={decisive}: {o:?}");
        assert!(
            !matches!(o.result, GradedResult::GreenWithFixItems(_)),
            "decisive={decisive}: {o:?}"
        );
        let mut f = server(std::slice::from_ref(&ok));
        assert!(grade_on(&mut f, &ok, touched(&["debrief"]), decisive).is_base());
    }
    // A cancelled job the plan did not declare (it is not in the declared scope): the run is still not measured, never fix items.
    let other = Run::train(8610, &c, "failure")
        .failing(CRATE, CRATE_FLAKY)
        .failing(APP, "Job: app\n")
        .edit(APP, |j| j["conclusion"] = json!("cancelled"));
    let mut f = server(std::slice::from_ref(&other));
    assert!(matches!(
        grade_on(&mut f, &other, touched(&["debrief"]), false).result,
        GradedResult::NotMeasured(_)
    ));
}

#[test]
fn a_smoke_or_partial_run_never_becomes_the_base() {
    let (a, b) = (sha(2), sha(1));
    let runs = [
        Run::push(8700, &a, "success").partial(run_scope::TEST_PARTIAL),
        Run::train(8701, &b, "success").partial(run_scope::TEST_OS_ONLY),
    ];
    let mut f = server(&runs);
    assert_eq!(base(&mut f, &[a, b], touched(&[])), None, "green but partial is not a base");
}

#[test]
fn a_test_report_that_cannot_be_read_is_not_measured() {
    let c = sha(2);
    // The real capture cut before its summary: nextest started and never finished in the log.
    let cut: String = CRATE_FLAKY
        .lines()
        .filter(|l| !l.contains("Summary"))
        .map(|l| format!("{l}\n"))
        .collect();
    for log in [Some(cut), None] {
        let mut r = Run::train(8610, &c, "failure");
        match &log {
            Some(l) => r = r.failing(CRATE, l),
            None => {
                r = r.failing(CRATE, "");
                r.logs.clear();
            }
        }
        let mut f = server(std::slice::from_ref(&r));
        let o = grade(&mut f, &r, touched(&["debrief"]));
        assert!(matches!(o.result, GradedResult::NotMeasured(_)), "{o:?}");
        assert_eq!(
            base(&mut f, std::slice::from_ref(&c), touched(&["debrief"])),
            None,
            "not measured is no base"
        );
    }
}

#[test]
fn a_cancelled_run_is_partial_not_measured_never_a_pass_or_a_failure() {
    let c = sha(2);
    let r = Run::push(8341, &c, "cancelled");
    let mut f = server(std::slice::from_ref(&r));
    let o = grade_on(&mut f, &r, Touched::default(), true);
    assert_eq!(o.scope, Scope::Partial);
    assert!(matches!(o.result, GradedResult::NotMeasured(_)), "{o:?}");
    assert!(!o.is_base() && !o.is_tag_green());
    assert_eq!(f.requested().len(), 0, "a cancelled run costs no call");
}

#[test]
fn a_whole_tree_run_is_complete_whatever_its_base_and_ends_the_chain() {
    // A nightly (schedule) or a plan of `all` declares the whole scope without any base: complete, and a base for the next push.
    let c = sha(2);
    let mut nightly = Run::train(8800, &c, "success");
    nightly.event = "schedule";
    nightly.branch = "main";
    let mut f = server(std::slice::from_ref(&nightly));
    let o = grade(&mut f, &nightly, Touched::default());
    assert_eq!((o.scope, o.result), (Scope::Complete, GradedResult::Green));
    assert_eq!(
        base(&mut f, std::slice::from_ref(&c), Touched::default()).map(|b| b.run),
        Some(8800)
    );
}

#[test]
fn a_run_rerun_in_place_is_graded_on_its_earlier_attempts_too() {
    // P1-3: Gitea keeps the run id on a rerun, so the last attempt's FLAKY-only failure could hide a failed job of attempt 1.
    let c = sha(2);
    let attempt1_failed = vec![
        real_job("plan", 8610, 0),
        real_job("conformance", 8610, 1),
        real_job(CRATE, 8610, 2),
    ];
    let attempt1_clean = vec![real_job("plan", 8610, 0), real_job("conformance", 8610, 1)];
    let rerun = |earlier: Option<Vec<Value>>| {
        let mut r = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
        r.attempt = 2;
        if let Some(jobs) = earlier {
            r.earlier.push((1, jobs));
        }
        r
    };
    let red = rerun(Some(attempt1_failed));
    let mut f = server(std::slice::from_ref(&red));
    assert!(matches!(&grade(&mut f, &red, touched(&["debrief"])).result, GradedResult::Red(w) if w.contains("attempt 1")));
    let unread = rerun(None);
    let mut f = server(std::slice::from_ref(&unread));
    assert!(matches!(
        grade(&mut f, &unread, touched(&["debrief"])).result,
        GradedResult::NotMeasured(_)
    ));
    let clean = rerun(Some(attempt1_clean));
    let mut f = server(std::slice::from_ref(&clean));
    assert!(matches!(
        grade(&mut f, &clean, touched(&["debrief"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
}

#[test]
fn the_change_of_a_train_is_measured_from_the_last_good_commit_not_from_its_last_member() {
    // P1-4: a train lands by fast-forward, so `sha^1` is only its last member. Two members, m1 touched the spotter app and m2 the
    // dyno app; the last good commit is B. The FLAKY is in spotter's job: planning from `m2^1` would call it untouched.
    let (head, m1, b) = (sha(3), sha(2), sha(1));
    let good = Run::push(8500, &b, "success");
    let mut f = server(std::slice::from_ref(&good));
    let asked = std::cell::RefCell::new(Vec::new());
    let mut plan_of = |from: &str, to: &str| {
        asked.borrow_mut().push((from.to_string(), to.to_string()));
        // The plan of the range B..head reaches both members' apps; of m1..head (the `^1` range) only dyno.
        Some(if from == b {
            touched(&["spotter", "dyno"])
        } else {
            touched(&["dyno"])
        })
    };
    let t = {
        let runs = Runs::read(&mut client(&mut f), "ci.yml").unwrap();
        touched_since_base(&mut client(&mut f), &runs, &[m1.clone(), b.clone()], &head, true, &mut plan_of).expect("planned")
    };
    assert_eq!(
        *asked.borrow(),
        vec![(b.clone(), head.clone())],
        "the range starts at the last good commit"
    );
    assert!(t.reaches(APP), "member 1's app is reached");
    // No good commit among the candidates: the change cannot be bounded, every target is reached.
    let mut f = server(&[]);
    let runs = Runs::read(&mut client(&mut f), "ci.yml").unwrap();
    let all = touched_since_base(&mut client(&mut f), &runs, &[m1], &head, true, &mut plan_of).expect("bounded as everything");
    assert!(all.all && all.reaches(APP) && all.reaches(CRATE));
    // A range that cannot be planned is not measured.
    let mut f = server(std::slice::from_ref(&good));
    let runs = Runs::read(&mut client(&mut f), "ci.yml").unwrap();
    assert_eq!(
        touched_since_base(&mut client(&mut f), &runs, &[b], &head, true, &mut |_, _| None),
        None
    );
}

#[test]
fn the_search_costs_pages_plus_the_candidate_runs_not_a_call_per_commit() {
    // 200 first-parent commits, 140 runs (three pages): 100 cancelled, 20 partial (a push that reused another run, a smoke run) and
    // 19 more cancelled, then one complete green one. A cancelled run costs nothing; each partial one a job list and a plan log; the
    // green one the same: whatever the number of commits, the calls are the pages plus two per run that is evidence.
    let commits: Vec<String> = (1..=200u32).rev().map(sha).collect();
    let mut runs: Vec<Run> = Vec::new();
    for i in 0..139u32 {
        let id = 9000 - u64::from(i);
        let c = &commits[i as usize];
        runs.push(if (100..120).contains(&i) {
            Run::push(id, c, "success").partial(run_scope::TEST_PARTIAL)
        } else {
            Run::push(id, c, "cancelled")
        });
    }
    runs.push(Run::train(8000, &commits[139], "success"));
    let mut f = server(&runs);
    let b = base(&mut f, &commits, touched(&[])).expect("the green one is found");
    assert_eq!(b.sha, commits[139]);
    let calls = f.requested().len();
    let (pages, evidence) = (3, 20 + 1);
    assert_eq!(
        calls,
        pages + 2 * evidence,
        "pages plus a job list and a plan log per run that is evidence: {:?}",
        f.requested().len()
    );
    assert!(
        calls <= usize::try_from(MAX_PAGES).unwrap() + 2 * 201,
        "never more than the pages and two calls per candidate commit"
    );
}

#[test]
fn the_candidates_are_the_first_parent_commits_before_the_asking_one_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = crate::git_env::git()
            .arg("-C")
            .arg(dir.path())
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    let mut shas = Vec::new();
    for n in 0..3 {
        git(&["commit", "-q", "--allow-empty", "-m", &format!("c{n}")]);
        shas.push(git(&["rev-parse", "HEAD"]));
    }
    assert_eq!(first_parent_before(dir.path(), &shas[2]), vec![shas[1].clone(), shas[0].clone()]);
    assert!(
        first_parent_before(dir.path(), &"f".repeat(40)).is_empty(),
        "an unknown commit gives no candidates, so everything runs"
    );
}

#[test]
fn the_search_with_a_touched_measure_reads_the_run_list_once() {
    // P2-2: a commit whose run failed with only FLAKY tests needs a measure of what its change touched, which searches for the last good
    // commit in the same run list. 150 commits, runs on three pages; the newest candidate is a FLAKY-only landing (it asks the measure,
    // which is not constant: it plans a range from the base it found), the last one the plain green base. The run list is read in three
    // requests in all, however many measures run.
    let commits: Vec<String> = (1..=150u32).rev().map(sha).collect();
    let mut runs: Vec<Run> = vec![Run::train(9500, &commits[0], "failure").failing(CRATE, CRATE_FLAKY)];
    for i in 1..139u32 {
        runs.push(Run::push(9000 - u64::from(i), &commits[i as usize], "cancelled"));
    }
    runs.push(Run::train(8000, &commits[139], "success"));
    let mut f = server(&runs);
    let planned = std::cell::RefCell::new(Vec::new());
    let found = {
        let mut c = client(&mut f);
        let list = Runs::read(&mut c, "ci.yml").unwrap();
        find_base_in(&mut c, &list, &commits, true, &mut |c, list, sha| {
            let at = commits.iter().position(|x| x == sha).unwrap();
            touched_since_base(c, list, &commits[at + 1..], sha, true, &mut |b, h| {
                planned.borrow_mut().push((b.to_string(), h.to_string()));
                Some(touched(&["debrief"]))
            })
        })
    };
    // The FLAKY reaches nothing of the change, so the newest commit is the base, with fix items.
    let base = found.expect("a base");
    assert_eq!(base.sha, commits[0]);
    assert!(matches!(base.outcome.result, GradedResult::GreenWithFixItems(_)));
    assert_eq!(
        *planned.borrow(),
        vec![(commits[139].clone(), commits[0].clone())],
        "the range starts at the last good commit"
    );
    let pages = f.requested().iter().filter(|u| u.contains("/runs?limit=50&page=")).count();
    assert_eq!(pages, 3, "the run list is read once: {:?}", f.requested());
}

// ---- ADR 0117 WP-8: the base of a train's landing dispatch (G1, G14) ----

fn landing_inputs(f: &mut Fake, git_ref: &str, inputs: &[(&str, &str)], chain: &[String]) -> (Vec<(String, String)>, Vec<String>) {
    let given: Vec<(String, String)> = inputs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut said = Vec::new();
    let got = super::super::landing::with_landing_base(
        "ci.yml",
        git_ref,
        given,
        &mut || Ok(base(f, chain, touched(&[])).map(|b| (b.sha, b.run))),
        &mut |l| said.push(l.to_string()),
    );
    (got, said)
}

#[test]
fn a_train_dispatch_carries_the_last_complete_commit_of_main_as_its_landing_base() {
    // The 8554 shape: main's tip was tested by its train's run; a push after it was cancelled.
    let (tip, older) = (sha(2), sha(1));
    let mut f = server(&[
        Run::push(8341, &tip, "cancelled"),
        Run::train(8554, &tip, "success"),
        Run::push(8045, &older, "success"),
    ]);
    let (got, said) = landing_inputs(&mut f, "train/2026-11-23-20", &[], &[tip.clone(), older]);
    assert_eq!(got, vec![("landing_base".to_string(), tip)], "{said:?}");
    assert!(said.iter().any(|l| l.contains("run 8554")), "{said:?}");
}

#[test]
fn a_train_dispatch_without_a_complete_base_stays_as_it_was_and_the_run_tests_everything() {
    // Fail closed: only a partial run, no run at all, and runs that cannot be read send no base.
    let (a, b) = (sha(2), sha(1));
    let mut f = server(&[Run::push(8700, &a, "success").partial(run_scope::TEST_PARTIAL)]);
    let (got, said) = landing_inputs(&mut f, "train/x", &[], &[a.clone(), b.clone()]);
    assert!(got.is_empty(), "{got:?}");
    assert!(said.iter().any(|l| l.contains("tests everything")), "{said:?}");
    let mut said = Vec::new();
    let got = super::super::landing::with_landing_base("ci.yml", "train/x", vec![], &mut || Err("HTTP 500".into()), &mut |l| {
        said.push(l.to_string())
    });
    assert!(got.is_empty() && said.iter().any(|l| l.contains("HTTP 500")), "{got:?} {said:?}");
}

#[test]
fn only_a_full_ci_dispatch_on_a_train_is_a_landing_run_nightly_tags_and_the_other_kinds_stay_whole_tree() {
    let tip = sha(2);
    let mut f = server(&[Run::train(8554, &tip, "success")]);
    // Positive control: the same world gives the full train dispatch a base.
    assert_eq!(landing_inputs(&mut f, "train/x", &[], std::slice::from_ref(&tip)).0.len(), 1);
    // Whole-tree runs (a tag, a manual run on main, the nightly's ref) and every other kind of run ask for nothing and change nothing.
    for (git_ref, inputs) in [
        ("refs/tags/v1.0.0", vec![]),
        ("main", vec![]),
        ("train/x", vec![("base", "origin/main")]),
        ("train/x", vec![("since_run", "8001")]),
        ("train/x", vec![("os", "linux")]),
        ("train/x", vec![("app", "dyno")]),
        ("train/x", vec![("landing_base", "abc")]),
    ] {
        let mut asked = false;
        let given: Vec<(String, String)> = inputs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let got = super::super::landing::with_landing_base(
            "ci.yml",
            git_ref,
            given.clone(),
            &mut || {
                asked = true;
                Ok(None)
            },
            &mut |_| {},
        );
        assert!(!asked && got == given, "{git_ref} {inputs:?}: asked={asked} got={got:?}");
    }
    // Another workflow (an app's own) is not ci.yml.
    let got = super::super::landing::with_landing_base("dyno.yml", "train/x", vec![], &mut || Ok(Some(("a".into(), 1))), &mut |_| {});
    assert!(got.is_empty());
}

#[test]
fn a_smoke_push_followed_by_a_code_push_plans_from_the_last_complete_commit() {
    // WP-4's golden: a plumbing-only push (smoke, partial) sits between the last complete commit and the new head, so the landing
    // diffs from the commit before the smoke push (and runs the plumbing change's jobs too).
    let (code, smoke, complete) = (sha(3), sha(2), sha(1));
    let mut f = server(&[
        Run::push(8802, &smoke, "success").partial(run_scope::TEST_PARTIAL),
        Run::train(8801, &complete, "success"),
    ]);
    let (got, _) = landing_inputs(&mut f, "train/x", &[], &[code, smoke, complete.clone()]);
    assert_eq!(got, vec![("landing_base".to_string(), complete)]);
}

/// The plan line of a landing run that ran exactly its own diff's narrowed selection: the diff from `base`, the conformance job, and
/// `projects` that ran only their smoke set (the tools vm-smoke phase of a Debrief-spec-only train) or affected tests.
fn landing_line(base: &str, smoke: &[&str], affected: &[&str]) -> String {
    let need = run_scope::Scope {
        jobs: ["conformance".to_string()].into(),
        engines: ["chromium".to_string(), "webkit".to_string()].into(),
        whole: true,
        os_full: true,
        ..run_scope::Scope::default()
    };
    let ran = run_scope::Scope {
        smoke: smoke.iter().map(|s| s.to_string()).collect(),
        affected: affected.iter().map(|s| s.to_string()).collect(),
        ..need.clone()
    };
    format!("{}\n", run_scope::ci_scope(ran, false, need, false).with_base(Some(base)).line())
}

/// The server of [`server`], plus each run listed under its own commit (`runs?head_sha=`), as the verdict reader asks.
fn server_by_sha(runs: &[Run]) -> Fake {
    let mut f = server(runs);
    let mut by: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for r in runs {
        by.entry(r.sha.clone()).or_default().push(r.json());
    }
    for (s, list) in by {
        f.json(
            &format!("{API}/runs?head_sha={s}&limit=50&page=1"),
            json!({"total_count": list.len(), "workflow_runs": list}),
        );
    }
    f
}

#[test]
fn a_landing_run_of_its_own_narrowed_diff_lands_but_is_never_a_base_nor_tag_green() {
    // Review round 2, P1-A (the user's decision of 2026-11-23): a train whose ci.yml landing run ran its own diff's selection (here
    // the tools smoke phase, as for a train that touches only a Debrief spec) lands on it.
    let (main, head) = (sha(1), sha(2));
    let landing = Run::train(8801, &head, "success").partial(&landing_line(&main, &["tools"], &[]));
    let mut f = server_by_sha(&[Run::push(8800, &main, "success"), clone_run(&landing)]);
    let o = grade(&mut f, &landing, touched(&[]));
    assert_eq!((o.scope, o.result.clone()), (Scope::Landing, GradedResult::Green), "{o:?}");
    assert!(o.lands(), "it lands");
    assert!(!o.is_base(), "never a base");
    assert!(!o.is_tag_green(), "never for a tag");
    assert_eq!(o.landing_base.as_deref(), Some(main.as_str()));
    // The landing verdict accepts it; the verdict of a tag or a base does not.
    let (v, from) = verdict::landing_verdict_with(&mut client(&mut f), "ci.yml", &head, None, true, &mut |_, _| Some(touched(&[])));
    assert_eq!(v, verdict::Verdict::Green { run: 8801 });
    assert_eq!(from.as_deref(), Some(main.as_str()));
    let plain = verdict::verdict_with_touched(&mut client(&mut f), "ci.yml", &head, None, true, &mut Vec::new, &mut |_, _| {
        Some(touched(&[]))
    });
    // Run 8983: the reason named the landing run with an empty cause, "8983 ()". It says why the run is not complete here.
    assert!(
        matches!(&plain, verdict::Verdict::Red { reason, .. }
            if reason.contains("8801 (a landing run") && reason.contains("smoke set") && reason.contains("tools") && !reason.contains("8801 ()")),
        "{plain:?}"
    );
    // Not a base: the search goes past it to the complete commit before it.
    assert_eq!(
        base(&mut f, &[head.clone(), main.clone()], touched(&[])).map(|b| b.sha),
        Some(main.clone())
    );
    // Controls: the same run with a job of its need not run, or planned for the whole tree, does not land.
    let short = Run::train(8802, &head, "success").edit("conformance", |j| j["conclusion"] = json!("skipped"));
    let short = short.partial(&landing_line(&main, &["tools"], &[]));
    let mut f2 = server_by_sha(&[clone_run(&short)]);
    assert!(!grade(&mut f2, &short, touched(&[])).lands());
    let whole = landing_line(&main, &["tools"], &[]).replace("\"need_whole\":false", "\"need_whole\":true");
    let whole = Run::train(8803, &head, "success").partial(&whole);
    let mut f3 = server_by_sha(&[clone_run(&whole)]);
    assert_eq!(grade(&mut f3, &whole, touched(&[])).scope, Scope::Partial);
}

#[test]
fn a_push_reuses_a_landing_run_only_when_it_tested_the_pushs_own_diff() {
    let (main, head, other) = (sha(1), sha(2), sha(3));
    let landing = |base: &str| {
        let mut r = Run::train(8811, &head, "success").partial(&landing_line(base, &[], &["app:debrief"]));
        // Every job that ran has a log (the reuse reads them for FLAKY tests).
        r.logs.push((r.jobs[1]["id"].as_u64().unwrap(), "ok\n".into()));
        r
    };
    let decide = |r: Run| {
        let mut f = server_by_sha(&[Run::push(8810, &main, "success"), r]);
        crate::ci_runs::reuse::decide_with_touched(
            &mut client(&mut f),
            "ci.yml",
            &head,
            None,
            true,
            &mut |_| vec![main.clone()],
            &mut |_, _| Some(touched(&[])),
            &mut |_, _| false,
        )
    };
    assert!(decide(landing(&main)).skip(), "the landing ran this push's diff (from {main})");
    let other_diff = decide(landing(&other));
    assert!(!other_diff.skip(), "another diff: the push runs its own ({other_diff:?})");
}

/// A temporary repository in the shape of main and train 2026-11-26-g on 2026-11-25: main at `b` (b3ad2dd2d), then `x` (4284e23d5,
/// train f's landing); the train was cut from `b`, merged `x` (af3085462) and ends at `y` (4ca4c56ec), to which main fast-forwarded.
/// `x` changes `flip.txt`; `y` changes it back when `revert_flip`, so the diff from `b` does not hold it while the diff from `x` does.
struct Shape {
    dir: tempfile::TempDir,
    b: String,
    x: String,
    y: String,
}

/// A private copy of the shape's template, built once per machine (`paddock_testkit::git_fixture::template`: the shape's 14 git starts are paid once, not
/// by every test that reads it, ER-20); the commits' ids are in the template's `shas` file.
fn train_g_shape(revert_flip: bool) -> Shape {
    let name = if revert_flip { "shape-revert" } else { "shape" };
    let t = paddock_testkit::git_fixture::template(name, include_str!("tests.rs"), |build, _| {
        let repo = build.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let [b, x, y] = build_shape(&repo, revert_flip);
        std::fs::write(build.join("shas"), format!("{b}\n{x}\n{y}\n")).unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    paddock_testkit::git_fixture::copy_tree(&t.join("repo"), dir.path());
    let shas = std::fs::read_to_string(t.join("shas")).unwrap();
    let mut it = shas.lines().map(str::to_string);
    Shape {
        dir,
        b: it.next().unwrap(),
        x: it.next().unwrap(),
        y: it.next().unwrap(),
    }
}

/// Budget (rule 85): a shape is a file copy of its template, not a build: two copies have the same commits, are private, and a write in
/// one is in neither the other nor the template.
#[test]
fn a_shape_is_a_private_copy_of_its_template_with_the_same_commits() {
    let (a, b) = (train_g_shape(false), train_g_shape(false));
    assert_eq!((&a.b, &a.x, &a.y), (&b.b, &b.x, &b.y));
    assert_ne!(a.dir.path(), b.dir.path());
    std::fs::write(a.dir.path().join("only-a.txt"), "a").unwrap();
    assert!(!b.dir.path().join("only-a.txt").exists());
    assert!(a.dir.path().join(".git").is_dir() && a.dir.path().join("y.txt").is_file());
    assert_ne!(train_g_shape(true).dir.path().join("flip.txt"), a.dir.path().join("flip.txt"));
    assert_eq!(
        std::fs::read_to_string(train_g_shape(true).dir.path().join("flip.txt")).unwrap(),
        "a\n"
    );
    assert_eq!(std::fs::read_to_string(a.dir.path().join("flip.txt")).unwrap(), "b\n");
}

/// Builds the shape in `root` and returns the ids of `b`, `x` and `y`.
fn build_shape(root: &std::path::Path, revert_flip: bool) -> [String; 3] {
    let git = |args: &[&str]| {
        let out = crate::git_env::git()
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let write = |p: &str, t: &str| std::fs::write(root.join(p), t).unwrap();
    git(&["init", "-q", "-b", "main"]);
    // LF checkouts on every OS before the first checkout (ER-61; the Windows runner's git wrote `a\r\n` here, run 9143).
    paddock_testkit::git_fixture::lf_only(root);
    write("flip.txt", "a\n");
    write("base.txt", "b\n");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "b"]);
    let b = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "-b", "train"]);
    write("member.txt", "m\n");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "member"]);
    git(&["checkout", "-q", "main"]);
    write("flip.txt", "b\n");
    write("x.txt", "x\n");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "x: train f lands"]);
    let x = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "train"]);
    git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "chore(train): merge origin/main (train f landed) into train g",
        "main",
    ]);
    if revert_flip {
        write("flip.txt", "a\n");
    }
    write("y.txt", "y\n");
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "y"]);
    let y = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "main"]);
    git(&["merge", "-q", "--ff-only", "train"]);
    [b, x, y]
}

/// A recorded run (`tests/ci-runs/reuse-push-base`: runs 9030, 9045, 9054 and 9056 of 2026-11-25, `./dev ci record-fixture`; the
/// plan logs are the `plan scope:` line of each run's plan job, nothing else) on the commit `sha` of the temporary repository, its
/// recorded commits replaced by the shape's (`map`).
fn recorded(id: u64, sha: &str, map: &[(&str, &str)]) -> Run {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ci-runs/reuse-push-base");
    let read = |f: String| std::fs::read_to_string(dir.join(f)).unwrap();
    let run: Value = serde_json::from_str(&read(format!("run-{id}.json"))).unwrap();
    let jobs: Value = serde_json::from_str(&read(format!("jobs-{id}.json"))).unwrap();
    let jobs: Vec<Value> = jobs["jobs"].as_array().unwrap().clone();
    let mut plan = std::fs::read_to_string(dir.join(format!("plan-{id}.log"))).unwrap_or_default();
    for (from, to) in map {
        plan = plan.replace(from, to);
    }
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    // Every job that ran has a log (the reuse reads them for FLAKY tests); the plan job's is its scope line.
    let logs = jobs
        .iter()
        .skip(1)
        .filter(|j| j["conclusion"].as_str() != Some("skipped"))
        .map(|j| (j["id"].as_u64().unwrap(), "ok\n".to_string()))
        .collect();
    Run {
        id,
        sha: sha.to_string(),
        event: leak(run["event"].as_str().unwrap()),
        branch: leak(run["head_branch"].as_str().unwrap()),
        conclusion: leak(run["conclusion"].as_str().unwrap()),
        status: "completed",
        attempt: 1,
        plan,
        jobs,
        logs,
        earlier: Vec::new(),
    }
}

/// Push run 9056 asks whether train g's landing run 9054 on the same commit can be reused: main moved from `x` to `y`.
fn push_9056_reuse(s: &Shape, before: Option<&str>) -> crate::ci_runs::reuse::Reuse {
    const B: &str = "b3ad2dd2dc1a82f52c3388a4d96af5d2a9e9387d";
    const X: &str = "4284e23d53e0eebe5c7660e210ddbe0a0870f1b0";
    let map = [(B, s.b.as_str()), (X, s.x.as_str())];
    let mut asking = recorded(9056, &s.y, &map);
    asking.status = "in_progress";
    asking.conclusion = "";
    let mut f = server_by_sha(&[
        asking,
        recorded(9054, &s.y, &map),
        recorded(9045, &s.x, &map),
        recorded(9030, &s.b, &map),
    ]);
    crate::ci_runs::reuse::decide_with(&mut client(&mut f), "ci.yml", &s.y, Some(9056), true, s.dir.path(), before)
}

#[test]
fn a_push_that_fast_forwards_main_reuses_the_landing_run_that_tested_the_diff_from_mains_previous_tip() {
    // Runs 9054 and 9056: the landing run tested the diff from x (main's last complete commit, run 9045); the push moved main from x
    // to y, so its diff is from x too. Its own first-parent line runs through the train and never meets x.
    let s = train_g_shape(false);
    let r = push_9056_reuse(&s, Some(&s.x));
    assert!(matches!(r, crate::ci_runs::reuse::Reuse::Skip { run: 9054, .. }), "{r:?}");
}

#[test]
fn without_the_pushs_before_the_older_base_is_used_and_the_landing_run_is_not_reused() {
    // The control: the same runs with no `before` (no push event): the base is b, two mains back, and the landing run tested less.
    let s = train_g_shape(false);
    let r = push_9056_reuse(&s, None);
    assert!(
        matches!(&r, crate::ci_runs::reuse::Reuse::Run(why) if why.contains(&s.x) && why.contains(&s.b)),
        "{r:?}"
    );
}

#[test]
fn a_landing_diff_from_an_older_main_is_reused_only_when_it_holds_every_path_the_pushs_diff_changes() {
    let s = train_g_shape(false);
    // From b, an ancestor of the push's base x, with every path of x..y in b..y: it tested at least the push's diff.
    assert!(diff_covers(s.dir.path(), &s.b, &s.x, &s.y));
    // A run from a newer commit than the base never covers it, nor a commit git does not know.
    assert!(!diff_covers(s.dir.path(), &s.x, &s.b, &s.y));
    assert!(!diff_covers(s.dir.path(), &"e".repeat(40), &s.x, &s.y));
    // y changes flip.txt back to b's text: x..y holds it, b..y does not, so the diff from b tested less than the push.
    let r = train_g_shape(true);
    assert!(!diff_covers(r.dir.path(), &r.b, &r.x, &r.y));
}

#[test]
fn the_push_chain_is_mains_line_from_before_and_falls_back_to_the_first_parent_line() {
    let s = train_g_shape(false);
    assert_eq!(push_chain(s.dir.path(), &s.y, Some(&s.x)), vec![s.x.clone(), s.b.clone()]);
    let train_line = first_parent_before(s.dir.path(), &s.y);
    assert!(!train_line.contains(&s.x), "the train's own line never meets x: {train_line:?}");
    for before in [
        None,
        Some("0".repeat(40)),
        Some(s.y.clone()),
        Some("e".repeat(40)),
        Some("not a sha".into()),
    ] {
        assert_eq!(push_chain(s.dir.path(), &s.y, before.as_deref()), train_line, "{before:?}");
    }
}

/// A temporary repository in the shape of main and trains i and j on 2026-11-26 (ER-46: invented ids): main at `b`, then `x` (a commit
/// on main, a push run of its own); train i was cut from `b`, merged `x` and ended at `t`, to which main fast-forwarded; train j was
/// cut from `t` and ended at `y`, to which main fast-forwarded. The first-parent line of `t` and `y` never meets `x`.
struct Line {
    dir: tempfile::TempDir,
    b: String,
    x: String,
    t: String,
    y: String,
}

fn train_ij_line() -> Line {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = crate::git_env::git()
            .arg("-C")
            .arg(dir.path())
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let commit = |file: &str, msg: &str| {
        std::fs::write(dir.path().join(file), format!("{msg}\n")).unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", msg]);
        git(&["rev-parse", "HEAD"])
    };
    git(&["init", "-q", "-b", "main"]);
    let b = commit("base.txt", "b");
    git(&["checkout", "-q", "-b", "train-i"]);
    commit("member-i.txt", "member i");
    git(&["checkout", "-q", "main"]);
    let x = commit("x.txt", "x");
    git(&["checkout", "-q", "train-i"]);
    git(&["merge", "-q", "--no-ff", "-m", "merge main into train i", "main"]);
    let t = git(&["rev-parse", "HEAD"]);
    git(&["checkout", "-q", "main"]);
    git(&["merge", "-q", "--ff-only", "train-i"]);
    git(&["checkout", "-q", "-b", "train-j"]);
    let y = commit("y.txt", "y");
    git(&["checkout", "-q", "main"]);
    git(&["merge", "-q", "--ff-only", "train-j"]);
    Line { dir, b, x, t, y }
}

/// The runs of the line: complete pushes on `b` and `x`; train i's landing and push runs on `t` only narrowed (landing runs, never
/// complete); train j's landing run on `y` narrowed from `x`, and the push run on `y` (`asking`) still going.
fn train_ij_runs(l: &Line) -> Vec<Run> {
    let landing = |id: u64, sha: &str, base: &str| {
        let mut r = Run::train(id, sha, "success").partial(&landing_line(base, &[], &["app:debrief"]));
        r.logs.push((r.jobs[1]["id"].as_u64().unwrap(), "ok\n".into()));
        r
    };
    let mut asking = Run::push(9084, &l.y, "success");
    asking.status = "in_progress";
    asking.conclusion = "";
    vec![
        asking,
        landing(9082, &l.y, &l.x),
        Run::push(9080, &l.t, "success").partial(&landing_line(&l.x, &[], &["app:debrief"])),
        Run::push(9073, &l.x, "success"),
        Run::push(9030, &l.b, "success"),
    ]
}

#[test]
fn a_push_to_main_reuses_the_landing_run_when_main_moved_through_a_train_that_merged_an_older_tip() {
    // Push run 9084 on main 595a0f688: main moved from t (train i's head) to y; the landing run tested the diff from x, main's last
    // complete commit. The first-parent line of t skips x, so the push's own search found b, two mains back, and ran its own.
    let l = train_ij_line();
    let mut f = server_by_sha(&train_ij_runs(&l));
    let r = crate::ci_runs::reuse::decide_with(&mut client(&mut f), "ci.yml", &l.y, Some(9084), true, l.dir.path(), Some(&l.t));
    assert!(matches!(r, crate::ci_runs::reuse::Reuse::Skip { run: 9082, .. }), "{r:?}");
}

#[test]
fn the_push_the_planner_and_the_next_landing_dispatch_find_the_same_base_on_mains_own_line() {
    // One definition of complete, one candidate line: the push's reuse (above), the planner's base for a push and the landing base of
    // train k's dispatch all name x. The dispatch said "no complete green ci run among main's newest commits" over the first-parent line.
    let l = train_ij_line();
    let mut f = server_by_sha(&train_ij_runs(&l));
    let planned = crate::plan_cmd::green_base_in(&mut client(&mut f), l.dir.path(), "main", Some(&l.t), &mut |_| {});
    assert_eq!(planned.unwrap().map(|b| b.0), Some(l.x.clone()), "the planner's base for the push");
    let mut f = server_by_sha(&train_ij_runs(&l));
    let landing = crate::plan_cmd::green_base_over(&mut client(&mut f), l.dir.path(), "main", true, None, &mut |_| {});
    assert_eq!(
        landing.unwrap().map(|b| b.0),
        Some(l.x.clone()),
        "the landing base of the next train"
    );
}

#[test]
fn the_next_landing_dispatch_takes_mains_tip_when_a_complete_push_run_tested_it() {
    // The control: with a complete run on y itself, y is the base (the tip is a candidate), whatever the line below it holds.
    let l = train_ij_line();
    let mut runs = train_ij_runs(&l);
    runs[0] = Run::push(9084, &l.y, "success");
    let mut f = server_by_sha(&runs);
    let landing = crate::plan_cmd::green_base_over(&mut client(&mut f), l.dir.path(), "main", true, None, &mut |_| {});
    assert_eq!(landing.unwrap().map(|b| b.0), Some(l.y.clone()));
}

#[test]
fn the_main_line_holds_main_tips_that_are_ancestors_only_and_falls_back_to_the_first_parent_line() {
    let l = train_ij_line();
    let elsewhere = "e".repeat(40);
    let tips = vec![l.y.clone(), l.t.clone(), elsewhere, l.x.clone()];
    let below = first_parent_before(l.dir.path(), &l.y);
    assert_eq!(below.len(), 3, "t, train i's member and b: {below:?}");
    let with = |head: Vec<&String>| head.into_iter().cloned().chain(below.iter().skip(1).cloned()).collect::<Vec<_>>();
    // The tip and the tips below it that are ancestors, then the first-parent line (without what is already there).
    assert_eq!(main_line(l.dir.path(), "main", true, None, &tips), with(vec![&l.y, &l.t, &l.x]));
    // A push's own commit is left out; an unknown tip (a forced-away commit) never becomes a candidate.
    assert_eq!(main_line(l.dir.path(), &l.y, false, Some(&l.t), &tips), with(vec![&l.t, &l.x]));
    // No tips (the run list holds no push of main): the first-parent line, as before.
    let mut line = vec![l.y.clone()];
    line.extend(below);
    assert_eq!(main_line(l.dir.path(), "main", true, None, &[]), line);
}

#[test]
fn the_pushs_before_comes_from_the_push_event_only() {
    let dir = tempfile::tempdir().unwrap();
    let event = dir.path().join("event.json");
    std::fs::write(&event, r#"{"before":"4284e23d53e0eebe5c7660e210ddbe0a0870f1b0","after":"x"}"#).unwrap();
    let path = event.display().to_string();
    let env = |name: &'static str| {
        let path = path.clone();
        move |k: &str| match k {
            "GITHUB_EVENT_NAME" => Some(name.to_string()),
            "GITHUB_EVENT_PATH" => Some(path.clone()),
            _ => None,
        }
    };
    assert_eq!(
        push_before(&env("push")).as_deref(),
        Some("4284e23d53e0eebe5c7660e210ddbe0a0870f1b0")
    );
    assert_eq!(push_before(&env("workflow_dispatch")), None);
}

#[test]
fn the_landing_queues_port_lands_a_train_on_its_narrowed_landing_run_without_a_second_run() {
    // The queue's tick reads `Ci::verdict` (queue/tick.rs judge): through the real port, a train touching only a Debrief spec, whose
    // ci.yml landing run ran the tools smoke phase, is green at once, so the tick lands instead of dispatching again up to
    // MAX_RUNS_PER_HEAD and stopping as Stuck.
    let (main, head) = (sha(1), sha(2));
    let landing = Run::train(8821, &head, "success").partial(&landing_line(&main, &["tools"], &[]));
    let mut f = server_by_sha(&[Run::push(8820, &main, "success"), landing]);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut ci = crate::queue::real::GiteaCi {
        server: "https://gitea.invalid".into(),
        repo: "decryp7/paddock".into(),
        token: "t".into(),
        root,
        max_wait: std::time::Duration::from_secs(1),
        http: &mut f,
    };
    use crate::queue::ports::Ci;
    assert_eq!(ci.verdict("ci.yml", &head), verdict::Verdict::Green { run: 8821 });
}

/// The package level (user decision 2026-11-24): the plan reached the crates workspace through Cargo alone, at the packages `reached`;
/// the workspace holds paddock-chromium (the 8610 FLAKY test's package) and paddock-core. `narrow`: the package level decides.
fn packages(reached: &[&str], narrow: bool) -> Touched {
    Touched {
        flags: ["crates".to_string()].into(),
        cargo_only: ["crates".to_string()].into(),
        packages: [("crates".to_string(), reached.iter().map(|p| p.to_string()).collect())].into(),
        members: [("crates".to_string(), ["paddock-chromium", "paddock-core"].map(String::from).into())].into(),
        narrow,
        ..Touched::default()
    }
}

#[test]
fn a_flaky_test_in_an_untouched_package_is_a_fix_item_and_landable_once_the_package_level_decides() {
    // The 8610 log: a FLAKY test of paddock-chromium. The change reached the crates workspace, but only paddock-core.
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, packages(&["paddock-core"], true));
    assert!(o.lands() && !o.is_tag_green(), "{o:?}");
    let items = fix_items(&o);
    assert_eq!(items.len(), 1);
    assert!(items[0].test.starts_with("paddock-chromium::it "), "{items:?}");
    // Positive control (ER-33): the same run in the report-only phase is red on the job level: the package level is what lands it.
    let o = grade(&mut f, &r, packages(&["paddock-core"], false));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("reached")), "{o:?}");
    assert!(!o.lands());
}

/// The plan reached the crates workspace through Cargo, at paddock-core only; the workspace holds paddock-ipc (the 9001 FLAKY test's
/// package) and paddock-core.
fn ipc_untouched() -> Touched {
    Touched {
        flags: ["crates".to_string()].into(),
        cargo_only: ["crates".to_string()].into(),
        packages: [("crates".to_string(), ["paddock-core".to_string()].into())].into(),
        members: [("crates".to_string(), ["paddock-ipc", "paddock-core"].map(String::from).into())].into(),
        narrow: true,
        ..Touched::default()
    }
}

#[test]
fn a_flaky_test_in_the_crates_coverage_step_is_graded_only_when_the_coverage_floor_ran_after_it() {
    // Run 9001: the crates job's coverage step (cargo llvm-cov nextest, then the coverage floor) failed on one FLAKY paddock-ipc
    // test. The executor stopped at nextest's exit 100, so the floor never ran: not measured, naming it (before the fix the job had
    // "no step known to run tests").
    let c = sha(3);
    let r = Run::train(9001, &c, "failure").failing_in(JOBS_9001, CRATES, CRATES_COVERAGE_FLAKY);
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, ipc_untouched());
    assert!(
        matches!(&o.result, GradedResult::NotMeasured(why) if why.contains("coverage-gate.mjs") && why.contains("not measured")),
        "{o:?}"
    );
    assert!(!o.lands());
    // With the floor run after the tests and passed, the FLAKY test in an untouched package is a fix item and the run lands.
    let floored = format!(
        "{CRATES_COVERAGE_FLAKY}2026-11-24T17:53:44.0000000Z ran: node ../scripts/coverage-gate.mjs lcov.info 90 (exit 0, 0.4 s)\n"
    );
    let r = Run::train(9001, &c, "failure").failing_in(JOBS_9001, CRATES, &floored);
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, ipc_untouched());
    assert!(o.lands() && !o.is_tag_green(), "{o:?}");
    let items = fix_items(&o);
    assert_eq!(items.len(), 1);
    assert!(items[0].test.starts_with("paddock-ipc::it endings::"), "{items:?}");
    // Positive control (ER-12, ER-33): the same log with the test failing both tries blocks, floor or not.
    let both = floored.replace("TRY 2 PASS", "TRY 2 FAIL").replace("FLKY-FL 2/2", "TRY 2 FAIL");
    assert_ne!(both, floored);
    let r = Run::train(9001, &c, "failure").failing_in(JOBS_9001, CRATES, &both);
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, ipc_untouched());
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("both tries")), "{o:?}");
    assert!(!o.lands());
}

#[test]
fn the_same_flaky_test_in_a_touched_package_blocks() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, packages(&["paddock-chromium", "paddock-core"], true));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("touched code")), "{o:?}");
    assert!(!o.lands());
    // A project reached for a reason that is not a Cargo reach (a UI file, a lockfile) touches every test of it.
    let mut whole = packages(&["paddock-core"], true);
    whole.cargo_only.clear();
    assert!(matches!(grade(&mut f, &r, whole).result, GradedResult::Red(_)));
}

#[test]
fn a_test_that_failed_both_tries_blocks_on_the_package_level_too() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, &format!("Job: crate\n{BOTH_TRIES}"));
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, packages(&[], true));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("both tries")), "{o:?}");
}

#[test]
fn a_flaky_test_the_graph_cannot_place_blocks() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(CRATE, CRATE_FLAKY);
    let mut f = server(std::slice::from_ref(&r));
    // The crate is no package of the job's workspace (a renamed crate, a log of another workspace): fail closed.
    let mut unknown = packages(&["paddock-core"], true);
    unknown.members.insert("crates".into(), ["paddock-core".to_string()].into());
    let o = grade(&mut f, &r, unknown);
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("cannot place")), "{o:?}");
    // The workspace's packages were not read at all: fail closed too.
    let mut unread = packages(&["paddock-core"], true);
    unread.members.clear();
    assert!(matches!(grade(&mut f, &r, unread).result, GradedResult::Red(_)));
    // Positive control: with the package known the same run lands.
    assert!(grade(&mut f, &r, packages(&["paddock-core"], true)).lands());
}

#[test]
fn the_package_level_reads_the_job_test_and_plan() {
    let t = packages(&["paddock-core"], true);
    assert!(matches!(t.reaches_test(CRATE, "paddock-core::it a::b"), Reach::Touched(_)));
    assert!(matches!(t.reaches_test(CRATE, "paddock-chromium a::b"), Reach::Untouched(_)));
    // Not reached at the job level: untouched whatever the package.
    assert!(matches!(t.reaches_test(APP, "spotter-render::real_browser a"), Reach::Untouched(_)));
    // A Playwright test of a reached project has no package level: touched.
    assert!(matches!(
        t.reaches_test("crates (ubuntu-latest)", "[chromium] › a.spec.ts:1:2 › t"),
        Reach::Touched(_)
    ));
    // An app's Rust tests narrow in the app's own workspace.
    let mut app = Touched {
        apps: ["spotter".to_string()].into(),
        cargo_only: ["app:spotter".to_string()].into(),
        app_cargo: [("spotter".to_string(), "apps/spotter/app/src-tauri".to_string())].into(),
        members: [("apps/spotter/app/src-tauri".to_string(), ["spotter-render".to_string()].into())].into(),
        narrow: true,
        ..Touched::default()
    };
    assert!(matches!(
        app.reaches_test(APP, "spotter-render::real_browser a"),
        Reach::Untouched(_)
    ));
    app.packages
        .insert("apps/spotter/app/src-tauri".into(), ["spotter-render".to_string()].into());
    assert!(matches!(app.reaches_test(APP, "spotter-render::real_browser a"), Reach::Touched(_)));
    // The plan's JSON gives the packages and cargoOnly.
    let p = Touched::from_plan(&json!({"crates": true, "apps": [], "packages": {"crates": ["paddock-core"]}, "cargoOnly": ["crates"]}));
    assert_eq!(p.packages["crates"], ["paddock-core".to_string()].into());
    assert!(p.cargo_only.contains("crates") && !p.narrow);
    // `all`: the change cannot be bounded, everything is touched.
    let all = Touched { all: true, ..t };
    assert!(matches!(all.reaches_test(CRATE, "paddock-chromium a::b"), Reach::Touched(_)));
}
