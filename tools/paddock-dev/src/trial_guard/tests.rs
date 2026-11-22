// ci-runs-reader: trial push profile: registers and cancels one dispatched run; no pass/fail verdict on a commit
use super::*;
use crate::wait::Http;
use serde_json::{Value, json};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A fake dispatch: a Gitea that knows the runs it is given and records every write (ER-14: no network).
#[derive(Default)]
pub struct FakeGitea {
    /// `(id, status)` of the runs it knows.
    pub runs: Vec<(u64, &'static str)>,
    /// The write requests made: `(method, url)`.
    pub writes: Vec<(String, String)>,
}

impl Http for FakeGitea {
    fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
        let id: u64 = url.rsplit('/').next().and_then(|s| s.parse().ok()).ok_or("unexpected GET")?;
        self.runs
            .iter()
            .find(|(i, _)| *i == id)
            .map(
                |(i, st)| json!({"id": i, "status": st, "conclusion": "", "path": "ci.yml@refs/heads/wip/x", "event": "workflow_dispatch"}),
            )
            .ok_or_else(|| "curl: (22) The requested URL returned error: 404".to_string())
    }
    fn send_json(&mut self, method: &str, url: &str, _token: &str, _body: Option<&Value>) -> Result<Value, String> {
        self.writes.push((method.to_string(), url.to_string()));
        Ok(Value::Null)
    }
}

fn slow(state: State) -> Slow {
    Slow {
        sha: SHA.into(),
        state,
        row: String::new(),
        why: String::new(),
        next: String::new(),
        pid: 4242,
        start_micros: 7,
        finish_by: 2_000,
        logs: "/logs/x".into(),
    }
}

fn failed() -> Slow {
    Slow {
        row: "clippy-linux crates".into(),
        why: "error: boom".into(),
        next: "fix the lint it names".into(),
        ..slow(State::Failed)
    }
}

fn dispatched() -> Dispatched {
    Dispatched {
        sha: SHA.into(),
        run_id: 9001,
        workflow: "ci.yml".into(),
        git_ref: "wip/x".into(),
        server: "https://gitea.invalid".into(),
        repo: "o/r".into(),
    }
}

fn cancels(g: &FakeGitea) -> Vec<&(String, String)> {
    g.writes
        .iter()
        .filter(|(m, u)| m == "POST" && u.ends_with("/actions/runs/9001/cancel"))
        .collect()
}

const LIVE: &dyn Fn(u32, u64) -> bool = &|_, _| true;
const GONE: &dyn Fn(u32, u64) -> bool = &|_, _| false;

fn dir_with(slow_rec: Option<&Slow>, run: Option<&Dispatched>) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    if let Some(s) = slow_rec {
        write_slow(d.path(), s).unwrap();
    }
    if let Some(r) = run {
        write_dispatched(d.path(), r).unwrap();
    }
    d
}

/// ER-33 positive control: a failing slow row cancels the registered run. Without the cancel the POST is missing and this is red.
#[test]
fn a_failed_slow_row_cancels_the_run_and_says_which_row_and_what_to_do() {
    let d = dir_with(Some(&failed()), Some(&dispatched()));
    let mut g = FakeGitea {
        runs: vec![(9001, "in_progress")],
        ..Default::default()
    };
    let got = settle(d.path(), SHA, &mut g, "t", 1_000, LIVE).unwrap();
    assert_eq!(cancels(&g).len(), 1, "{:?}", g.writes);
    match got {
        Settled::Cancelled {
            run: 9001,
            why,
            already_over: false,
        } => {
            assert!(
                why.contains("clippy-linux crates") && why.contains("error: boom") && why.contains("Next: fix the lint"),
                "{why}"
            )
        }
        other => panic!("{other:?}"),
    }
}

/// A not-measured slow row cancels too: nothing is proven, and a trial is never green by skipping.
#[test]
fn a_not_measured_slow_row_also_cancels_the_run() {
    let rec = Slow {
        row: "clippy-windows crates".into(),
        why: "no xwin".into(),
        ..slow(State::NotMeasured)
    };
    let d = dir_with(Some(&rec), Some(&dispatched()));
    let mut g = FakeGitea {
        runs: vec![(9001, "queued")],
        ..Default::default()
    };
    let got = settle(d.path(), SHA, &mut g, "t", 1_000, LIVE).unwrap();
    assert_eq!(cancels(&g).len(), 1, "{:?}", g.writes);
    assert!(matches!(got, Settled::Cancelled { why, .. } if why.contains("not measured") && why.contains("not evidence")));
}

/// The control's other half: the same setup with every row passed (or still running) makes no write, so the positive tests above
/// can only be red for the failure.
#[test]
fn passed_slow_rows_and_running_ones_leave_the_run_alone() {
    for (rec, want) in [(slow(State::Passed), Settled::Fine), (slow(State::Running), Settled::Waiting)] {
        let d = dir_with(Some(&rec), Some(&dispatched()));
        let mut g = FakeGitea {
            runs: vec![(9001, "in_progress")],
            ..Default::default()
        };
        assert_eq!(settle(d.path(), SHA, &mut g, "t", 1_000, LIVE).unwrap(), want);
        assert!(g.writes.is_empty(), "{:?}", g.writes);
    }
}

