use super::*;
use serde_json::json;
use std::cell::Cell;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const A11Y: &str = "[webkit] › e2e/a11y.e2e.ts:6:7 › main screen has no serious accessibility violations (dark)";
const CHAT: &str = "[webkit] › e2e/ai-chat.e2e.ts:130:7 › AI chat › a long conversation scrolls";

/// One dispatch: workflow, inputs, run id.
type Dispatch = (String, Vec<(String, String)>, u64);

/// A table instead of Gitea: runs are numbered from 9000 as they are dispatched; `ends` says how a workflow's run ends.
struct Table {
    now: Cell<Duration>,
    next: u64,
    dispatched: Vec<Dispatch>,
    ends: BTreeMap<String, RunEnd>,
    disk: Result<(), String>,
    dispatch_error: Option<String>,
    triage: BTreeMap<u64, Result<Value, String>>,
    runs: BTreeMap<u64, RunInfo>,
    waited: Vec<(u64, Duration)>,
    triaged: Vec<u64>,
    wait_took: Duration,
}

impl Table {
    fn new() -> Table {
        Table {
            now: Cell::new(Duration::ZERO),
            next: 9000,
            dispatched: Vec::new(),
            ends: BTreeMap::new(),
            disk: Ok(()),
            dispatch_error: None,
            triage: BTreeMap::new(),
            runs: BTreeMap::new(),
            waited: Vec::new(),
            triaged: Vec::new(),
            wait_took: Duration::from_secs(60),
        }
    }
    fn end(mut self, wf: &str, end: RunEnd) -> Table {
        self.ends.insert(wf.into(), end);
        self
    }
    fn workflows(&self) -> Vec<&str> {
        self.dispatched.iter().map(|d| d.0.as_str()).collect()
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
        Ok(self.next)
    }
    fn wait_run(&mut self, run: u64, max: Duration) -> RunEnd {
        self.waited.push((run, max));
        self.now.set(self.now.get() + self.wait_took);
        let wf = &self.dispatched.iter().find(|d| d.2 == run).expect("a dispatched run").0;
        self.ends.get(wf).cloned().unwrap_or(RunEnd::Green)
    }
    fn run_info(&mut self, run: u64) -> Result<Option<RunInfo>, String> {
        Ok(self.runs.get(&run).cloned())
    }
    fn triage(&mut self, run: u64) -> Result<Value, String> {
        self.triaged.push(run);
        self.triage
            .get(&run)
            .cloned()
            .unwrap_or_else(|| Err("no triage in the table".into()))
    }
}

fn info(run: u64, wf: &str, sha: &str, conclusion: &str) -> RunInfo {
    RunInfo {
        id: run,
        path: format!("{wf}@refs/heads/train/x"),
        event: "workflow_dispatch".into(),
        status: "completed".into(),
        conclusion: conclusion.into(),
        head_sha: sha.into(),
        head_branch: "train/x".into(),
        created_at: String::new(),
        run_attempt: 1,
    }
}

