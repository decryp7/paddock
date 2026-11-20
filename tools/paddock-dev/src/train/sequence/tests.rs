use super::*;
use crate::plan_only::Kind;
use serde_json::json;
use std::cell::Cell;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const A11Y: &str = "[webkit] › e2e/a11y.e2e.ts:6:7 › main screen has no serious accessibility violations (dark)";
const CHAT: &str = "[webkit] › e2e/ai-chat.e2e.ts:130:7 › AI chat › a long conversation scrolls";

/// One dispatch: workflow, inputs, run id.
type Dispatch = (String, Vec<(String, String)>, u64);

/// A table instead of Gitea. Runs are numbered from 9000 as they are dispatched. A dispatched run's facts say how `results` has it
/// end (success by default) and the shared reader's verdict is red until its workflow has been dispatched, then `after`.
struct Table {
    now: Cell<Duration>,
    next: u64,
    dispatched: Vec<Dispatch>,
    results: BTreeMap<String, RunResult>,
    ends: BTreeMap<String, RunEnd>,
    after: BTreeMap<String, Verdict>,
    verdict_now: BTreeMap<String, Verdict>,
    disk: Result<(), String>,
    dispatch_error: Option<String>,
    triage: BTreeMap<u64, Result<Value, String>>,
    runs: BTreeMap<u64, RunFacts>,
    waited: Vec<(u64, Duration)>,
    triaged: Vec<u64>,
    wait_took: Duration,
    ancestor: bool,
    reach: BTreeMap<String, Result<Reach, String>>,
    reach_asked: Vec<(String, String)>,
    landables: BTreeMap<String, Result<Landable, String>>,
    ran: BTreeMap<u64, Result<Value, String>>,
    kinds: BTreeMap<String, Kind>,
    stage1: Result<Option<Vec<(String, String)>>, String>,
    /// The runs the shared reader counts as a full green run (a defect a rung 3 must catch).
    counted_full: Vec<u64>,
    /// Dispatched runs (by id) that end red, whatever their workflow's default.
    fail_runs: Vec<u64>,
    /// The runs cancelled, and whether a cancel fails.
    cancelled: Vec<u64>,
    cancel_error: Option<String>,
    /// The commits of the runs of the fix workflow newer than the red run (the server's rounds).
    earlier_heads: Vec<String>,
}

impl Table {
    fn new() -> Table {
        // The since-run of the default plan: a finished, red ci.yml run.
        let mut runs = BTreeMap::new();
        runs.insert(8300, facts("ci.yml", SHA, RunResult::Failure));
        // The red run's own triage (the ladder reads it for the jobs that failed): one FLAKY row in a job with no test list.
        let mut triage = BTreeMap::new();
        triage.insert(8300, Ok(flaky_doc()));
        Table {
            now: Cell::new(Duration::ZERO),
            next: 9000,
            dispatched: Vec::new(),
            results: BTreeMap::new(),
            ends: BTreeMap::new(),
            after: BTreeMap::new(),
            verdict_now: BTreeMap::new(),
            disk: Ok(()),
            dispatch_error: None,
            triage,
            runs,
            waited: Vec::new(),
            triaged: Vec::new(),
            wait_took: Duration::from_secs(60),
            ancestor: true,
            reach: BTreeMap::new(),
            reach_asked: Vec::new(),
            landables: BTreeMap::new(),
            ran: BTreeMap::new(),
            kinds: BTreeMap::new(),
            stage1: Ok(None),
            counted_full: Vec::new(),
            fail_runs: Vec::new(),
            cancelled: Vec::new(),
            cancel_error: None,
            earlier_heads: Vec::new(),
        }
    }
    fn result(mut self, wf: &str, r: RunResult) -> Table {
        self.results.insert(wf.into(), r);
        self
    }
    fn workflows(&self) -> Vec<&str> {
        self.dispatched.iter().map(|d| d.0.as_str()).collect()
    }
}

fn facts(wf: &str, sha: &str, result: RunResult) -> RunFacts {
    RunFacts {
        workflow: wf.into(),
        git_ref: "refs/heads/train/x".into(),
        head_sha: sha.into(),
        result,
    }
}

