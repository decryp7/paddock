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
    let all: Value = serde_json::from_str(JOBS_8610).unwrap();
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
    fn failing(mut self, name: &str, log: &str) -> Run {
        let job = real_job(name, self.id, 2 + self.jobs.len() as u64);
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
            &mut || vec![main.clone()],
            &mut |_, _| Some(touched(&[])),
        )
    };
    assert!(decide(landing(&main)).skip(), "the landing ran this push's diff (from {main})");
    let other_diff = decide(landing(&other));
    assert!(!other_diff.skip(), "another diff: the push runs its own ({other_diff:?})");
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
