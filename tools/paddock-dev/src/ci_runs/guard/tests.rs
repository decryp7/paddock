// ci-runs-reader: trial push profile: registers and cancels one dispatched run; no pass/fail verdict on a commit
use super::*;
use crate::trial_guard::tests::FakeGitea as RunsOnly;
use crate::trial_guard::{Slow, State, write_slow};
use crate::wait::Http;
use serde_json::{Value, json};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const LIVE: &dyn Fn(u32, u64) -> bool = &|_, _| true;
const T: Target<'static> = Target {
    server: "https://gitea.invalid",
    repo: "o/r",
    token: "t",
};

fn time(now: u64) -> Time<'static> {
    Time { now, alive: LIVE }
}

fn req() -> Request {
    super::super::dispatch::parse_request("ci.yml", "wip/x", &[], "1m", None).unwrap()
}

/// A fake Gitea for the dispatch side: a branch head, a run list that gains the dispatched run, the runs by id and the writes.
struct Gitea {
    head: Option<&'static str>,
    /// Run lists, one per look; the last repeats.
    lists: Vec<Vec<(u64, &'static str)>>,
    looks: usize,
    inner: RunsOnly,
}

impl Http for Gitea {
    fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String> {
        if url.contains("/branches/") {
            return self
                .head
                .map(|h| json!({"commit": {"id": h}}))
                .ok_or_else(|| "curl: (22) The requested URL returned error: 404".into());
        }
        if url.contains("/runs?") {
            let l = self.lists[self.looks.min(self.lists.len() - 1)].clone();
            self.looks += 1;
            return Ok(json!({"total_count": l.len(), "workflow_runs": l.iter().map(|(id, wf)| json!({
                "id": id, "path": format!("{wf}@refs/heads/wip/x"), "status": "queued", "event": "workflow_dispatch", "head_sha": SHA
            })).collect::<Vec<_>>()}));
        }
        self.inner.get_json(url, token)
    }
    fn send_json(&mut self, m: &str, u: &str, t: &str, b: Option<&Value>) -> Result<Value, String> {
        self.inner.send_json(m, u, t, b)
    }
}

fn gitea(head: Option<&'static str>, lists: Vec<Vec<(u64, &'static str)>>) -> Gitea {
    Gitea {
        head,
        lists,
        looks: 0,
        inner: RunsOnly {
            runs: vec![(9001, "in_progress")],
            ..Default::default()
        },
    }
}

fn rec(state: State) -> Slow {
    Slow {
        sha: SHA.into(),
        state,
        row: "doctest crates".into(),
        why: "boom".into(),
        next: "fix it".into(),
        pid: 5,
        start_micros: 6,
        beat: u64::MAX,
        posted: false,
        orphaned: false,
        logs: "/l".into(),
        remote_ref: String::new(),
        server: String::new(),
        repo: String::new(),
    }
}

struct Clk(Duration);
impl Clock for Clk {
    fn elapsed(&self) -> Duration {
        self.0
    }
    fn sleep(&mut self, d: Duration) {
        self.0 += d;
    }
}

#[test]
fn only_wip_and_trial_refs_can_be_guarded() {
    assert_eq!(guarded_branch("wip/x"), Ok("wip/x".into()));
    assert_eq!(guarded_branch("refs/heads/trial/a/b"), Ok("trial/a/b".into()));
    for full in ["main", "train/x", "refs/heads/train/x", "feat/x", "v1.2.3"] {
        assert!(guarded_branch(full).unwrap_err().contains("full push gate"), "{full}");
    }
}

#[test]
fn a_condemned_commit_is_not_dispatched_at_all() {
    let d = tempfile::tempdir().unwrap();
    write_slow(d.path(), &rec(State::Failed)).unwrap();
    let mut g = gitea(Some(SHA), vec![vec![]]);
    let got = before(&mut g, &T, d.path(), "wip/x", &time(1)).unwrap();
    assert!(
        matches!(&got, Before::Refuse(why) if why.contains("doctest crates") && why.contains("Next: fix it")),
        "{got:?}"
    );
    assert!(g.inner.writes.is_empty());
}

#[test]
fn no_record_dispatches_unguarded_with_a_warning_and_a_missing_branch_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let mut g = gitea(Some(SHA), vec![vec![]]);
    let got = before(&mut g, &T, d.path(), "wip/x", &time(1)).unwrap();
    assert!(
        matches!(&got, Before::Unguarded(why) if why.contains("nothing cancels this run")),
        "{got:?}"
    );
    let mut none = gitea(None, vec![vec![]]);
    assert!(before(&mut none, &T, d.path(), "wip/x", &time(1)).unwrap_err().contains("not on"));
    assert!(before(&mut g, &T, d.path(), "main", &time(1)).is_err());
}