impl Ports for Table {
    fn elapsed(&self) -> Duration {
        self.now.get()
    }
    fn disk_check(&mut self) -> Result<(), String> {
        self.disk.clone()
    }
    fn dispatch(&mut self, workflow: &str, inputs: &[(String, String)], _wait: Duration) -> Result<u64, String> {
        if let Some(e) = &self.dispatch_error {
            return Err(e.clone());
        }
        self.next += 1;
        self.dispatched.push((workflow.into(), inputs.to_vec(), self.next));
        let mut result = self.results.get(workflow).cloned().unwrap_or(RunResult::Success);
        if self.fail_runs.contains(&self.next) {
            result = RunResult::Failure;
        }
        self.runs.insert(self.next, facts(workflow, SHA, result));
        Ok(self.next)
    }
    fn wait_run(&mut self, run: u64, max: Duration) -> RunEnd {
        self.waited.push((run, max));
        self.now.set(self.now.get() + self.wait_took);
        let wf = self.dispatched.iter().find(|d| d.2 == run).expect("a dispatched run").0.clone();
        self.ends.get(&wf).cloned().unwrap_or(RunEnd::Finished)
    }
    fn run_facts(&mut self, run: u64) -> Result<Option<RunFacts>, String> {
        Ok(self.runs.get(&run).cloned())
    }
    fn ci_verdict(&mut self, workflow: &str, _sha: &str) -> Verdict {
        // A defective reader: counts the stage 1 run as a full green run, but only once that run was dispatched.
        if let Some(&run) = self.counted_full.first()
            && self.dispatched.iter().any(|d| d.1.iter().any(|(k, _)| k == "stage"))
        {
            return Verdict::Green { run };
        }
        // A full run: no since_run and no stage input (stage 1 is a rung, not a full run).
        if self
            .dispatched
            .iter()
            .any(|d| d.0 == workflow && !d.1.iter().any(|(k, _)| k == "since_run" || k == "stage"))
        {
            return self.after.get(workflow).cloned().unwrap_or(Verdict::Green { run: 7 });
        }
        self.verdict_now.get(workflow).cloned().unwrap_or(Verdict::Red {
            run: None,
            reason: "no full run".into(),
        })
    }
    fn landable(&mut self, workflow: &str, _: &str) -> Result<Landable, String> {
        self.landables
            .get(workflow)
            .cloned()
            .unwrap_or(Err("no landable in the table".into()))
    }
    fn triage(&mut self, run: u64) -> Result<Value, String> {
        self.triaged.push(run);
        self.triage
            .get(&run)
            .cloned()
            .unwrap_or_else(|| Err("no triage in the table".into()))
    }
    fn cancel_run(&mut self, run: u64) -> Result<(), String> {
        if let Some(e) = &self.cancel_error {
            return Err(e.clone());
        }
        self.cancelled.push(run);
        Ok(())
    }
    fn runs_after(&mut self, _: &str, _: u64) -> Result<Vec<String>, String> {
        Ok(self.earlier_heads.clone())
    }
    fn triage_ran(&mut self, run: u64, _only: Option<&[String]>) -> Result<Value, String> {
        self.triaged.push(run);
        self.ran.get(&run).cloned().unwrap_or_else(|| Err("no counts in the table".into()))
    }
    fn job_kind(&mut self, _: &str, job: &str) -> Option<Kind> {
        self.kinds.get(job).copied()
    }
    fn stage1(&mut self, _: &str) -> Result<Option<Vec<(String, String)>>, String> {
        self.stage1.clone()
    }
    fn is_ancestor(&mut self, _: &str, _: &str) -> Result<bool, String> {
        Ok(self.ancestor)
    }
    fn reach(&mut self, app: &str, from: &str) -> Result<Reach, String> {
        self.reach_asked.push((app.into(), from.into()));
        self.reach.get(app).cloned().unwrap_or(Ok(Reach {
            reached: false,
            diff: "docs".into(),
        }))
    }
}

fn plan(workflow: FixWorkflow, round: Round) -> Plan {
    Plan {
        branch: "train/x".into(),
        sha: SHA.into(),
        workflow,
        round,
        accepted: vec![Accepted::parse("apps/debrief/app/e2e/a11y.e2e.ts:6:7").unwrap()],
        reuse: BTreeMap::new(),
        max: Duration::from_secs(14 * 3600),
        ladder: Ladder::off(),
    }
}

fn dispatch_plan() -> Plan {
    plan(FixWorkflow::Ci, Round::Dispatch(8300))
}

fn flaky_doc() -> Value {
    json!({"ok": true, "counts": {"flaky": 1}, "skipped_jobs": 0, "rows": [
        {"job": "app debrief", "test": A11Y, "whole_job": false, "status": "flaky"}]})
}

fn new_doc() -> Value {
    json!({"ok": true, "counts": {"flaky": 1, "new": 1}, "skipped_jobs": 0, "rows": [
        {"job": "app debrief", "test": A11Y, "whole_job": false, "status": "flaky"},
        {"job": "j", "test": CHAT, "whole_job": false, "status": "new"}]})
}

fn go(p: &Plan, t: &mut Table) -> (Result<Done, Stop>, String) {
    let mut log = Vec::new();
    let r = release(p, t, &mut log);
    (r, String::from_utf8(log).unwrap())
}

fn since(t: &Table, wf: &str, n: usize) -> bool {
    t.dispatched
        .iter()
        .filter(|d| d.0 == wf && d.1.iter().any(|(k, _)| k == "since_run"))
        .count()
        == n
}

#[test]
fn a_green_run_dispatches_the_fix_round_then_the_full_ci_and_debrief_and_every_release_workflow() {
    let mut t = Table::new();
    let (r, log) = go(&dispatch_plan(), &mut t);
    let done = r.unwrap();
    assert_eq!(
        t.workflows(),
        [
            "ci.yml",
            "ci.yml",
            "debrief.yml",
            "dyno-release.yml",
            "pitcrew-release.yml",
            "pitwall-release.yml",
            "spotter-release.yml",
            "shell-demo-release.yml",
            "workbench-demo-release.yml"
        ]
    );
    let inputs = |i: usize| t.dispatched[i].1.clone();
    let kv = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(inputs(0), vec![kv("webkit", "true"), kv("since_run", "8300")]);
    assert_eq!(inputs(1), vec![kv("webkit", "true")], "the full run has no since_run (ER-10)");
    assert_eq!(inputs(2), vec![kv("bundles", "true")]);
    assert_eq!(inputs(3), vec![kv("gates", "false")]);
    assert!(inputs(7).is_empty());
    assert_eq!(done.steps.len(), 9);
    assert!(log.contains("release sequence green on 012345678"), "{log}");
}

#[test]
fn a_debrief_round_also_needs_the_full_ci_and_debrief_on_the_head() {
    let mut t = Table::new();
    t.runs.insert(8301, facts("debrief.yml", SHA, RunResult::Failure));
    t.triage.insert(8301, Ok(flaky_doc()));
    let (r, _) = go(&plan(FixWorkflow::Debrief, Round::Dispatch(8301)), &mut t);
    r.unwrap();
    assert_eq!(&t.workflows()[..3], ["debrief.yml", "ci.yml", "debrief.yml"]);
    assert!(since(&t, "debrief.yml", 1) && since(&t, "ci.yml", 0));
}

/// ER-10: the full run is decided by the shared reader, so a full run that is not green by it stops, whatever the run's own
/// conclusion says.
#[test]
fn a_full_run_that_the_shared_reader_calls_red_or_not_measured_stops_the_sequence() {
    for v in [
        Verdict::Red {
            run: Some(9002),
            reason: "it failed".into(),
        },
        Verdict::Red {
            run: None,
            reason: "only partial runs".into(),
        },
        Verdict::NotMeasured("GITEA_TOKEN".into()),
    ] {
        let mut t = Table::new();
        t.after.insert("ci.yml".into(), v);
        let (r, _) = go(&dispatch_plan(), &mut t);
        let stop = r.unwrap_err();
        assert!(
            stop.cause.contains("full ci.yml run") && stop.cause.contains("ci verdict"),
            "{stop:?}"
        );
        assert_eq!(t.workflows(), ["ci.yml", "ci.yml"], "nothing after the full run");
    }
}

