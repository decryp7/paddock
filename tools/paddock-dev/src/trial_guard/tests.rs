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
    /// The commit statuses it holds `(id, context, state)`; a POST to `.../statuses/<sha>` adds one with the next id.
    pub statuses: Vec<(u64, String, String)>,
    /// Make every status POST fail (the commit is not on the server yet).
    pub status_post_fails: bool,
}

impl Http for FakeGitea {
    fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
        if url.contains("/statuses") {
            return Ok(Value::Array(
                self.statuses
                    .iter()
                    .rev()
                    .map(|(id, c, s)| json!({"id": id, "context": c, "status": s}))
                    .collect(),
            ));
        }
        let id: u64 = url.rsplit('/').next().and_then(|s| s.parse().ok()).ok_or("unexpected GET")?;
        self.runs
            .iter()
            .find(|(i, _)| *i == id)
            .map(
                |(i, st)| json!({"id": i, "status": st, "conclusion": "", "path": "ci.yml@refs/heads/wip/x", "event": "workflow_dispatch"}),
            )
            .ok_or_else(|| "curl: (22) The requested URL returned error: 404".to_string())
    }
    fn send_json(&mut self, method: &str, url: &str, _token: &str, body: Option<&Value>) -> Result<Value, String> {
        if url.contains("/statuses/") {
            if self.status_post_fails {
                return Err("curl: (22) The requested URL returned error: 404".into());
            }
            let b = body.cloned().unwrap_or(Value::Null);
            let id = self.statuses.len() as u64 + 1;
            self.statuses.push((
                id,
                b["context"].as_str().unwrap_or("").into(),
                b["state"].as_str().unwrap_or("").into(),
            ));
        }
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
        remote_ref: String::new(),
        server: String::new(),
        repo: String::new(),
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

fn on_server(rec: Slow) -> Slow {
    Slow {
        remote_ref: "refs/heads/wip/x".into(),
        server: "https://gitea.invalid".into(),
        repo: "o/r".into(),
        ..rec
    }
}

fn gitea_with_run() -> FakeGitea {
    FakeGitea {
        runs: vec![(9001, "in_progress")],
        ..Default::default()
    }
}

fn time(now: u64, alive: &'static dyn Fn(u32, u64) -> bool) -> Time<'static> {
    Time { now, alive }
}

/// A start time of 0 means the identity could not be read: it is never "alive", so a record cannot keep a run unguarded for good.
#[test]
fn a_holder_with_no_start_time_is_not_alive_and_this_process_is() {
    assert!(!holder_alive(std::process::id(), 0));
    let start = paddock_proc::ProcessId::of(std::process::id()).unwrap().started().as_micros();
    assert!(holder_alive(std::process::id(), start));
    assert!(
        !holder_alive(std::process::id(), start + 3_600_000_000),
        "a pid with another start time is another process"
    );
}

#[test]
fn each_state_is_posted_as_its_own_commit_status_under_one_context() {
    for (state, want) in [
        (State::Running, "pending"),
        (State::Passed, "success"),
        (State::Failed, "failure"),
        (State::NotMeasured, "error"),
    ] {
        let mut g = FakeGitea::default();
        assert!(post_slow_status(&mut g, "t", &on_server(slow(state))).unwrap());
        assert_eq!(g.statuses, vec![(1, CONTEXT.to_string(), want.to_string())]);
        assert!(
            g.writes[0].1.ends_with(&format!("/api/v1/repos/o/r/statuses/{SHA}")),
            "{:?}",
            g.writes
        );
    }
    let mut g = FakeGitea::default();
    assert!(
        !post_slow_status(&mut g, "t", &slow(State::Failed)).unwrap(),
        "no server in the record: nothing posted"
    );
    assert!(g.writes.is_empty());
}

/// P1-2: a green CI run that finished before the slow rows failed must still be refused, through the server's status.
#[test]
fn a_slow_rows_status_that_is_not_success_refuses_the_commit_and_the_newest_one_counts() {
    let refusal = |statuses: Vec<(u64, &str, &str)>| {
        let mut g = FakeGitea {
            statuses: statuses.into_iter().map(|(i, c, s)| (i, c.to_string(), s.to_string())).collect(),
            ..Default::default()
        };
        let mut client = Client::new(&mut g, "https://gitea.invalid", "o/r", "t");
        status_refusal(&mut client, SHA).unwrap()
    };
    for bad in ["failure", "error", "pending"] {
        let why = refusal(vec![(1, CONTEXT, bad)]).unwrap_or_else(|| panic!("{bad} must refuse"));
        assert!(why.contains(bad) && why.contains(CONTEXT) && why.contains("Next:"), "{why}");
    }
    assert!(refusal(vec![]).is_none(), "no status: a commit that never had a profile run");
    assert!(refusal(vec![(1, CONTEXT, "success")]).is_none());
    assert!(refusal(vec![(1, "ci/other", "failure")]).is_none(), "another context is not ours");
    assert!(
        refusal(vec![(1, CONTEXT, "failure"), (2, CONTEXT, "success")]).is_none(),
        "the newest status wins"
    );
    assert!(
        refusal(vec![(1, CONTEXT, "success"), (2, CONTEXT, "failure")]).is_some(),
        "a later failure refuses"
    );
}

#[test]
fn this_checkouts_own_record_refuses_a_commit_whose_slow_rows_did_not_pass() {
    let d = tempfile::tempdir().unwrap();
    assert!(local_refusal(d.path(), SHA).is_none(), "no record");
    write_slow(d.path(), &slow(State::Passed)).unwrap();
    assert!(local_refusal(d.path(), SHA).is_none());
    for bad in [State::Running, State::Failed, State::NotMeasured] {
        write_slow(d.path(), &slow(bad)).unwrap();
        assert!(local_refusal(d.path(), SHA).unwrap().contains("Next:"), "{bad:?}");
    }
    std::fs::write(dir(d.path()).join(format!("{SHA}.slow.json")), "not json").unwrap();
    assert!(
        local_refusal(d.path(), SHA).unwrap().contains("cannot be read"),
        "an unreadable record fails closed"
    );
}

/// The shared sweep: a dead, killed or overdue child gets its run cancelled and its status posted as error, then the record is
/// rewritten; a live one is left alone; a failure to post leaves it running so the next sweep retries.
#[test]
fn the_sweep_settles_a_dead_or_overdue_child_and_leaves_a_live_one() {
    let dead = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = gitea_with_run();
    let lines = sweep(dead.path(), &mut g, "t", &time(1_000, GONE));
    assert_eq!(cancels(&g).len(), 1, "{lines:?}");
    assert_eq!(g.statuses, vec![(1, CONTEXT.to_string(), "error".to_string())], "{lines:?}");
    assert_eq!(read_slow(dead.path(), SHA).unwrap().unwrap().state, State::NotMeasured);
    assert!(
        lines.iter().any(|l| l.starts_with("::error::") && l.contains("is gone")),
        "{lines:?}"
    );
    assert!(
        sweep(dead.path(), &mut g, "t", &time(1_001, GONE)).is_empty(),
        "a settled record is not settled twice"
    );

    let overdue = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = gitea_with_run();
    sweep(overdue.path(), &mut g, "t", &time(2_001, LIVE));
    assert_eq!(cancels(&g).len(), 1, "past its deadline even though the pid answers");

    let alive = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = gitea_with_run();
    assert!(sweep(alive.path(), &mut g, "t", &time(1_000, LIVE)).is_empty());
    assert!(g.writes.is_empty() && read_slow(alive.path(), SHA).unwrap().unwrap().state == State::Running);

    let down = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = FakeGitea {
        status_post_fails: true,
        ..gitea_with_run()
    };
    let lines = sweep(down.path(), &mut g, "t", &time(1_000, GONE));
    assert!(lines.iter().any(|l| l.contains("could not be posted")), "{lines:?}");
    assert_eq!(
        read_slow(down.path(), SHA).unwrap().unwrap().state,
        State::Running,
        "left running: the next sweep tries again"
    );
}

/// ER-51: a new push to the same ref ends the previous push's process through its pid and start time, and only that one.
#[test]
fn a_new_push_to_the_same_ref_ends_the_previous_holder_by_identity_and_no_other() {
    let d = tempfile::tempdir().unwrap();
    let other = "f".repeat(40);
    let mine = on_server(slow(State::Running));
    write_slow(d.path(), &mine).unwrap();
    write_slow(
        d.path(),
        &Slow {
            sha: other.clone(),
            remote_ref: "refs/heads/wip/y".into(),
            ..on_server(slow(State::Running))
        },
    )
    .unwrap();
    write_slow(
        d.path(),
        &Slow {
            sha: "e".repeat(40),
            state: State::Passed,
            ..on_server(slow(State::Running))
        },
    )
    .unwrap();
    let ended = std::cell::RefCell::new(Vec::new());
    let new_sha = "a".repeat(40);
    let got = supersede(d.path(), "refs/heads/wip/x", &new_sha, &|p, s| ended.borrow_mut().push((p, s)));
    assert_eq!(
        ended.into_inner(),
        vec![(4242, 7)],
        "only the same ref's running holder, by pid and start time"
    );
    assert_eq!(got.len(), 1);
    let r = read_slow(d.path(), SHA).unwrap().unwrap();
    assert_eq!(r.state, State::NotMeasured);
    assert!(r.why.contains("superseded"), "{r:?}");
    assert_eq!(
        read_slow(d.path(), &other).unwrap().unwrap().state,
        State::Running,
        "another ref is untouched"
    );
    let same = supersede(d.path(), "refs/heads/wip/x", SHA, &|_, _| {
        panic!("a re-push of the same commit ends nothing")
    });
    assert!(same.is_empty());
}

/// ER-51 (amended): the folder has an owner note, a size limit and oldest-first eviction, and never evicts a live holder.
#[test]
fn the_records_folder_has_an_owner_a_limit_and_evicts_the_oldest_but_never_a_live_holder() {
    let d = tempfile::tempdir().unwrap();
    let sha = |n: u32| format!("{n:040x}");
    let me = std::process::id();
    let start = paddock_proc::ProcessId::of(me).unwrap().started().as_micros();
    for n in 1..=5u32 {
        let live = n == 1;
        write_slow(
            d.path(),
            &Slow {
                sha: sha(n),
                state: if live { State::Running } else { State::Passed },
                pid: if live { me } else { 0 },
                start_micros: if live { start } else { 0 },
                ..slow(State::Passed)
            },
        )
        .unwrap();
        write_dispatched(
            d.path(),
            &Dispatched {
                sha: sha(n),
                ..dispatched()
            },
        )
        .unwrap();
        // Distinct modification times, oldest first: the order of eviction.
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(1000 - u64::from(n) * 10);
        for what in ["slow", "dispatch"] {
            std::fs::File::options()
                .write(true)
                .open(dir(d.path()).join(format!("{}.{what}.json", sha(n))))
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
    }
    prune(d.path(), 3);
    assert!(std::fs::read_to_string(dir(d.path()).join("owner")).unwrap().contains("limit"));
    let left = |n: u32| read_slow(d.path(), &sha(n)).unwrap().is_some();
    assert!(left(1), "the oldest is a live holder: kept");
    assert!(!left(2), "the next oldest goes first");
    assert!(left(3) && left(4) && left(5));
    assert!(
        read_dispatched(d.path(), &sha(2)).unwrap().is_none(),
        "its dispatch record goes with it"
    );
}