/// The dispatch side end to end: the run that appears after the dispatch is registered, and the slow rows that have failed meanwhile
/// cancel it at once.
#[test]
fn a_run_found_after_the_dispatch_is_registered_and_cancelled_when_the_rows_failed_meanwhile() {
    let d = tempfile::tempdir().unwrap();
    write_slow(d.path(), &rec(State::Running)).unwrap();
    let mut g = gitea(
        Some(SHA),
        vec![vec![(5, "ci.yml")], vec![(5, "ci.yml")], vec![(9001, "ci.yml"), (5, "ci.yml")]],
    );
    let Before::Guarded(guard) = before(&mut g, &T, d.path(), "wip/x", &time(1)).unwrap() else {
        panic!("guarded")
    };
    // The rows fail while the dispatch waits for a runner.
    write_slow(d.path(), &rec(State::Failed)).unwrap();
    let mut clock = Clk(Duration::ZERO);
    let line = register(&guard, &mut g, &T, &req(), &mut clock, &time(2), &mut Vec::new());
    assert!(line.contains("run 9001 is registered") && line.contains("was cancelled"), "{line}");
    assert!(
        g.inner.writes.iter().any(|(_, u)| u.ends_with("/runs/9001/cancel")),
        "{:?}",
        g.inner.writes
    );
    assert_eq!(
        clock.0,
        Duration::from_secs(4),
        "it looked again once, bounded, until the run showed"
    );
    assert_eq!(trial_guard::read_dispatched(d.path(), SHA).unwrap().unwrap().run_id, 9001);
}

/// A run that never shows in the list is said loudly as not guarded: the tool never guesses "the newest run of the branch".
#[test]
fn a_run_that_never_shows_is_reported_as_not_guarded() {
    let d = tempfile::tempdir().unwrap();
    write_slow(d.path(), &rec(State::Running)).unwrap();
    let mut g = gitea(Some(SHA), vec![vec![(5, "ci.yml")]]);
    let Before::Guarded(guard) = before(&mut g, &T, d.path(), "wip/x", &time(1)).unwrap() else {
        panic!("guarded")
    };
    let line = register(&guard, &mut g, &T, &req(), &mut Clk(Duration::ZERO), &time(2), &mut Vec::new());
    assert!(line.contains("::warning::") && line.contains("NOT guarded"), "{line}");
    assert!(g.inner.writes.is_empty());
    assert!(trial_guard::read_dispatched(d.path(), SHA).unwrap().is_none());
}

#[test]
fn this_checkouts_failed_record_refuses_without_a_server() {
    let d = tempfile::tempdir().unwrap();
    assert!(trial_guard::local_refusal(d.path(), SHA).is_none());
    write_slow(d.path(), &rec(State::Failed)).unwrap();
    assert!(trial_guard::local_refusal(d.path(), SHA).is_some());
}

/// Round 3 P2-8 (CQS, ER-45): `ci verdict` only reads. A running record whose holder is gone is left exactly as it is, and nothing
/// is written to the server; only a gate start and `ci dispatch` settle such records.
#[test]
fn ci_verdict_is_read_only_and_leaves_a_dead_holders_record_alone() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    assert!(crate::plan_repo::git_text(&root, &["init", "-q"]).is_some());
    let state = crate::pre_push::state_dir(&root);
    write_slow(
        &state,
        &Slow {
            pid: 999_999_999,
            start_micros: 1,
            beat: 1,
            ..rec(State::Running)
        },
    )
    .unwrap();
    let mut f = crate::ci_runs::testutil::Fake::default();
    let a = super::super::VerdictArgs::for_test(SHA);
    let report = super::super::verdict_with(&a, &root, Some("t"), &mut f);
    assert!(report.is_ok());
    let after = trial_guard::read_slow(&state, SHA).unwrap().unwrap();
    assert_eq!(after.state, State::Running, "a query must not settle a record: {after:?}");
}

/// With the run id the dispatch itself returned, no run list is read: the handle is held, not found (ER-51).
#[test]
fn a_run_id_the_dispatch_returned_is_registered_without_looking_for_it() {
    let d = tempfile::tempdir().unwrap();
    write_slow(d.path(), &rec(State::Running)).unwrap();
    // The list is an error: reading it would fail the lookup, so a registered run proves it was not read.
    let mut g = gitea(Some(SHA), vec![vec![]]);
    let Before::Guarded(mut guard) = before(&mut g, &T, d.path(), "wip/x", &time(1)).unwrap() else {
        panic!("guarded")
    };
    guard.known_run = Some(9001);
    let mut clock = Clk(Duration::ZERO);
    let looks = g.looks;
    let line = register(&guard, &mut g, &T, &req(), &mut clock, &time(2), &mut Vec::new());
    assert!(line.contains("run 9001 is registered"), "{line}");
    assert_eq!(g.looks, looks, "no run list was read");
    assert_eq!(clock.0, Duration::ZERO, "and nothing waited");
    assert_eq!(trial_guard::read_dispatched(d.path(), SHA).unwrap().unwrap().run_id, 9001);
}