#[test]
fn a_green_verdict_that_already_exists_is_used_without_a_dispatch() {
    let mut t = Table::new();
    t.verdict_now.insert("ci.yml".into(), Verdict::Green { run: 8999 });
    let (r, _) = go(&dispatch_plan(), &mut t);
    let done = r.unwrap();
    assert!(done.steps.contains(&("ci.yml".to_string(), Source::Verdict(8999))));
    assert_eq!(t.workflows().iter().filter(|w| **w == "ci.yml").count(), 1, "only the fix round");
}

#[test]
fn it_stops_at_the_first_red_release_run_and_dispatches_nothing_after_it() {
    let mut t = Table::new().result("pitcrew-release.yml", RunResult::Failure);
    let (r, _) = go(&dispatch_plan(), &mut t);
    let stop = r.unwrap_err();
    assert_eq!(t.workflows().last(), Some(&"pitcrew-release.yml"));
    assert!(
        stop.cause.contains("pitcrew-release.yml run") && stop.cause.contains("is red"),
        "{stop:?}"
    );
    assert!(stop.annotation().starts_with("::error::train release: ") && stop.annotation().contains("Next step: "));
    assert_eq!(stop.annotation().lines().count(), 1);
}

/// A run that ends `cancelled` (or any conclusion but success and failure) is neither green nor red: it stops, and is never
/// taken for a pass.
#[test]
fn a_cancelled_or_unknown_conclusion_stops_every_kind_of_run() {
    let other = || RunResult::Other("cancelled".into());
    // a release run
    let mut t = Table::new().result("dyno-release.yml", other());
    assert!(go(&dispatch_plan(), &mut t).0.unwrap_err().cause.contains("neither green nor red"));
    // the fix round's own run
    let mut t = Table::new().result("ci.yml", other());
    assert!(go(&dispatch_plan(), &mut t).0.unwrap_err().cause.contains("neither green nor red"));
    assert!(t.triaged.is_empty());
    // a since-run that was cancelled is not a red run to repeat
    let mut t = Table::new();
    t.runs.insert(8300, facts("ci.yml", SHA, other()));
    assert!(go(&dispatch_plan(), &mut t).0.unwrap_err().cause.contains("neither green nor red"));
    assert!(t.dispatched.is_empty());
}

#[test]
fn the_since_run_must_be_a_finished_red_run_of_the_same_workflow() {
    for (f, why) in [
        (facts("ci.yml", SHA, RunResult::Success), "it is green"),
        (facts("ci.yml", SHA, RunResult::Running("in_progress".into())), "still in_progress"),
        (facts("dyno-release.yml", SHA, RunResult::Failure), "not of ci.yml"),
    ] {
        let mut t = Table::new();
        t.runs.insert(8300, f);
        let (r, _) = go(&dispatch_plan(), &mut t);
        assert!(r.unwrap_err().cause.contains(why), "{why}");
        assert!(t.dispatched.is_empty(), "{why}");
    }
    let mut t = Table::new();
    t.runs.clear();
    assert!(go(&dispatch_plan(), &mut t).0.unwrap_err().cause.contains("no such run"));
}

/// The positive control of the round: a red fix round whose rows are accepted flaky tests goes on (to the full run).
#[test]
fn a_red_fix_round_with_only_accepted_flaky_tests_goes_on() {
    let mut t = Table::new().result("ci.yml", RunResult::Failure);
    t.triage.insert(9001, Ok(flaky_doc()));
    // the full ci.yml also ends red by its conclusion, but the shared reader says green (set by default)
    let (r, log) = go(&dispatch_plan(), &mut t);
    let done = r.unwrap();
    assert_eq!(done.steps[0], ("ci.yml".to_string(), Source::Judged(9001)));
    assert!(log.contains("PASS: 1 failing row(s)"), "{log}");
}

/// The negative control: the same round with a NEW failure beside it stops and dispatches nothing more.
#[test]
fn a_red_fix_round_with_a_new_failure_stops_and_dispatches_nothing_more() {
    let mut t = Table::new().result("ci.yml", RunResult::Failure);
    t.triage.insert(9001, Ok(new_doc()));
    let (r, _) = go(&dispatch_plan(), &mut t);
    let stop = r.unwrap_err();
    assert_eq!(t.workflows(), ["ci.yml"]);
    assert!(stop.cause.contains("new: ") && stop.cause.contains("ai-chat"), "{stop:?}");
}

#[test]
fn a_red_fix_round_whose_triage_fails_stops() {
    let mut t = Table::new().result("ci.yml", RunResult::Failure);
    t.triage.insert(9001, Err("the network is down".into()));
    let (r, _) = go(&dispatch_plan(), &mut t);
    assert!(r.unwrap_err().cause.contains("its triage failed: the network is down"));
    assert_eq!(t.workflows(), ["ci.yml"]);
}

#[test]
fn a_run_that_did_not_finish_is_never_triaged_or_taken_for_green() {
    let mut t = Table::new();
    t.ends.insert("ci.yml".into(), RunEnd::Unfinished("still running after 6h".into()));
    let (r, _) = go(&dispatch_plan(), &mut t);
    assert!(r.unwrap_err().cause.contains("did not finish: still running after 6h"));
    assert!(t.triaged.is_empty());
    assert_eq!(t.workflows().len(), 1);
}

