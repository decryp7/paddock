//! The split push profile's background phase with the real binary (ADR 0114): started exactly as the hook starts it, from nowhere
//! near the repository, it must find its repository, measure the pushed commit and leave its verdict. A review found that the child
//! ran in the logs folder inside `.git`, died with "This is not a git checkout" and left its record running for ever.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::pre_push::trial_profile::{child_args, start_slow};
use paddock_dev::trial_guard::{self, Slow, State};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Ends the spawned child through the identity the starter holds, whatever the test does (a panic included): a test never leaves a
/// process behind (ER-14, ER-51).
struct Reaper {
    pid: u32,
    start: u64,
}

impl Drop for Reaper {
    fn drop(&mut self) {
        let id = paddock_proc::ProcessId::new(self.pid, paddock_proc::StartTime::from_micros(self.start));
        if id.may_be_alive() {
            paddock_proc::kill_tree(id);
        }
    }
}

struct Phase {
    rec: Slow,
    log: String,
    sha: String,
    pid: u32,
    start: u64,
    _keep: tempfile::TempDir,
    _reaper: Reaper,
}

/// A repository of two commits, the record the hook would have written, and the slow phase started the way the hook starts it
/// (`extra` goes on its command line: a row selection a hook never passes, to make a case that passes). Returns once the child's
/// record is no longer running, or panics after 180 s.
fn run_slow_phase(extra: &[&str]) -> Phase {
    let d = tempfile::tempdir().unwrap();
    let base = paddock_env::winpath::real(d.path()).unwrap();
    let root: PathBuf = base.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    crate::golden::git(&root, &["init", "-q", "-b", "main"]);
    crate::golden::git(&root, &["config", "user.email", "t@t"]);
    crate::golden::git(&root, &["config", "user.name", "t"]);
    std::fs::write(root.join("a.txt"), "1\n").unwrap();
    crate::golden::git(&root, &["add", "-A"]);
    crate::golden::git(&root, &["commit", "-q", "-m", "one"]);
    let first = paddock_dev::plan_repo::git_text(&root, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    std::fs::write(root.join("a.txt"), "2\n").unwrap();
    crate::golden::git(&root, &["commit", "-q", "-am", "two"]);
    let sha = paddock_dev::plan_repo::git_text(&root, &["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    crate::golden::git(&root, &["update-ref", "refs/remotes/origin/main", &first]);
    // The state and logs folders are inside `.git`, as the hook's are: the child must not be started there.
    let state = root.join(".git/paddock-pre-push");
    let logs = state.join("logs/run-1");
    std::fs::create_dir_all(&logs).unwrap();
    // The record the hook wrote before starting the child: running, holder not yet known.
    trial_guard::write_slow(
        &state,
        &Slow {
            sha: sha.clone(),
            state: State::Running,
            row: String::new(),
            why: String::new(),
            next: String::new(),
            pid: 0,
            start_micros: 0,
            beat: u64::MAX,
            posted: false,
            logs: logs.to_string_lossy().into_owned(),
            remote_ref: "refs/heads/wip/x".into(),
            server: String::new(),
            repo: String::new(),
        },
    )
    .unwrap();
    let mut env: HashMap<String, String> = paddock_env::vars().filter(|(k, _)| k != "GITEA_TOKEN").collect();
    env.insert("PADDOCK_SCCACHE".into(), "off".into());
    env.insert("PADDOCK_PREPUSH_WORKTREES".into(), base.join("wt").to_string_lossy().into_owned());
    env.insert("HOME".into(), base.join("home").to_string_lossy().into_owned());
    let stdin = format!("refs/heads/wip/x {sha} refs/heads/wip/x {}\n", "0".repeat(40));
    let mut args = child_args(&first, Some("origin"), &root, &logs);
    args.extend(extra.iter().map(|a| a.to_string()));
    let (pid, start) = start_slow(crate::dev_bin::PATH, &args, &stdin, &root, &logs, &env).unwrap();
    assert!(pid > 1 && start > 0);
    let reaper = Reaper { pid, start };
    // The child ends on its own: wait on its record, bounded.
    let t0 = Instant::now(); // conformance: hang-guard — the wait below is bounded by the 180 s assert, the child's record is its only signal
    let rec = loop {
        let r = trial_guard::read_slow(&state, &sha).unwrap().unwrap();
        if r.state != State::Running {
            break r;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(180),
            "the slow phase left its record running: {r:?}"
        );
        std::thread::sleep(Duration::from_millis(250)); // conformance: hang-guard — bounded by the 180 s assert above
    };
    // The record is final before the process prints its report: read the log once the process has ended.
    while paddock_proc::ProcessId::new(pid, paddock_proc::StartTime::from_micros(start)).may_be_alive() {
        assert!(t0.elapsed() < Duration::from_secs(180), "the slow phase did not exit");
        std::thread::sleep(Duration::from_millis(100)); // conformance: hang-guard — bounded by the 180 s assert above
    }
    let log = std::fs::read_to_string(logs.join("slow-phase.log")).unwrap();
    Phase {
        rec,
        log,
        sha,
        pid,
        start,
        _keep: d,
        _reaper: reaper,
    }
}

fn assert_ran_properly(p: &Phase) {
    assert!(
        !p.log.contains("not a git checkout"),
        "the child must find the repository: {}",
        p.log
    );
    assert!(
        p.log.contains("pre-push: base ") && p.log.contains("checking the pushed commit"),
        "it ran the gate on the pushed commit: {}",
        p.log
    );
    assert!(
        p.log.contains("throw-away worktree"),
        "the slow phase measures a throw-away worktree of the pushed commit, never the live tree: {}",
        p.log
    );
    assert!(
        p.log.contains(&format!("trial {}", &p.sha[..12])),
        "it settled and said so: {}",
        p.log
    );
    assert_eq!(
        (p.rec.pid, p.rec.start_micros),
        (p.pid, p.start),
        "the record keeps the identity of the process that held the rows"
    );
}

/// A slow row that cannot be measured (conformance-tests: nothing is installed in this repository) is a record that is not evidence.
#[test]
fn the_slow_phase_started_as_the_hook_starts_it_runs_rows_writes_its_record_and_settles() {
    let p = run_slow_phase(&[]);
    assert_ran_properly(&p);
    assert_eq!(p.rec.state, State::NotMeasured, "{:?}\n{}", p.rec, p.log);
    assert_eq!(p.rec.row, "conformance-tests");
}

/// The passing case: every slow row of the selection passes (or is skipped as not reached), so the record is `passed` and the
/// settle says the run stays.
#[test]
fn a_slow_phase_whose_rows_all_pass_leaves_a_passed_record() {
    let p = run_slow_phase(&["--only", "member-audits"]);
    assert_ran_properly(&p);
    assert_eq!(p.rec.state, State::Passed, "{:?}\n{}", p.rec, p.log);
    assert!(
        p.log.contains("no CI run is registered"),
        "it settled as a passed record: {}",
        p.log
    );
}