/// Never green by skipping: a record still running past its deadline, or whose process is gone, is not evidence.
#[test]
fn slow_rows_that_never_finished_cancel_the_run() {
    let past_deadline = dir_with(Some(&slow(State::Running)), Some(&dispatched()));
    let mut g = FakeGitea {
        runs: vec![(9001, "in_progress")],
        ..Default::default()
    };
    let got = settle(past_deadline.path(), SHA, &mut g, "t", 2_001, LIVE).unwrap();
    assert_eq!(cancels(&g).len(), 1);
    assert!(matches!(got, Settled::Cancelled { why, .. } if why.contains("never finished")));

    let dead = dir_with(Some(&slow(State::Running)), Some(&dispatched()));
    let mut g = FakeGitea {
        runs: vec![(9001, "in_progress")],
        ..Default::default()
    };
    let got = settle(dead.path(), SHA, &mut g, "t", 1_000, GONE).unwrap();
    assert_eq!(cancels(&g).len(), 1);
    assert!(matches!(got, Settled::Cancelled { why, .. } if why.contains("pid 4242") && why.contains("is gone")));
}

/// The two writers meet in either order: the rows fail before the run is registered, and then the dispatch side's settle cancels.
#[test]
fn a_run_registered_after_the_failure_is_cancelled_by_the_dispatch_side() {
    let d = dir_with(Some(&failed()), None);
    let mut g = FakeGitea {
        runs: vec![(9001, "queued")],
        ..Default::default()
    };
    let first = settle(d.path(), SHA, &mut g, "t", 1_000, LIVE).unwrap();
    assert!(matches!(first, Settled::NoRun(Verdict::Cancel(_))), "{first:?}");
    assert!(g.writes.is_empty(), "no run is known yet");
    write_dispatched(d.path(), &dispatched()).unwrap();
    let second = settle(d.path(), SHA, &mut g, "t", 1_001, LIVE).unwrap();
    assert!(matches!(second, Settled::Cancelled { run: 9001, .. }), "{second:?}");
    assert_eq!(cancels(&g).len(), 1);
}

#[test]
fn a_run_that_is_over_or_gone_is_not_cancelled_again() {
    for runs in [vec![(9001, "completed")], vec![]] {
        let d = dir_with(Some(&failed()), Some(&dispatched()));
        let mut g = FakeGitea {
            runs,
            ..Default::default()
        };
        let got = settle(d.path(), SHA, &mut g, "t", 1_000, LIVE).unwrap();
        assert!(matches!(got, Settled::Cancelled { already_over: true, .. }), "{got:?}");
        assert!(g.writes.is_empty(), "{:?}", g.writes);
    }
}

#[test]
fn no_record_is_unguarded_and_a_note_never_puts_a_finished_record_back_to_running() {
    let d = dir_with(None, None);
    assert_eq!(
        settle(d.path(), SHA, &mut FakeGitea::default(), "t", 1, LIVE).unwrap(),
        Settled::Unguarded
    );
    write_slow(d.path(), &failed()).unwrap();
    note_holder(d.path(), SHA, 77, 88).unwrap();
    assert_eq!(read_slow(d.path(), SHA).unwrap().unwrap().state, State::Failed);
    write_slow(d.path(), &slow(State::Running)).unwrap();
    note_holder(d.path(), SHA, 77, 88).unwrap();
    let r = read_slow(d.path(), SHA).unwrap().unwrap();
    assert_eq!((r.pid, r.start_micros), (77, 88));
}

#[test]
fn a_commit_id_that_is_not_hex_never_becomes_a_file_name() {
    let d = tempfile::tempdir().unwrap();
    for bad in ["../x", "", "abc", "zzzzzzzz"] {
        assert!(read_slow(d.path(), bad).is_err(), "{bad}");
    }
}

fn run(id: u64, wf: &str, r: &str, sha: &str, event: &str) -> RunInfo {
    RunInfo {
        id,
        path: format!("{wf}@refs/heads/{r}"),
        event: event.into(),
        status: "queued".into(),
        conclusion: String::new(),
        head_sha: sha.into(),
        head_branch: r.into(),
        created_at: String::new(),
        run_attempt: 1,
    }
}

#[test]
fn the_dispatched_run_is_the_one_new_run_of_that_workflow_ref_and_commit() {
    let listed = vec![
        run(5, "ci.yml", "wip/x", SHA, "workflow_dispatch"),
        run(6, "ci.yml", "wip/x", SHA, "workflow_dispatch"),
        run(7, "ci.yml", "wip/y", SHA, "workflow_dispatch"),
        run(8, "other.yml", "wip/x", SHA, "workflow_dispatch"),
        run(9, "ci.yml", "wip/x", "ffff", "workflow_dispatch"),
        run(10, "ci.yml", "wip/x", SHA, "push"),
    ];
    assert_eq!(find_new_run(&listed, &[5], "ci.yml", "wip/x", SHA), Ok(Some(6)));
    assert_eq!(find_new_run(&listed, &[5, 6], "ci.yml", "wip/x", SHA), Ok(None));
    assert!(
        find_new_run(&listed, &[], "ci.yml", "wip/x", SHA).is_err(),
        "two candidates are ambiguous, never guessed"
    );
}