#[test]
fn a_passed_round_is_not_dispatched_again_but_it_is_judged_and_checked() {
    let p = || plan(FixWorkflow::Debrief, Round::Passed(8307));
    let mut t = Table::new();
    t.runs.insert(8307, facts("debrief.yml", SHA, RunResult::Failure));
    t.triage.insert(8307, Ok(flaky_doc()));
    let (r, log) = go(&p(), &mut t);
    assert_eq!(r.unwrap().steps[0], ("debrief.yml".to_string(), Source::Judged(8307)));
    assert!(!since(&t, "debrief.yml", 1) && log.contains("not dispatched again"), "{log}");
    // a green one needs no triage
    let mut t = Table::new();
    t.runs.insert(8307, facts("debrief.yml", SHA, RunResult::Success));
    go(&p(), &mut t).0.unwrap();
    assert!(t.triaged.is_empty());
    // the claim is checked: no run, another workflow, another commit, not finished, a new failure, a cancel
    let checks: Vec<(Option<RunFacts>, &str)> = vec![
        (None, "no such run"),
        (Some(facts("ci.yml", SHA, RunResult::Success)), "not of debrief.yml"),
        (
            Some(facts("debrief.yml", "ffffffffffffffff", RunResult::Success)),
            "it tested fffffffff",
        ),
        (
            Some(facts("debrief.yml", SHA, RunResult::Running("in_progress".into()))),
            "still in_progress",
        ),
        (
            Some(facts("debrief.yml", SHA, RunResult::Other("cancelled".into()))),
            "neither green nor red",
        ),
    ];
    for (f, why) in checks {
        let mut t = Table::new();
        if let Some(f) = f {
            t.runs.insert(8307, f);
        }
        assert!(go(&p(), &mut t).0.unwrap_err().cause.contains(why), "{why}");
        assert!(t.dispatched.is_empty());
    }
    let mut t = Table::new();
    t.runs.insert(8307, facts("debrief.yml", SHA, RunResult::Failure));
    t.triage.insert(8307, Ok(new_doc()));
    assert!(go(&p(), &mut t).0.is_err());
    assert!(t.dispatched.is_empty());
}

fn reuse_plan(run: u64) -> Plan {
    let mut p = dispatch_plan();
    p.reuse.insert("dyno-release.yml".into(), run);
    p
}

/// The positive control: a green run of this train's earlier head, an ancestor, with the app not reached, is reused and its diff logged.
#[test]
fn a_reused_run_of_an_unchanged_app_is_not_dispatched_again_and_its_diff_is_logged() {
    let mut t = Table::new();
    t.runs.insert(8302, facts("dyno-release.yml", "aaaaaaaaaaaa", RunResult::Success));
    let (r, log) = go(&reuse_plan(8302), &mut t);
    let done = r.unwrap();
    assert!(!t.workflows().contains(&"dyno-release.yml"));
    assert!(done.steps.contains(&("dyno-release.yml".to_string(), Source::Reused(8302))));
    assert_eq!(t.reach_asked, vec![("dyno".to_string(), "aaaaaaaaaaaa".to_string())]);
    assert!(
        log.contains("plan aaaaaaaaa..012345678 reaches: docs") && log.contains("reusing green run 8302"),
        "{log}"
    );
}

#[test]
fn a_reuse_is_refused_for_an_unrelated_tree_a_reached_app_another_ref_or_a_run_that_is_not_green() {
    let green = || facts("dyno-release.yml", "aaaaaaaaaaaa", RunResult::Success);
    type Setup = Box<dyn Fn(&mut Table)>;
    let cases: Vec<(&str, Setup)> = vec![
        ("unrelated tree", Box::new(|t| t.ancestor = false)),
        (
            "its tree changed",
            Box::new(|t| {
                t.reach.insert(
                    "dyno".into(),
                    Ok(Reach {
                        reached: true,
                        diff: "app:dyno".into(),
                    }),
                );
            }),
        ),
        (
            "could not say what",
            Box::new(|t| {
                t.reach.insert("dyno".into(), Err("planner failed".into()));
            }),
        ),
        (
            "not on refs/heads/train/x",
            Box::new(|t| {
                t.runs.get_mut(&8302).unwrap().git_ref = "refs/heads/main".into();
            }),
        ),
        (
            "did not succeed",
            Box::new(|t| {
                t.runs.get_mut(&8302).unwrap().result = RunResult::Failure;
            }),
        ),
        (
            "it is a run of pitwall-release.yml",
            Box::new(|t| {
                t.runs.get_mut(&8302).unwrap().workflow = "pitwall-release.yml".into();
            }),
        ),
        (
            "no such run",
            Box::new(|t| {
                t.runs.remove(&8302);
            }),
        ),
    ];
    for (why, setup) in cases {
        let mut t = Table::new();
        t.runs.insert(8302, green());
        setup(&mut t);
        let (r, _) = go(&reuse_plan(8302), &mut t);
        assert!(r.unwrap_err().cause.contains(why), "{why}");
        assert!(!t.workflows().contains(&"pitcrew-release.yml"), "{why}: nothing after the refusal");
    }
    let mut p = dispatch_plan();
    p.reuse.insert("dyno.yml".into(), 1);
    let mut t = Table::new();
    assert!(go(&p, &mut t).0.unwrap_err().cause.contains("not a release workflow"));
    assert!(t.dispatched.is_empty(), "a bad argument dispatches nothing");
}

#[test]
fn a_refused_disk_check_dispatches_nothing() {
    let mut t = Table::new();
    t.disk = Err("only 10 GiB free".into());
    let (r, _) = go(&dispatch_plan(), &mut t);
    let stop = r.unwrap_err();
    assert!(
        stop.cause.contains("only 10 GiB free") && stop.next.contains("./dev disk"),
        "{stop:?}"
    );
    assert!(t.dispatched.is_empty());
}

#[test]
fn a_failed_dispatch_stops() {
    let mut t = Table::new();
    t.dispatch_error = Some("401".into());
    let (r, _) = go(&dispatch_plan(), &mut t);
    assert!(r.unwrap_err().cause.contains("ci.yml was not dispatched: 401"));
}

