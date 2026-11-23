// ci-runs-reader: the tests of the one definition of green, over the fake runs API
//! The one definition of green (ADR 0117 WP-4), over the fake server of `testutil`.
//!
//! Provenance of the fixtures: the run and job records have the keys the Gitea Actions API returns (`id`, `path`, `event`,
//! `head_branch`, `head_sha`, `status`, `conclusion`, `run_attempt`, `created_at`; jobs `id`, `name`, `conclusion`); the plan logs are
//! `run_scope::TEST_COMPLETE`, `TEST_PARTIAL` and `TEST_OS_ONLY`, lines of the shape `./dev plan --ci` prints; the job logs are real
//! nextest captures in `tests/ci-runs/`: `nextest-r1-flaky-only.log` (a FLAKY test and nothing else, flaky-result = "fail",
//! cargo-nextest 0.9.152, captured 2026-11-23 from a scratch crate) and `nextest-r1-pass.log` (a test that failed both tries beside
//! a FLAKY one, 0.9.149).

use super::*;
use crate::ci_runs::testutil::Fake;
use crate::run_scope;
use serde_json::json;

const API: &str = "https://gitea.invalid/api/v1/repos/decryp7/paddock/actions";
const FLAKY_ONLY: &str = include_str!("../../../tests/ci-runs/nextest-r1-flaky-only.log");
const BOTH_TRIES: &str = include_str!("../../../tests/ci-runs/nextest-r1-pass.log");
/// The composite step of ci.yml's `app` job, whose failed commands are named by `::error::failed (exit N): <command>` annotations.
const APP_GATES: &str = "Run ./.gitea/actions/app-gates";
/// A real app job capture (`tests/since-run/app-dyno-macos.log`, a nextest failure inside app-gates): its annotation lines are the
/// real shape.
const APP_CAPTURE: &str = include_str!("../../../tests/since-run/app-dyno-macos.log");
const DYNO: &str = "crate (windows-latest)";
const TEST_STEP: &str = "Tests (workspace, every feature)";
const CONFORMANCE: &str = "conformance (checks, release scripts)";

fn sha(n: u32) -> String {
    format!("{n:040x}")
}

/// A job besides the plan job: name, conclusion, log, steps (name, conclusion).
type JobSpec = (&'static str, &'static str, Option<String>, Vec<(&'static str, &'static str)>);

struct Run {
    id: u64,
    sha: String,
    event: &'static str,
    branch: &'static str,
    conclusion: &'static str,
    plan: &'static str,
    /// The jobs besides the plan job: name, conclusion, log.
    jobs: Vec<JobSpec>,
}

impl Run {
    fn train(id: u64, sha: &str, conclusion: &'static str) -> Run {
        Run {
            id,
            sha: sha.to_string(),
            event: "workflow_dispatch",
            branch: "train/2026-11-21-18",
            conclusion,
            plan: run_scope::TEST_COMPLETE,
            jobs: vec![(CONFORMANCE, "success", None, Vec::new())],
        }
    }
    fn push(id: u64, sha: &str, conclusion: &'static str) -> Run {
        Run {
            event: "push",
            branch: "main",
            ..Run::train(id, sha, conclusion)
        }
    }
    /// A failed job whose only failed step is its test step, with the log as the real CI prints it (a `Job: <key>` line first).
    fn failing(self, job: &'static str, log: Option<String>) -> Run {
        self.failing_steps(job, log, &[(TEST_STEP, "failure")])
    }
    fn failing_steps(mut self, job: &'static str, log: Option<String>, steps: &[(&'static str, &'static str)]) -> Run {
        let log = log.map(|l| if l.starts_with("Job: ") { l } else { format!("Job: crate\n{l}") });
        self.jobs.push((job, "failure", log, steps.to_vec()));
        self
    }
    fn partial(mut self, plan: &'static str) -> Run {
        self.plan = plan;
        self
    }
    fn json(&self) -> serde_json::Value {
        json!({"id": self.id, "path": format!("ci.yml@refs/heads/{}", self.branch), "event": self.event, "head_branch": self.branch,
               "head_sha": self.sha, "status": "completed", "conclusion": self.conclusion, "run_attempt": 1,
               "created_at": "2026-11-21T00:00:00Z"})
    }
    fn info(&self) -> RunInfo {
        RunInfo {
            id: self.id,
            path: format!("ci.yml@refs/heads/{}", self.branch),
            event: self.event.into(),
            status: "completed".into(),
            conclusion: self.conclusion.into(),
            head_sha: self.sha.clone(),
            head_branch: self.branch.into(),
            created_at: "2026-11-21T00:00:00Z".into(),
            run_attempt: 1,
        }
    }
}