fn plan(workflow: FixWorkflow, round: Round) -> Plan {
    Plan {
        branch: "train/x".into(),
        sha: SHA.into(),
        workflow,
        round,
        accepted: vec![Accepted::parse("e2e/a11y.e2e.ts:6:7").unwrap()],
        reuse: BTreeMap::new(),
        max: Duration::from_secs(14 * 3600),
    }
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

const ALL: [&str; 8] = [
    "ci.yml",
    "debrief.yml",
    "dyno-release.yml",
    "pitcrew-release.yml",
    "pitwall-release.yml",
    "spotter-release.yml",
    "shell-demo-release.yml",
    "workbench-demo-release.yml",
];

#[test]
fn a_green_ci_round_runs_debrief_and_every_release_workflow_in_order() {
    let mut t = Table::new();
    let (r, log) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    let done = r.unwrap();
    assert_eq!(t.workflows(), ALL);
    assert_eq!(done.steps.len(), 8);
    assert!(done.steps.iter().all(|(_, s)| matches!(s, Source::Ran(_))));
    let inputs = |wf: &str| t.dispatched.iter().find(|d| d.0 == wf).unwrap().1.clone();
    assert_eq!(
        inputs("ci.yml"),
        vec![("webkit".to_string(), "true".to_string()), ("since_run".into(), "8300".into())]
    );
    assert_eq!(inputs("debrief.yml"), vec![("bundles".to_string(), "true".to_string())]);
    assert_eq!(inputs("dyno-release.yml"), vec![("gates".to_string(), "false".to_string())]);
    assert!(inputs("shell-demo-release.yml").is_empty());
    assert!(log.contains("run 9001 Green after 1m"), "{log}");
    assert!(log.contains("release sequence green on 012345678"), "{log}");
}

#[test]
fn a_debrief_round_has_no_debrief_run_after_it_and_takes_the_bundles_input() {
    let mut t = Table::new();
    let (r, _) = go(&plan(FixWorkflow::Debrief, Round::Dispatch(8301)), &mut t);
    r.unwrap();
    assert_eq!(t.workflows().iter().filter(|w| **w == "debrief.yml").count(), 1);
    assert!(!t.workflows().contains(&"ci.yml"));
    assert_eq!(
        t.dispatched[0].1,
        vec![("bundles".to_string(), "true".to_string()), ("since_run".into(), "8301".into())]
    );
}

#[test]
fn it_stops_at_the_first_red_and_dispatches_nothing_after_it() {
    let mut t = Table::new().end("pitcrew-release.yml", RunEnd::Red);
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    let stop = r.unwrap_err();
    assert_eq!(t.workflows().last(), Some(&"pitcrew-release.yml"));
    assert_eq!(t.workflows().len(), 4);
    assert!(stop.cause.contains("pitcrew-release.yml run 9004 is red"), "{stop:?}");
    assert!(stop.annotation().starts_with("::error::train release: ") && stop.annotation().contains("Next step: "));
    assert_eq!(stop.annotation().lines().count(), 1);
}

#[test]
fn a_debrief_run_that_is_red_after_the_ci_round_stops_before_the_releases() {
    let mut t = Table::new().end("debrief.yml", RunEnd::Red);
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    assert!(r.is_err());
    assert_eq!(t.workflows(), ["ci.yml", "debrief.yml"]);
}

/// The positive control of the round: a red fix round whose rows are accepted flaky tests goes on.
#[test]
fn a_red_fix_round_with_only_accepted_flaky_tests_goes_on() {
    let mut t = Table::new().end("ci.yml", RunEnd::Red);
    t.triage.insert(9001, Ok(flaky_doc()));
    let (r, log) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    let done = r.unwrap();
    assert_eq!(done.steps[0], ("ci.yml".to_string(), Source::Judged(9001)));
    assert_eq!(t.workflows().len(), 8);
    assert!(log.contains("PASS: 1 failing row(s)"), "{log}");
}

/// The negative control: the same round with a NEW failure beside it stops and dispatches nothing more.
#[test]
fn a_red_fix_round_with_a_new_failure_stops_and_dispatches_nothing_more() {
    let mut t = Table::new().end("ci.yml", RunEnd::Red);
    t.triage.insert(9001, Ok(new_doc()));
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    let stop = r.unwrap_err();
    assert_eq!(t.workflows(), ["ci.yml"]);
    assert!(stop.cause.contains("new: ") && stop.cause.contains("ai-chat"), "{stop:?}");
}

#[test]
fn a_red_fix_round_whose_triage_fails_stops() {
    let mut t = Table::new().end("ci.yml", RunEnd::Red);
    t.triage.insert(9001, Err("the network is down".into()));
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    assert!(r.unwrap_err().cause.contains("its triage failed: the network is down"));
    assert_eq!(t.workflows(), ["ci.yml"]);
}

#[test]
fn a_run_that_did_not_finish_is_never_triaged_or_taken_for_green() {
    let mut t = Table::new().end("ci.yml", RunEnd::Unfinished("still running after 6h".into()));
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    assert!(r.unwrap_err().cause.contains("did not finish: still running after 6h"));
    assert!(t.triaged.is_empty());
    assert_eq!(t.workflows().len(), 1);
}

#[test]
fn a_passed_round_is_not_dispatched_again_but_it_is_judged() {
    let mut t = Table::new();
    t.runs.insert(8307, info(8307, "debrief.yml", SHA, "failure"));
    t.triage.insert(8307, Ok(flaky_doc()));
    let (r, log) = go(&plan(FixWorkflow::Debrief, Round::Passed(8307)), &mut t);
    let done = r.unwrap();
    assert_eq!(done.steps[0], ("debrief.yml".to_string(), Source::Judged(8307)));
    assert!(
        !t.workflows().contains(&"debrief.yml") && t.workflows().len() == 6,
        "{:?}",
        t.workflows()
    );
    assert!(log.contains("not dispatched again"), "{log}");
}

#[test]
fn a_passed_round_that_was_green_needs_no_triage() {
    let mut t = Table::new();
    t.runs.insert(8307, info(8307, "ci.yml", SHA, "success"));
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Passed(8307)), &mut t);
    r.unwrap();
    assert!(t.triaged.is_empty());
    assert_eq!(t.workflows()[0], "debrief.yml");
}