/// Every wait is bounded: a run is waited for at most what is left of --max, and nothing is dispatched once it is used up.
#[test]
fn the_waits_are_bounded_by_max_and_the_run_cap() {
    let mut p = dispatch_plan();
    p.max = Duration::from_secs(8 * 3600);
    let mut t = Table::new();
    t.wait_took = Duration::from_secs(3 * 3600);
    let (r, _) = go(&p, &mut t);
    assert_eq!(t.waited[0].1, RUN_CAP);
    assert_eq!(t.waited[1].1, Duration::from_secs(5 * 3600));
    assert_eq!(t.waited[2].1, Duration::from_secs(2 * 3600));
    assert!(t.waited.iter().all(|(_, m)| *m <= RUN_CAP));
    assert!(r.unwrap_err().cause.contains("used up before"));
    assert_eq!(t.workflows().len(), 3);
}

#[test]
fn the_round_and_workflow_arguments_are_parsed_strictly() {
    assert_eq!(Round::parse("8300"), Ok(Round::Dispatch(8300)));
    assert_eq!(Round::parse("passed:8307"), Ok(Round::Passed(8307)));
    for bad in ["", "passed:", "passed:x", "x", "-1"] {
        assert!(Round::parse(bad).is_err(), "{bad}");
    }
    assert_eq!(FixWorkflow::parse("ci.yml"), Ok(FixWorkflow::Ci));
    assert!(FixWorkflow::parse("dyno.yml").is_err());
    assert_eq!(app_of("dyno-release.yml"), "dyno");
    assert_eq!(app_of("shell-demo-release.yml"), "shell-demo");
}

fn landable_full() -> Landable {
    Landable::for_test(9002, 1, &["accepted FLAKY (e2e): a test"])
}

/// ER-12: a full run that is red only by accepted FLAKY tests is accepted through the train-only `Landable`, every row logged.
#[test]
fn a_full_run_that_is_red_only_by_accepted_flaky_tests_is_accepted_and_logged() {
    let mut t = Table::new();
    t.after.insert(
        "ci.yml".into(),
        Verdict::Red {
            run: Some(9002),
            reason: "failure".into(),
        },
    );
    t.landables.insert("ci.yml".into(), Ok(landable_full()));
    let (r, log) = go(&dispatch_plan(), &mut t);
    let done = r.unwrap();
    assert!(done.steps.contains(&("ci.yml".to_string(), Source::Landable(9002))));
    assert!(
        log.contains("landing on a full run 9002 with 1 accepted FLAKY") && log.contains("accepted FLAKY (e2e): a test"),
        "{log}"
    );
    // and one that is not landable stops
    let mut t = Table::new();
    t.after.insert(
        "ci.yml".into(),
        Verdict::Red {
            run: Some(9002),
            reason: "failure".into(),
        },
    );
    t.landables.insert("ci.yml".into(), Err("a NEW failure".into()));
    let stop = go(&dispatch_plan(), &mut t).0.unwrap_err();
    assert!(stop.cause.contains("not landable: a NEW failure"), "{stop:?}");
}

#[test]
fn a_red_full_run_that_already_exists_and_is_landable_is_used_without_a_dispatch() {
    let mut t = Table::new();
    t.verdict_now.insert(
        "ci.yml".into(),
        Verdict::Red {
            run: Some(8999),
            reason: "failure".into(),
        },
    );
    t.landables.insert("ci.yml".into(), Ok(landable_full()));
    let done = go(&dispatch_plan(), &mut t).0.unwrap();
    assert!(done.steps.contains(&("ci.yml".to_string(), Source::Landable(9002))));
    assert_eq!(t.workflows().iter().filter(|w| **w == "ci.yml").count(), 1, "only the fix round");
}

#[test]
fn a_since_run_or_passed_run_on_another_ref_is_refused() {
    let mut t = Table::new();
    t.runs.get_mut(&8300).unwrap().git_ref = "refs/heads/main".into();
    assert!(
        go(&dispatch_plan(), &mut t)
            .0
            .unwrap_err()
            .cause
            .contains("not on this train's ref")
    );
    assert!(t.dispatched.is_empty());
    let mut t = Table::new();
    let mut f = facts("debrief.yml", SHA, RunResult::Success);
    f.git_ref = "refs/heads/other".into();
    t.runs.insert(8307, f);
    assert!(
        go(&plan(FixWorkflow::Debrief, Round::Passed(8307)), &mut t)
            .0
            .unwrap_err()
            .cause
            .contains("not on this train's ref")
    );
}

// ---- the fix ladder ----

const CRATE: &str = "crate (ubuntu-latest)";
const CONFORMANCE: &str = "conformance (checks, release scripts)";
const NX: &str = "paddock-service per_machine::tests::every_adopting_app_is_in_sync";

/// The red run's triage: one NEW nextest failure in the crate job and one whole-job failure elsewhere.
fn red_doc() -> Value {
    json!({"ok": true, "counts": {"new": 1, "failed": 1, "flaky": 0, "already_failing": 0, "unknown": 0}, "skipped_jobs": 0, "rows": [
        {"job": CRATE, "test": NX, "whole_job": false, "status": "new"},
        {"job": CONFORMANCE, "test": null, "whole_job": true, "status": "failed"}]})
}

/// What `ci triage --ran` says of a rung run: the crate job ran `n` tests.
fn ran_doc(n: u64, rows: Value) -> Value {
    let mut d = json!({"ok": true, "counts": {"new": 0, "failed": 0, "flaky": 0, "already_failing": 0, "unknown": 0}, "skipped_jobs": 0, "rows": rows,
        "ran": {"total": n, "jobs": [{"job": CRATE, "tests_run": n}, {"job": "plan (affected projects)", "tests_run": 0}]}});
    if d["rows"].as_array().is_some_and(|r| !r.is_empty()) {
        d["counts"]["new"] = json!(d["rows"].as_array().map_or(0, Vec::len));
    }
    d
}

fn ladder_plan() -> Plan {
    let mut p = dispatch_plan();
    p.ladder = Ladder { max_rounds: 3 };
    p
}

