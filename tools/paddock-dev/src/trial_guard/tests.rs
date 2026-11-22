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
    /// `permissions.push` of the repository it answers for (None: the repository answers 404).
    pub repo_push: Option<bool>,
    /// The most statuses the server returns per page whatever `limit` asks (Gitea's MAX_RESPONSE_ITEMS).
    pub status_page_cap: Option<usize>,
}

impl Http for FakeGitea {
    fn get_json(&mut self, url: &str, _token: &str) -> Result<Value, String> {
        if url.contains("/commits/") && url.contains("/status") {
            // Gitea's combined status: the newest status of each context, newest first, paged.
            let q = |k: &str| {
                url.split(&format!("{k}="))
                    .nth(1)
                    .and_then(|v| v.split('&').next())
                    .and_then(|v| v.parse::<usize>().ok())
            };
            let (page, mut limit) = (q("page").unwrap_or(1), q("limit").unwrap_or(50));
            if let Some(cap) = self.status_page_cap {
                limit = limit.min(cap);
            }
            let mut latest: Vec<&(u64, String, String)> = Vec::new();
            for st in self.statuses.iter() {
                match latest.iter().position(|l| l.1 == st.1) {
                    Some(i) if latest[i].0 < st.0 => latest[i] = st,
                    Some(_) => {}
                    None => latest.push(st),
                }
            }
            latest.sort_by_key(|l| std::cmp::Reverse(l.0));
            let list: Vec<Value> = latest
                .iter()
                .skip((page - 1) * limit)
                .take(limit)
                .map(|(id, c, s)| json!({"id": id, "context": c, "status": s}))
                .collect();
            return Ok(json!({"state": "", "statuses": list}));
        }
        if url.ends_with("/api/v1/repos/o/r") {
            return self
                .repo_push
                .map(|p| json!({"permissions": {"push": p}}))
                .ok_or_else(|| "curl: (22) The requested URL returned error: 404".to_string());
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
        beat: 1_000,
        posted: false,
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

fn gate(statuses: Vec<(u64, String, String)>, local: Option<&Path>) -> Result<Option<String>, String> {
    let mut g = FakeGitea {
        statuses,
        ..Default::default()
    };
    let mut client = Client::new(&mut g, "https://gitea.invalid", "o/r", "t");
    client.backoff_ms = 0;
    slow_rows_gate(&mut client, local, SHA)
}

fn stat(id: u64, context: &str, state: &str) -> (u64, String, String) {
    (id, context.to_string(), state.to_string())
}

fn filler(first_id: u64, n: u64) -> Vec<(u64, String, String)> {
    (0..n).map(|i| stat(first_id + i, &format!("ci/job-{i}"), "success")).collect()
}

/// The gate fails closed: success is the only positive evidence once a status exists, whatever else the commit has.
#[test]
fn the_slow_rows_gate_lets_evidence_count_only_for_no_split_push_or_an_explicit_success() {
    for bad in ["failure", "error", "pending"] {
        let why = gate(vec![stat(1, CONTEXT, bad)], None)
            .unwrap()
            .unwrap_or_else(|| panic!("{bad} must refuse"));
        assert!(why.contains(bad) && why.contains(CONTEXT) && why.contains("Next:"), "{why}");
    }
    assert_eq!(gate(vec![], None), Ok(None), "no status and no record: never a split push");
    assert_eq!(gate(vec![stat(1, CONTEXT, "success")], None), Ok(None));
    assert_eq!(
        gate(vec![stat(1, "ci/other", "failure")], None),
        Ok(None),
        "another context is not ours"
    );
    assert_eq!(
        gate(vec![stat(1, CONTEXT, "failure"), stat(2, CONTEXT, "success")], None),
        Ok(None),
        "the newest status wins"
    );
    assert!(
        gate(vec![stat(1, CONTEXT, "success"), stat(2, CONTEXT, "failure")], None)
            .unwrap()
            .is_some()
    );
}

/// Round 2 P1-1: the status paged out behind more than 50 newer CI statuses is still found, and a bound hit is not measured.
#[test]
fn a_slow_rows_status_behind_more_than_fifty_newer_statuses_is_found_and_the_page_bound_is_not_measured() {
    let mut statuses = vec![stat(1, CONTEXT, "failure")];
    statuses.extend(filler(10, 120));
    let why = gate(statuses, None)
        .unwrap()
        .expect("a failure behind 120 newer statuses still refuses");
    assert!(why.contains("failure"), "{why}");
    let mut ok = vec![stat(1, CONTEXT, "success")];
    ok.extend(filler(10, 120));
    assert_eq!(gate(ok, None), Ok(None));
    let mut too_many = vec![stat(1, CONTEXT, "failure")];
    too_many.extend(filler(10, u64::from(STATUS_PAGES) * 50 + 10));
    let err = gate(too_many, None).unwrap_err();
    assert!(err.contains("first"), "{err}");
}

/// Round 2 P1-3: no status while this checkout holds a record for the commit means the post did not land: refused. An unreadable
/// record refuses too.
#[test]
fn a_post_that_never_landed_is_refused_through_the_local_record_and_a_cleared_one_lets_evidence_count() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(gate(vec![], Some(d.path())), Ok(None), "no record here: unchanged");
    write_slow(d.path(), &on_server(slow(State::Passed))).unwrap();
    let why = gate(vec![], Some(d.path())).unwrap().expect("no status, but a record");
    assert!(why.contains("did not land") && why.contains("Next:"), "{why}");
    assert_eq!(
        gate(vec![stat(1, CONTEXT, "success")], Some(d.path())),
        Ok(None),
        "explicit success counts"
    );
    std::fs::write(dir(d.path()).join(format!("{SHA}.slow.json")), "not json").unwrap();
    assert!(gate(vec![], Some(d.path())).unwrap().is_some(), "an unreadable record fails closed");
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
    let lines = sweep(dead.path(), &mut g, "t", &time(1_000, GONE), &|_, _| {});
    assert_eq!(cancels(&g).len(), 1, "{lines:?}");
    assert_eq!(g.statuses, vec![(1, CONTEXT.to_string(), "error".to_string())], "{lines:?}");
    assert_eq!(read_slow(dead.path(), SHA).unwrap().unwrap().state, State::NotMeasured);
    assert!(
        lines.iter().any(|l| l.starts_with("::error::") && l.contains("is gone")),
        "{lines:?}"
    );
    assert!(
        sweep(dead.path(), &mut g, "t", &time(1_001, GONE), &|_, _| {}).is_empty(),
        "a settled record is not settled twice"
    );

    let overdue = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = gitea_with_run();
    sweep(overdue.path(), &mut g, "t", &time(2_001, LIVE), &|_, _| {});
    assert_eq!(cancels(&g).len(), 1, "past its deadline even though the pid answers");

    let alive = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = gitea_with_run();
    assert!(sweep(alive.path(), &mut g, "t", &time(1_000, LIVE), &|_, _| {}).is_empty());
    assert!(g.writes.is_empty() && read_slow(alive.path(), SHA).unwrap().unwrap().state == State::Running);

    let down = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let mut g = FakeGitea {
        status_post_fails: true,
        ..gitea_with_run()
    };
    let lines = sweep(down.path(), &mut g, "t", &time(1_000, GONE), &|_, _| {});
    assert!(lines.iter().any(|l| l.contains("could not be posted")), "{lines:?}");
    let r = read_slow(down.path(), SHA).unwrap().unwrap();
    assert_eq!(
        (r.state, r.posted),
        (State::NotMeasured, false),
        "settled locally, not posted: the next sweep posts it"
    );
    let mut up = gitea_with_run();
    sweep(down.path(), &mut up, "t", &time(1_001, LIVE), &|_, _| {});
    assert_eq!(up.statuses, vec![(1, CONTEXT.to_string(), "error".to_string())]);
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

/// Round 2 P2-4: a holder that is still alive but went silent is ENDED through the pid and start time the record holds, not only
/// cancelled; a holder that is gone is not "ended" again.
#[test]
fn a_silent_holder_that_is_still_alive_is_ended_through_its_held_identity() {
    let silent = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let ended = std::cell::RefCell::new(Vec::new());
    let mut g = gitea_with_run();
    let lines = sweep(silent.path(), &mut g, "t", &time(1_301, LIVE), &|p, s| {
        ended.borrow_mut().push((p, s))
    });
    // `LIVE` answers for any pid, so the held identity of the record (4242, 7) is what is ended.
    assert_eq!(*ended.borrow(), vec![(4242, 7)], "{lines:?}");
    assert_eq!(cancels(&g).len(), 1);
    assert!(lines.iter().any(|l| l.contains("still alive and was ended")), "{lines:?}");

    let gone = dir_with(Some(&on_server(slow(State::Running))), Some(&dispatched()));
    let ended = std::cell::RefCell::new(Vec::new());
    sweep(gone.path(), &mut gitea_with_run(), "t", &time(1_000, GONE), &|p, s| {
        ended.borrow_mut().push((p, s))
    });
    assert!(ended.borrow().is_empty(), "a dead holder is not ended again");
}

/// Progress, not a total budget: a record that keeps beating is never judged silent, however old the push; silence is.
#[test]
fn a_record_that_keeps_beating_is_never_cancelled_and_a_silent_one_is() {
    let d = tempfile::tempdir().unwrap();
    write_slow(
        d.path(),
        &Slow {
            beat: 100,
            ..on_server(slow(State::Running))
        },
    )
    .unwrap();
    let long_after = 100 + 24 * 3600;
    assert!(matches!(
        judge(&read_slow(d.path(), SHA).unwrap().unwrap(), 100 + STALE_AFTER_SECS, LIVE),
        Verdict::Pending
    ));
    beat(d.path(), SHA, long_after - 10).unwrap();
    let r = read_slow(d.path(), SHA).unwrap().unwrap();
    assert!(
        matches!(judge(&r, long_after, LIVE), Verdict::Pending),
        "a day in the queue, still beating"
    );
    assert!(
        matches!(judge(&r, long_after - 10 + STALE_AFTER_SECS + 1, LIVE), Verdict::Cancel(_)),
        "silence is judged"
    );
    write_slow(d.path(), &slow(State::Failed)).unwrap();
    beat(d.path(), SHA, 5).unwrap();
    assert_eq!(
        read_slow(d.path(), SHA).unwrap().unwrap().state,
        State::Failed,
        "a settled record is never beaten back to running"
    );
}

/// Round 2 P1-3: a final record whose status post did not land is posted again by the next sweep (the `posted` flag).
#[test]
fn a_final_status_that_did_not_land_is_posted_again_by_the_next_sweep() {
    let d = tempfile::tempdir().unwrap();
    write_slow(
        d.path(),
        &Slow {
            posted: false,
            ..on_server(failed())
        },
    )
    .unwrap();
    let mut down = FakeGitea {
        status_post_fails: true,
        ..Default::default()
    };
    let mut rec = read_slow(d.path(), SHA).unwrap().unwrap();
    assert!(post_final(d.path(), &mut down, "t", &mut rec).is_err());
    assert!(
        !read_slow(d.path(), SHA).unwrap().unwrap().posted,
        "a post that failed is not marked posted"
    );
    let lines = sweep(d.path(), &mut down, "t", &time(1_000, LIVE), &|_, _| {});
    assert!(lines.iter().any(|l| l.contains("still not posted")), "{lines:?}");
    let mut up = FakeGitea::default();
    let lines = sweep(d.path(), &mut up, "t", &time(1_000, LIVE), &|_, _| {});
    assert_eq!(up.statuses, vec![(1, CONTEXT.to_string(), "failure".to_string())], "{lines:?}");
    assert!(read_slow(d.path(), SHA).unwrap().unwrap().posted);
    assert!(
        sweep(d.path(), &mut up, "t", &time(1_001, LIVE), &|_, _| {}).is_empty(),
        "posted once, not again"
    );
    assert_eq!(up.statuses.len(), 1);
}

/// Round 2 P2-3: a full-gate pass on a red commit clears its record and posts `success`, so the "run ./dev check --pre-push" step
/// of a failed trial is true; a failed post leaves a passed record that the sweep posts.
#[test]
fn a_full_gate_pass_clears_a_red_record_and_posts_success() {
    let d = tempfile::tempdir().unwrap();
    write_slow(d.path(), &on_server(failed())).unwrap();
    let mut g = FakeGitea {
        statuses: vec![stat(1, CONTEXT, "failure")],
        ..Default::default()
    };
    let line = clear_after_full_pass(d.path(), &mut g, "t", SHA)
        .unwrap()
        .expect("something to clear");
    assert!(line.contains("`success`"), "{line}");
    let r = read_slow(d.path(), SHA).unwrap().unwrap();
    assert_eq!((r.state, r.posted, r.row.as_str()), (State::Passed, true, ""));
    assert_eq!(
        gate(g.statuses.clone(), Some(d.path())),
        Ok(None),
        "the commit's evidence counts again"
    );
    assert_eq!(
        clear_after_full_pass(d.path(), &mut g, "t", SHA).unwrap(),
        None,
        "nothing left to clear"
    );
    assert_eq!(
        clear_after_full_pass(tempfile::tempdir().unwrap().path(), &mut g, "t", SHA).unwrap(),
        None,
        "no record, nothing to do"
    );

    write_slow(d.path(), &on_server(failed())).unwrap();
    let mut down = FakeGitea {
        status_post_fails: true,
        ..Default::default()
    };
    assert!(clear_after_full_pass(d.path(), &mut down, "t", SHA).is_err());
    let r = read_slow(d.path(), SHA).unwrap().unwrap();
    assert_eq!((r.state, r.posted), (State::Passed, false), "passed locally, not yet on the server");
    let mut up = FakeGitea::default();
    sweep(d.path(), &mut up, "t", &time(1_000, LIVE), &|_, _| {});
    assert_eq!(up.statuses, vec![(1, CONTEXT.to_string(), "success".to_string())]);
}

/// Round 3 P2-3: a server that returns fewer than `limit` items per page (its own cap) must not end the paging on a short page:
/// that would read the rest as absent, which fails open. Paging ends on an empty page.
#[test]
fn a_server_page_cap_below_the_limit_does_not_hide_the_slow_rows_status() {
    let mut statuses = vec![stat(1, CONTEXT, "failure")];
    statuses.extend(filler(10, 45));
    let mut g = FakeGitea {
        statuses,
        status_page_cap: Some(30),
        ..Default::default()
    };
    let mut client = Client::new(&mut g, "https://gitea.invalid", "o/r", "t");
    client.backoff_ms = 0;
    assert_eq!(
        client.slow_rows_status(SHA),
        Ok(Some("failure".to_string())),
        "found on the second, short page"
    );
    let mut none = FakeGitea {
        statuses: filler(10, 45),
        status_page_cap: Some(30),
        ..Default::default()
    };
    let mut client = Client::new(&mut none, "https://gitea.invalid", "o/r", "t");
    assert_eq!(client.slow_rows_status(SHA), Ok(None), "absent only after an empty page");
}