#[test]
fn a_passed_claim_is_checked_against_the_run_the_train_head_and_the_verdict() {
    let p = || plan(FixWorkflow::Debrief, Round::Passed(8307));
    // no such run
    let (r, _) = go(&p(), &mut Table::new());
    assert!(r.unwrap_err().cause.contains("no such run"));
    // another workflow
    let mut t = Table::new();
    t.runs.insert(8307, info(8307, "ci.yml", SHA, "success"));
    assert!(go(&p(), &mut t).0.unwrap_err().cause.contains("not of debrief.yml"));
    // another commit
    let mut t = Table::new();
    t.runs.insert(8307, info(8307, "debrief.yml", "ffffffffffffffff", "success"));
    assert!(go(&p(), &mut t).0.unwrap_err().cause.contains("it tested fffffffff"));
    // not finished
    let mut t = Table::new();
    let mut running = info(8307, "debrief.yml", SHA, "");
    running.status = "in_progress".into();
    t.runs.insert(8307, running);
    assert!(go(&p(), &mut t).0.unwrap_err().cause.contains("still in_progress"));
    // red with a new failure
    let mut t = Table::new();
    t.runs.insert(8307, info(8307, "debrief.yml", SHA, "failure"));
    t.triage.insert(8307, Ok(new_doc()));
    assert!(go(&p(), &mut t).0.is_err());
    assert!(t.dispatched.is_empty());
}

#[test]
fn a_reused_green_run_is_not_dispatched_again_and_is_checked() {
    let mut p = plan(FixWorkflow::Ci, Round::Dispatch(8300));
    p.reuse.insert("dyno-release.yml".into(), 8302);
    let mut t = Table::new();
    t.runs.insert(8302, info(8302, "dyno-release.yml", "oldsha", "success"));
    let (r, log) = go(&p, &mut t);
    let done = r.unwrap();
    assert!(!t.workflows().contains(&"dyno-release.yml"));
    assert!(done.steps.contains(&("dyno-release.yml".to_string(), Source::Reused(8302))));
    assert!(log.contains("reusing green run 8302"), "{log}");
    // a reuse of a red run, of another workflow, of a missing run and of an unknown workflow all stop
    for (run, wf, why) in [
        (
            info(8302, "dyno-release.yml", "s", "failure"),
            "dyno-release.yml",
            "completed (failure)",
        ),
        (
            info(8302, "pitwall-release.yml", "s", "success"),
            "dyno-release.yml",
            "it is a run of pitwall-release.yml",
        ),
    ] {
        let mut p = plan(FixWorkflow::Ci, Round::Dispatch(8300));
        p.reuse.insert(wf.into(), 8302);
        let mut t = Table::new();
        t.runs.insert(8302, run);
        assert!(go(&p, &mut t).0.unwrap_err().cause.contains(why), "{why}");
    }
    let mut p = plan(FixWorkflow::Ci, Round::Dispatch(8300));
    p.reuse.insert("dyno-release.yml".into(), 1);
    assert!(go(&p, &mut Table::new()).0.unwrap_err().cause.contains("no such run"));
    let mut p = plan(FixWorkflow::Ci, Round::Dispatch(8300));
    p.reuse.insert("dyno.yml".into(), 1);
    let mut t = Table::new();
    assert!(go(&p, &mut t).0.unwrap_err().cause.contains("not a release workflow"));
    assert!(t.dispatched.is_empty(), "a bad argument dispatches nothing");
}

#[test]
fn a_refused_disk_check_dispatches_nothing() {
    let mut t = Table::new();
    t.disk = Err("only 10 GiB free".into());
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
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
    let (r, _) = go(&plan(FixWorkflow::Ci, Round::Dispatch(8300)), &mut t);
    assert!(r.unwrap_err().cause.contains("ci.yml was not dispatched: 401"));
}

/// Every wait is bounded: a run is waited for at most what is left of --max, and nothing is dispatched once it is used up.
#[test]
fn the_waits_are_bounded_by_max_and_the_run_cap() {
    let mut p = plan(FixWorkflow::Ci, Round::Dispatch(8300));
    p.max = Duration::from_secs(8 * 3600);
    let mut t = Table::new();
    t.wait_took = Duration::from_secs(3 * 3600);
    let (r, _) = go(&p, &mut t);
    // ci.yml 3h, debrief 3h, dyno 2h left (the third wait is cut to what is left), then nothing is left
    assert_eq!(t.waited[0].1, RUN_CAP.min(Duration::from_secs(8 * 3600)));
    assert_eq!(t.waited[1].1, Duration::from_secs(5 * 3600));
    assert_eq!(t.waited[2].1, Duration::from_secs(2 * 3600));
    assert!(t.waited.iter().all(|(_, m)| *m <= RUN_CAP));
    assert!(r.unwrap_err().cause.contains("used up before pitcrew-release.yml"));
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
}