fn ladder_table() -> Table {
    let mut t = Table::new();
    t.triage.insert(8300, Ok(red_doc()));
    t.kinds.insert(CRATE.into(), Kind::Nextest);
    // The first dispatched run is 9001: the tests-only run.
    t.ran.insert(9001, Ok(ran_doc(1, json!([]))));
    t
}

fn only_dispatch(t: &Table) -> Option<&Dispatch> {
    t.dispatched.iter().find(|d| d.1.iter().any(|(k, _)| k == "only_tests"))
}

#[test]
fn rung_1_runs_the_named_failing_tests_only_then_rung_2_and_the_full_run_decide_the_landing() {
    let mut t = ladder_table();
    let (r, log) = go(&ladder_plan(), &mut t);
    let done = r.unwrap();
    let d = only_dispatch(&t).expect("the tests-only run was dispatched");
    let only = d.1.iter().find(|(k, _)| k == "only_tests").unwrap();
    // The input carries the failing test of the crate job, and not the whole-job failure of the other job.
    assert_eq!(only.1, json!({ CRATE: [NX] }).to_string());
    assert!(
        d.1.contains(&("since_run".into(), "8300".into())),
        "rung 1 repeats the red run's failures"
    );
    // The order of the ladder: the tests-only run, the whole-jobs round, the full runs.
    assert_eq!(&t.workflows()[..3], ["ci.yml", "ci.yml", "ci.yml"]);
    assert_eq!(
        done.rungs
            .iter()
            .map(|(r, o)| (r.number(), matches!(o, RungOutcome::Green(_))))
            .collect::<Vec<_>>(),
        [(1, true), (2, true), (3, false)]
    );
    // Each rung names its tests, its run and its duration in the log.
    assert!(
        log.contains("rung 1 (failed tests only), round 1/3: 1 test(s) in 1 job(s) of run 8300"),
        "{log}"
    );
    assert!(log.contains(&format!("rung 1:   {CRATE}: {NX}")), "{log}");
    assert!(
        log.contains("rung 1 (failed tests only): green, run 9001: 1 named test(s) ran, after"),
        "{log}"
    );
    assert!(log.contains("rung 3 (stage 1 on every OS): skipped"), "{log}");
    // Only the full runs of rung 4 are what a landing reads: the rung steps are not full evidence.
    let full: Vec<_> = done.steps.iter().filter(|(_, s)| !s.is_rung()).collect();
    assert!(full.iter().any(|(w, s)| w == "ci.yml" && matches!(s, Source::Verdict(_))));
    assert!(done.steps.iter().any(|(_, s)| matches!(s, Source::TestsOnly(9001))));
}

#[test]
fn a_rung_that_ran_no_test_is_not_measured_and_stops_the_ladder_whatever_the_run_says() {
    // Positive control: the run is green, but its filter matched nothing (nextest --no-tests=warn).
    let mut t = ladder_table();
    t.ran.insert(9001, Ok(ran_doc(0, json!([]))));
    let (r, _) = go(&ladder_plan(), &mut t);
    let stop = r.unwrap_err();
    assert!(
        stop.cause.contains("NOT MEASURED") && stop.cause.contains("matched nothing"),
        "{}",
        stop.cause
    );
    assert_eq!(t.workflows(), ["ci.yml"], "nothing after the unmeasured rung ran");
    // Fewer tests than ids is as unmeasured.
    let mut t = ladder_table();
    t.kinds.insert("other job".into(), Kind::Nextest);
    let mut doc = red_doc();
    doc["rows"]
        .as_array_mut()
        .unwrap()
        .push(json!({"job": CRATE, "test": "paddock-service other::t", "whole_job": false, "status": "failed"}));
    doc["counts"]["failed"] = json!(1);
    t.triage.insert(8300, Ok(doc));
    let (r, _) = go(&ladder_plan(), &mut t);
    assert!(r.unwrap_err().cause.contains("ran 1 test(s) but 2 were named"));
    // No counts at all, or counts that could not be read, are not a zero and not a pass.
    for ran in [
        Ok(json!({"ok": true, "rows": [], "counts": {}, "skipped_jobs": 0})),
        Err("the log is gone".to_string()),
    ] {
        let mut t = ladder_table();
        t.ran.insert(9001, ran);
        let stop = go(&ladder_plan(), &mut t).0.unwrap_err();
        assert!(
            stop.cause.contains("NOT MEASURED") || stop.cause.contains("test counts failed"),
            "{}",
            stop.cause
        );
        assert_eq!(t.workflows(), ["ci.yml"]);
    }
}

#[test]
fn a_rung_that_fails_stops_the_ladder_and_a_green_rung_never_lets_a_train_land() {
    // Rung 1 red with a new failure: stop, with the next round named.
    let mut t = ladder_table().result("ci.yml", RunResult::Failure);
    t.ran.insert(
        9001,
        Ok(ran_doc(1, json!([{"job": CRATE, "test": NX, "whole_job": false, "status": "new"}]))),
    );
    let (r, _) = go(&ladder_plan(), &mut t);
    let stop = r.unwrap_err();
    assert!(
        stop.cause.contains("rung 1 run 9001 is red") && stop.next.contains("counted from the server"),
        "{stop:?}"
    );
    assert_eq!(t.workflows(), ["ci.yml"], "rung 2 and the full runs never started");
    // Every rung green, the full run red: not green, nothing lands.
    let mut t = ladder_table();
    t.verdict_now.insert(
        "ci.yml".into(),
        Verdict::Red {
            run: None,
            reason: "no".into(),
        },
    );
    t.after.insert(
        "ci.yml".into(),
        Verdict::Red {
            run: None,
            reason: "a full test failed".into(),
        },
    );
    let (r, _) = go(&ladder_plan(), &mut t);
    assert!(r.unwrap_err().cause.contains("not green by `ci verdict`"));
}