fn some(log: &str) -> Option<String> {
    Some(log.to_string())
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
        let plan_job = r.id * 100;
        let mut jobs = vec![json!({"id": plan_job, "name": "plan (affected projects)", "conclusion": "success"})];
        for (n, (name, conclusion, log, steps)) in r.jobs.iter().enumerate() {
            let id = plan_job + 1 + n as u64;
            let steps: Vec<_> = steps
                .iter()
                .enumerate()
                .map(|(i, (s, c))| json!({"name": s, "number": i, "status": "completed", "conclusion": c}))
                .collect();
            jobs.push(json!({"id": id, "name": name, "conclusion": conclusion, "steps": steps}));
            if let Some(l) = log {
                f.log(id, l);
            }
        }
        f.json(
            &format!("{API}/runs/{}/jobs?limit=50&page=1", r.id),
            json!({"total_count": jobs.len(), "jobs": jobs}),
        );
        f.log(plan_job, r.plan);
    }
    f
}

/// The plan reached every crate (the strict interim reading: a FLAKY test of a reached crate is red).
fn reached() -> Touched {
    Touched {
        flags: ["crates".to_string()].into(),
        apps: BTreeSet::new(),
    }
}

fn touched(apps: &[&str]) -> Touched {
    Touched {
        flags: BTreeSet::new(),
        apps: apps.iter().map(|a| a.to_string()).collect(),
    }
}

fn client(f: &mut Fake) -> Client<'_> {
    let mut c = Client::new(f, "https://gitea.invalid", "decryp7/paddock", "t");
    c.backoff_ms = 0;
    c
}

fn base(f: &mut Fake, first_parent: &[String], t: Touched) -> Option<Base> {
    find_base(&mut client(f), "ci.yml", first_parent, true, &mut |_| Some(t.clone())).unwrap()
}

fn grade(f: &mut Fake, r: &Run, t: Touched) -> Outcome {
    grade_run(&mut client(f), &r.info(), true, &mut || Some(t.clone())).expect("a run that is evidence")
}

#[test]
fn a_landing_run_with_flaky_only_failures_in_untouched_targets_is_complete_green_with_fix_items_and_the_base() {
    // Train 19's 8610 shape: the landing run is red, every failed job is a FLAKY test, and the change touched other targets.
    let c = sha(2);
    let mut f = server(&[Run::train(8610, &c, "failure").failing(DYNO, some(FLAKY_ONLY))]);
    let b = base(&mut f, std::slice::from_ref(&c), touched(&["debrief"])).expect("a base");
    assert_eq!((b.sha.as_str(), b.run), (c.as_str(), 8610));
    assert_eq!(b.outcome.scope, Scope::Complete);
    assert!(
        matches!(&b.outcome.result, GradedResult::GreenWithFixItems(items) if items.len() == 1 && items[0].contains("flaky_once")),
        "{b:?}"
    );
    assert!(b.outcome.is_base() && !b.outcome.is_tag_green(), "a tag needs plain green");
}

#[test]
fn the_base_is_found_among_first_parent_commits_whatever_their_push_run_concluded() {
    // The 8554/8555 shape: the train's run 8554 is green; the pushes after it were cancelled (8341) or failed with a FLAKY only (8555);
    // a push that reused a run declared less. The newest commit has only a cancelled push and a reusing one.
    let (newest, landed, older) = (sha(3), sha(2), sha(1));
    let runs = [
        Run::push(8556, &newest, "success").partial(run_scope::TEST_PARTIAL),
        Run::push(8555, &landed, "failure").failing(DYNO, some(FLAKY_ONLY)),
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
fn a_flaky_test_in_a_touched_target_is_red() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(DYNO, some(FLAKY_ONLY));
    let mut f = server(std::slice::from_ref(&r));
    assert_eq!(
        base(&mut f, &[c], reached()),
        None,
        "the change touched dyno: its FLAKY is red, not a base"
    );
    let o = grade(&mut f, &r, reached());
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("touched")), "{o:?}");
    // Positive control: the same log is fix items when the change touched something else.
    assert!(matches!(
        grade(&mut f, &r, touched(&["debrief"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
}

#[test]
fn a_test_that_failed_both_tries_is_red_even_in_an_untouched_target() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing(DYNO, some(BOTH_TRIES));
    let mut f = server(std::slice::from_ref(&r));
    assert_eq!(base(&mut f, &[c], touched(&["debrief"])), None);
    let o = grade(&mut f, &r, touched(&["debrief"]));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("both tries")), "{o:?}");
}

#[test]
fn a_step_that_is_not_a_test_failing_is_red() {
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing_steps(
        "clippy (ubuntu-latest)",
        some("error: could not compile `paddock-x`\n"),
        &[("Clippy", "failure")],
    );
    let mut f = server(std::slice::from_ref(&r));
    assert_eq!(base(&mut f, &[c], touched(&["debrief"])), None);
    let o = grade(&mut f, &r, touched(&["debrief"]));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("not a test")), "{o:?}");
}

