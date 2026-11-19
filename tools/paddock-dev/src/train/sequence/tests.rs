use super::*;
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
    landables: BTreeMap<String, Result<FullRun, String>>,
}

impl Table {
    fn new() -> Table {
        // The since-run of the default plan: a finished, red ci.yml run.
        let mut runs = BTreeMap::new();
        runs.insert(8300, facts("ci.yml", SHA, RunResult::Failure));
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
            triage: BTreeMap::new(),
            runs,
            waited: Vec::new(),
            triaged: Vec::new(),
            wait_took: Duration::from_secs(60),
            ancestor: true,
            reach: BTreeMap::new(),
            reach_asked: Vec::new(),
            landables: BTreeMap::new(),
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
        let result = self.results.get(workflow).cloned().unwrap_or(RunResult::Success);
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
        if self
            .dispatched
            .iter()
            .any(|d| d.0 == workflow && !d.1.iter().any(|(k, _)| k == "since_run"))
        {
            return self.after.get(workflow).cloned().unwrap_or(Verdict::Green { run: 7 });
        }
        self.verdict_now.get(workflow).cloned().unwrap_or(Verdict::Red {
            run: None,
            reason: "no full run".into(),
        })
    }
    fn landable(&mut self, workflow: &str, _: &str) -> Result<FullRun, String> {
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
    }
}

fn dispatch_plan() -> Plan {
    plan(FixWorkflow::Ci, Round::Dispatch(8300))
}

fn flaky_doc() -> Value {
    json!({"ok": true, "counts": {"flaky": 1}, "skipped_jobs": 0, "rows": [
        {"job": "j", "test": A11Y, "whole_job": false, "status": "flaky"}]})
}

fn new_doc() -> Value {
    json!({"ok": true, "counts": {"flaky": 1, "new": 1}, "skipped_jobs": 0, "rows": [
        {"job": "j", "test": A11Y, "whole_job": false, "status": "flaky"},
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

fn landable_full() -> FullRun {
    FullRun {
        run: 9002,
        summary: "full run 9002 with 1 accepted FLAKY, 0 already failing on main".into(),
        lines: vec!["accepted FLAKY (e2e): a test".into()],
    }
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