#[test]
fn rung_1_is_skipped_with_its_reason_when_nothing_can_be_named_and_never_runs_an_empty_filter() {
    // Only whole-job and unknown rows: nothing to name.
    let mut t = ladder_table();
    t.triage.insert(
        8300,
        Ok(json!({"ok": true, "counts": {"new": 0, "already_failing": 0, "failed": 1, "flaky": 0, "unknown": 1}, "skipped_jobs": 0, "rows": [
            {"job": CONFORMANCE, "test": null, "whole_job": true, "status": "failed"},
            {"job": CRATE, "test": NX, "whole_job": false, "status": "unknown"}]})),
    );
    let (r, log) = go(&ladder_plan(), &mut t);
    let done = r.unwrap();
    assert!(only_dispatch(&t).is_none(), "no only_tests dispatch");
    assert!(
        matches!(&done.rungs[0], (Rung::Tests, RungOutcome::Skipped(w)) if w.contains("no named failing test")),
        "{:?}",
        done.rungs
    );
    assert!(log.contains("rung 1 (failed tests only): skipped"));
    // A job whose runner takes no test list is left to rung 2.
    let mut t = ladder_table();
    t.kinds.clear();
    assert!(go(&ladder_plan(), &mut t).0.is_ok());
    assert!(only_dispatch(&t).is_none());
    // Past the bound of rounds (counted from the server: three earlier heads with runs make this the fourth round), and with the
    // rung off: rung 2 starts.
    for (heads, max) in [(3usize, 3u32), (0, 0)] {
        let mut t = ladder_table();
        t.earlier_heads = (0..heads).map(|i| format!("{i:040x}")).collect();
        let mut p = dispatch_plan();
        p.ladder = Ladder { max_rounds: max };
        let done = go(&p, &mut t).0.unwrap();
        assert!(only_dispatch(&t).is_none(), "{heads} earlier heads, bound {max}");
        assert!(matches!(&done.rungs[0], (Rung::Tests, RungOutcome::Skipped(_))), "{:?}", done.rungs);
    }
    // Two earlier heads make this the third of three rounds: it still runs. Runs on this head do not count as a round.
    let mut t = ladder_table();
    t.earlier_heads = vec![format!("{:040x}", 1), format!("{:040x}", 2), SHA.to_string(), SHA.to_string()];
    assert!(go(&ladder_plan(), &mut t).0.is_ok());
    assert!(only_dispatch(&t).is_some(), "round 3 of 3 runs");
}

#[test]
fn a_test_that_also_fails_on_main_is_run_in_rung_1_and_blocks_like_any_other() {
    let mut t = ladder_table().result("ci.yml", RunResult::Failure);
    t.triage.insert(
        8300,
        Ok(json!({"ok": true, "counts": {"new": 0, "already_failing": 1, "failed": 0, "flaky": 0, "unknown": 0}, "skipped_jobs": 0, "rows": [
            {"job": CRATE, "test": NX, "whole_job": false, "status": "already-failing"}]})),
    );
    t.ran.insert(
        9001,
        Ok(ran_doc(
            1,
            json!([{"job": CRATE, "test": NX, "whole_job": false, "status": "already-failing"}]),
        )),
    );
    let stop = go(&ladder_plan(), &mut t).0.unwrap_err();
    assert!(only_dispatch(&t).is_some(), "the test was named");
    assert!(
        stop.cause.contains("rung 1 run 9001 is red") && stop.cause.contains("already-failing"),
        "{stop:?}"
    );
}

#[test]
fn rung_2_reruns_only_the_jobs_rung_1_did_not_cover_and_is_skipped_when_it_covered_them_all() {
    // The default red doc: the crate job's test (rung 1) and a whole-job failure (rung 2).
    let mut t = ladder_table();
    let done = go(&ladder_plan(), &mut t).0.unwrap();
    let r2 = t
        .dispatched
        .iter()
        .find(|d| d.1.iter().any(|(k, _)| k == "only_jobs"))
        .expect("rung 2 dispatched with only_jobs");
    let jobs = r2.1.iter().find(|(k, _)| k == "only_jobs").unwrap();
    assert_eq!(
        jobs.1,
        json!([CONFORMANCE]).to_string(),
        "the crate job ran green in rung 1: it does not rerun"
    );
    assert!(r2.1.contains(&("since_run".into(), "8300".into())));
    assert!(
        done.rungs
            .iter()
            .any(|(r, o)| *r == Rung::Jobs && matches!(o, RungOutcome::Green(_)))
    );

    // Rung 1 covered the only failing job: rung 2 is skipped, not run for nothing.
    let mut t = ladder_table();
    t.triage.insert(
        8300,
        Ok(json!({"ok": true, "counts": {"new": 1, "failed": 0, "flaky": 0, "already_failing": 0, "unknown": 0}, "skipped_jobs": 0, "rows": [
            {"job": CRATE, "test": NX, "whole_job": false, "status": "new"}]})),
    );
    let (r, log) = go(&ladder_plan(), &mut t);
    let done = r.unwrap();
    assert!(
        t.dispatched.iter().all(|d| !d.1.iter().any(|(k, _)| k == "only_jobs")),
        "no only_jobs dispatch"
    );
    assert_eq!(
        t.dispatched.iter().filter(|d| d.1.iter().any(|(k, _)| k == "since_run")).count(),
        1,
        "only rung 1 carried since_run"
    );
    assert!(
        matches!(&done.rungs[1], (Rung::Jobs, RungOutcome::Skipped(w)) if w.contains("every failing test")),
        "{:?}",
        done.rungs
    );
    assert!(log.contains("rung 2 (their whole jobs): skipped"), "{log}");
}

#[test]
fn a_rung_2_test_job_that_ran_no_test_is_not_measured() {
    // Rung 1 off: rung 2 reruns every failed job; the crate job takes a test list and must show tests that ran.
    let mut p = dispatch_plan();
    p.ladder = Ladder::off();
    let mut t = ladder_table();
    t.ran.insert(9001, Ok(ran_doc(0, json!([]))));
    let stop = go(&p, &mut t).0.unwrap_err();
    assert!(
        stop.cause.contains("rung 2 run 9001 is NOT MEASURED") && stop.cause.contains("ran no test"),
        "{stop:?}"
    );
    assert_eq!(t.workflows(), ["ci.yml"], "nothing after the unmeasured rung ran");
    // With tests that ran, the same rung passes (the positive control of the check).
    let mut t = ladder_table();
    t.ran.insert(9001, Ok(ran_doc(5, json!([]))));
    assert!(go(&p, &mut t).0.is_ok());
}