#[test]
fn a_step_skipped_because_of_a_failure_makes_the_run_partial_not_green() {
    // The declared job (conformance) was skipped after another job failed: the run did less than it declared.
    let c = sha(2);
    let mut r = Run::train(8610, &c, "failure").failing(DYNO, some(FLAKY_ONLY));
    r.jobs[0].1 = "skipped";
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, touched(&["debrief"]));
    assert_eq!(o.scope, Scope::Partial, "{o:?}");
    assert!(!o.is_base());
    assert_eq!(base(&mut f, &[c], touched(&["debrief"])), None);
    // Positive control: with the job run, the same failure is a base.
    r.jobs[0].1 = "success";
    let mut f = server(std::slice::from_ref(&r));
    assert!(grade(&mut f, &r, touched(&["debrief"])).is_base());
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
    let cut: String = FLAKY_ONLY
        .lines()
        .filter(|l| !l.contains("Summary"))
        .map(|l| format!("{l}\n"))
        .collect();
    for log in [Some(cut), None] {
        let r = Run::train(8610, &c, "failure").failing(DYNO, log);
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
    let o = grade_run(&mut client(&mut f), &r.info(), true, &mut || None).expect("an outcome");
    assert_eq!(o.scope, Scope::Partial);
    assert!(matches!(o.result, GradedResult::NotMeasured(_)), "{o:?}");
    assert!(!o.is_base() && !o.is_tag_green());
    assert_eq!(f.requested().len(), 0, "a cancelled run costs no call");
}

