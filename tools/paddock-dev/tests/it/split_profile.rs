//! The split push profile's background phase with the real binary (ADR 0114): started exactly as the hook starts it, from nowhere
//! near the repository, it must find its repository, measure the pushed commit and leave its verdict. A review found that the child
//! ran in the logs folder inside `.git`, died with "This is not a git checkout" and left its record running for ever.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::pre_push::trial_profile::{child_args, start_slow};
use paddock_dev::trial_guard::{self, Slow, State};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[test]
fn the_slow_phase_started_as_the_hook_starts_it_runs_rows_writes_its_record_and_settles() {
    let d = tempfile::tempdir().unwrap();
    let base = paddock_env::winpath::real(d.path()).unwrap();
    let root = base.join("repo");
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
    // The record the hook wrote before starting the child: running, deadline far away, holder not yet known.
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
            finish_by: u64::MAX,
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
    let args = child_args(&first, Some("origin"), &root, &logs);
    let (pid, start) = start_slow(crate::dev_bin::PATH, &args, &stdin, &root, &logs, &env).unwrap();
    assert!(pid > 1 && start > 0);
    // The child ends on its own: wait on its record, bounded.
    let t0 = Instant::now();
    let rec = loop {
        let r = trial_guard::read_slow(&state, &sha).unwrap().unwrap();
        if r.state != State::Running {
            break r;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(180),
            "the slow phase left its record running: {r:?}"
        );
        std::thread::sleep(Duration::from_millis(250)); // conformance: hang-guard — the detached child ends on its own and its record is the only signal; bounded by the 180 s assert above
    };
    let log = std::fs::read_to_string(logs.join("slow-phase.log")).unwrap();
    assert!(!log.contains("not a git checkout"), "the child must find the repository: {log}");
    assert!(
        log.contains("pre-push: base ") && log.contains("checking the pushed commit"),
        "it ran the gate on the pushed commit: {log}"
    );
    assert!(
        log.contains("throw-away worktree"),
        "the slow phase measures a throw-away worktree of the pushed commit, never the live tree: {log}"
    );
    assert!(log.contains(&format!("trial {}", &sha[..12])), "it settled and said so: {log}");
    assert_ne!(rec.state, State::Running);
    assert_eq!(
        (rec.pid, rec.start_micros),
        (pid, start),
        "the record keeps the identity of the process that held the rows"
    );
}