#[test]
fn rung_3_dispatches_stage_1_when_the_workflow_has_it_and_never_counts_it_as_a_full_run() {
    let stage = vec![("stage".to_string(), "fast".to_string())];
    let mut t = ladder_table();
    t.stage1 = Ok(Some(stage.clone()));
    // Dispatch order: 9001 rung 1, 9002 rung 2, 9003 stage 1.
    t.ran.insert(9003, Ok(ran_doc(40, json!([]))));
    let (r, log) = go(&ladder_plan(), &mut t);
    let done = r.unwrap();
    let s1 = t
        .dispatched
        .iter()
        .find(|d| d.1.iter().any(|(k, _)| k == "stage"))
        .expect("stage 1 dispatched");
    assert!(
        !s1.1.iter().any(|(k, _)| k == "since_run"),
        "stage 1 is a run of its own, not a rerun"
    );
    assert!(done.rungs.iter().any(|(r, o)| *r == Rung::Stage1 && *o == RungOutcome::Green(s1.2)));
    assert!(log.contains("rung 3 (stage 1 on every OS): green"), "{log}");
    // Rung 4 went alongside rung 3 (dispatched right after it, 9004), was not cancelled and is the run the landing reads.
    assert_eq!(t.dispatched[3].0, "ci.yml");
    assert!(
        t.dispatched[3].1.iter().all(|(k, _)| k != "stage" && k != "since_run"),
        "a full run"
    );
    assert!(t.cancelled.is_empty(), "{:?}", t.cancelled);
    assert!(log.contains("rung 4 alongside rung 3: ci.yml full run 9004"), "{log}");
    assert!(done.steps.iter().any(|(w, s)| w == "ci.yml" && matches!(s, Source::Verdict(_))));
    assert_eq!(
        t.dispatched.iter().filter(|d| d.0 == "ci.yml").count(),
        4,
        "the first full run was not dispatched twice"
    );

    // A stage 1 that ran no test is not measured.
    let mut t = ladder_table();
    t.stage1 = Ok(Some(stage.clone()));
    t.ran.insert(9003, Ok(ran_doc(0, json!([]))));
    assert!(go(&ladder_plan(), &mut t).0.unwrap_err().cause.contains("NOT MEASURED"));

    // A reader that counts the stage 1 run as a full green one is a defect that blocks the landing path.
    let mut t = ladder_table();
    t.stage1 = Ok(Some(stage.clone()));
    t.ran.insert(9003, Ok(ran_doc(40, json!([]))));
    t.counted_full.push(9003);
    let stop = go(&ladder_plan(), &mut t).0.unwrap_err();
    assert!(stop.cause.contains("counts the stage 1 run 9003"), "{stop:?}");
    assert_eq!(t.cancelled, [9004], "the full run alongside is cancelled");

    // A red stage 1 stops the ladder before the full run.
    let mut t = ladder_table();
    t.stage1 = Ok(Some(stage));
    t.fail_runs.push(9003);
    let stop = go(&ladder_plan(), &mut t).0.unwrap_err();
    assert!(stop.cause.contains("rung 3 run 9003") && stop.cause.contains("is red"), "{stop:?}");
    assert_eq!(t.cancelled, [9004], "rung 4 alongside is cancelled when rung 3 goes red");
    assert!(!t.waited.iter().any(|(r, _)| *r == 9004), "the cancelled run is not waited for");
    // A cancel that fails is a warning, not another stop: the red rung 3 is still the cause.
    let mut t = ladder_table();
    t.stage1 = Ok(Some(vec![("stage".to_string(), "fast".to_string())]));
    t.fail_runs.push(9003);
    t.cancel_error = Some("HTTP 404".into());
    let mut log = Vec::new();
    let stop = release(&ladder_plan(), &mut t, &mut log).unwrap_err();
    assert!(stop.cause.contains("rung 3 run 9003"), "{stop:?}");
    assert!(
        String::from_utf8(log)
            .unwrap()
            .contains("run 9004 could not be cancelled (HTTP 404); cancel it by hand")
    );
    // An acceptable full run of the head that exists already makes rung 3 unnecessary: no stage dispatch, no second full run.
    let mut t = ladder_table();
    t.stage1 = Ok(Some(vec![("stage".to_string(), "fast".to_string())]));
    t.verdict_now.insert("ci.yml".into(), Verdict::Green { run: 8999 });
    let done = go(&ladder_plan(), &mut t).0.unwrap();
    assert!(
        t.dispatched.iter().all(|d| !d.1.iter().any(|(k, _)| k == "stage")),
        "no stage 1 run"
    );
    assert!(
        matches!(&done.rungs[2], (Rung::Stage1, RungOutcome::Skipped(w)) if w.contains("exists already")),
        "{:?}",
        done.rungs
    );
}

#[test]
fn a_workflow_that_cannot_be_read_stops_rung_3_and_a_passed_round_skips_rung_1() {
    let mut t = ladder_table();
    t.stage1 = Err("git show failed".into());
    assert!(
        go(&ladder_plan(), &mut t)
            .0
            .unwrap_err()
            .cause
            .contains("stage 1 could not be read")
    );
    let mut t = ladder_table();
    t.runs.insert(8307, facts("ci.yml", SHA, RunResult::Failure));
    t.triage.insert(8307, Ok(flaky_doc()));
    let mut p = plan(FixWorkflow::Ci, Round::Passed(8307));
    p.ladder = Ladder { max_rounds: 3 };
    let done = go(&p, &mut t).0.unwrap();
    assert!(only_dispatch(&t).is_none());
    assert!(matches!(&done.rungs[0], (Rung::Tests, RungOutcome::Skipped(_))));
}