#[test]
fn an_interim_strict_flaky_in_any_reached_crate_is_red_and_in_an_unreached_one_is_fix_items() {
    // User decision 2026-11-23: any crate or app the plan reached makes a FLAKY red, until module edges land.
    let c = sha(2);
    let r = Run::train(8610, &c, "failure").failing("crate (windows-latest)", some(FLAKY_ONLY));
    let mut f = server(std::slice::from_ref(&r));
    assert!(matches!(grade(&mut f, &r, reached()).result, GradedResult::Red(_)));
    assert!(matches!(
        grade(&mut f, &r, touched(&["debrief"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
}

#[test]
fn a_whole_tree_run_is_complete_whatever_its_base_and_ends_the_chain() {
    // A nightly (schedule) or a plan of `all` declares the whole scope without any base: complete, and a base for the next push.
    let c = sha(2);
    let mut nightly = Run::train(8800, &c, "success");
    nightly.event = "schedule";
    nightly.branch = "main";
    let mut f = server(std::slice::from_ref(&nightly));
    let o = grade(&mut f, &nightly, touched(&[]));
    assert_eq!((o.scope, o.result), (Scope::Complete, GradedResult::Green));
    assert_eq!(base(&mut f, std::slice::from_ref(&c), touched(&[])).map(|b| b.run), Some(8800));
}

#[test]
fn the_search_costs_pages_plus_the_candidate_runs_not_a_call_per_commit() {
    // 200 first-parent commits, 120 runs (three pages), only the last of them a complete green one: the calls are the three pages,
    // plus a job list and a plan log for each run that is evidence (cancelled runs cost nothing), whatever the number of commits.
    let commits: Vec<String> = (1..=200u32).rev().map(sha).collect();
    let mut runs: Vec<Run> = (0..119u32)
        .map(|i| Run::push(9000 - u64::from(i), &commits[i as usize], "cancelled"))
        .collect();
    runs.push(Run::train(8000, &commits[119], "success"));
    let mut f = server(&runs);
    let b = base(&mut f, &commits, touched(&[])).expect("the green one is found");
    assert_eq!(b.sha, commits[119]);
    let calls = f.requested().len();
    assert_eq!(calls, 3 + 2, "three run pages, one job list and one plan log: {:?}", f.requested());
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
fn a_flaky_job_with_another_failed_step_is_red_and_one_with_no_listed_failed_step_is_not_measured() {
    // ER-33: the log shows a FLAKY test, but the API lists a second failed step in the same job (here a lint step after the tests).
    let c = sha(2);
    let both = [(TEST_STEP, "failure"), ("Clippy", "failure")];
    let r = Run::train(8610, &c, "failure").failing_steps(DYNO, some(FLAKY_ONLY), &both);
    let mut f = server(std::slice::from_ref(&r));
    let o = grade(&mut f, &r, touched(&["debrief"]));
    assert!(
        matches!(&o.result, GradedResult::Red(why) if why.contains("Clippy") && why.contains("not a test step")),
        "{o:?}"
    );
    assert_eq!(base(&mut f, std::slice::from_ref(&c), touched(&["debrief"])), None);
    // The step list is the only thing that differs from the base case: with the test step alone it is fix items.
    let ok = Run::train(8610, &c, "failure").failing_steps(DYNO, some(FLAKY_ONLY), &[("Clippy", "success"), (TEST_STEP, "failure")]);
    let mut f = server(std::slice::from_ref(&ok));
    assert!(matches!(
        grade(&mut f, &ok, touched(&["debrief"])).result,
        GradedResult::GreenWithFixItems(_)
    ));
    // A failed job whose steps list no failure cannot be shown to be test-only.
    let none = Run::train(8610, &c, "failure").failing_steps(DYNO, some(FLAKY_ONLY), &[(TEST_STEP, "success")]);
    let mut f = server(std::slice::from_ref(&none));
    let o = grade(&mut f, &none, touched(&["debrief"]));
    assert!(matches!(o.result, GradedResult::NotMeasured(_)), "{o:?}");
}

/// The annotation lines of the real app-gates capture, and the log of an app job: the FLAKY-only nextest capture, then these lines.
fn app_log(annotation: impl Fn(&str) -> String, completed: usize) -> String {
    let plain = |l: &str| l.split_once("Z ").map_or(l.to_string(), |(_, r)| r.to_string());
    let mut out = format!("Job: app\n{FLAKY_ONLY}");
    let mut seen = 0;
    for l in APP_CAPTURE.lines().map(plain) {
        if l.contains("::error::failed (exit") {
            out.push_str(&format!("{}\n", annotation(&l)));
        } else if l.starts_with("##[error]Process completed") && seen < completed {
            seen += 1;
            out.push_str(&format!("{l}\n"));
        }
    }
    out
}

#[test]
fn a_flaky_app_job_with_a_lint_annotation_inside_app_gates_is_red_and_an_unreadable_composite_failure_is_not_measured() {
    // ER-44: app-gates is one step in the API; which command failed inside it is only in the `::error::failed` annotations.
    let c = sha(2);
    let app = "app dyno (ubuntu-latest)";
    let steps = [(APP_GATES, "failure")];
    let grade_log = |log: String| {
        let r = Run::train(8610, &c, "failure").failing_steps(app, Some(log), &steps);
        let mut f = server(std::slice::from_ref(&r));
        grade(&mut f, &r, touched(&["debrief"]))
    };
    // Control: the one failed command is the nextest run (the real annotation), and the app is not reached: fix items.
    assert!(matches!(
        grade_log(app_log(|l| l.to_string(), 2)).result,
        GradedResult::GreenWithFixItems(_)
    ));
    // A lint annotation (the real line with its command replaced) is red, though the log also shows only FLAKY tests.
    let lint = |_: &str| "::error::failed (exit 101): cargo clippy --workspace --all-targets -- -D warnings".to_string();
    let o = grade_log(app_log(lint, 2));
    assert!(matches!(&o.result, GradedResult::Red(why) if why.contains("clippy")), "{o:?}");
    // A composite failure that does not show the failed command and the composite is not measured.
    let o = grade_log(app_log(|l| l.to_string(), 1));
    assert!(matches!(o.result, GradedResult::NotMeasured(_)), "{o:?}");
}
