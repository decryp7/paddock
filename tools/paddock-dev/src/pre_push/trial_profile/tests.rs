use super::*;
use crate::pre_push::exec::Row;
use crate::trial_guard::{Slow, State};

fn line(remote_ref: &str) -> String {
    format!("refs/heads/x {} {remote_ref} {}\n", "a".repeat(40), "0".repeat(40))
}

#[test]
fn only_a_push_of_wip_and_trial_refs_is_covered() {
    assert!(covers(&line("refs/heads/wip/a")));
    assert!(covers(&line("refs/heads/trial/feat/x")));
    for full in [
        "refs/heads/train/2026-11-22-1",
        "refs/heads/main",
        "refs/heads/feat/x",
        "refs/heads/wip",
        "refs/heads/wip/",
        "refs/heads/wipe/x",
        "refs/tags/v1.2.3",
        "refs/tags/wip/x",
    ] {
        assert!(!covers(&line(full)), "{full} keeps the full gate");
    }
    // One landing ref among trial refs, in either order, even for the very same commit, keeps the full gate.
    let wip = line("refs/heads/wip/a");
    for mixed in [
        format!("{wip}{}", line("refs/heads/main")),
        format!("{}{wip}", line("refs/heads/train/x")),
    ] {
        assert!(!covers(&mixed), "{mixed}");
    }
    // A deletion sends nothing, so it neither covers nor spoils a push; a push of nothing is not covered.
    let delete = format!("(delete) {} refs/heads/wip/old {}\n", "0".repeat(40), "b".repeat(40));
    assert!(!covers(&delete));
    assert!(covers(&format!("{wip}{delete}")));
    assert!(!covers(""));
}

#[test]
fn train_and_main_pushes_are_the_full_gate_and_a_wip_push_the_fast_phase() {
    for full in ["refs/heads/main", "refs/heads/train/x", "refs/heads/feat/x"] {
        assert_eq!(phase_for(None, true, false, false, &line(full)), None, "{full}");
    }
    assert_eq!(phase_for(None, true, false, false, &line("refs/heads/wip/a")), Some(Phase::Fast));
    // The member gate, a row selection and a person's run (no hook) are never the profile; the background child names its phase.
    assert_eq!(phase_for(None, true, true, false, &line("refs/heads/wip/a")), None);
    assert_eq!(phase_for(None, true, false, true, &line("refs/heads/wip/a")), None);
    assert_eq!(phase_for(None, false, false, false, &line("refs/heads/wip/a")), None);
    assert_eq!(
        phase_for(Some(Phase::Slow), true, false, false, &line("refs/heads/wip/a")),
        Some(Phase::Slow)
    );
}

fn skip(id: &str) -> Planned {
    Planned::Skip {
        id: id.into(),
        reason: "r".into(),
    }
}

#[test]
fn every_planned_row_goes_to_exactly_one_phase_and_an_unknown_row_is_slow() {
    let ids = [
        "fmt crates",
        "lockfile apps/dyno",
        "commit-lint",
        "conformance",
        "gates-register",
        "ipc-endpoints",
        "pitwall-deny (licences, bans, sources)",
        "pitwall-scripts",
        "clippy crates",
        "clippy-windows crates",
        "clippy-linux crates",
        "doctest crates",
        "conformance-tests",
        "member-audits",
        "dyno-wire-fixtures",
        "a-row-added-next-year",
    ];
    let all: Vec<Planned> = ids.iter().map(|i| skip(i)).collect();
    let id_of = |p: &Planned| planned_id(p).to_string();
    let fast: Vec<String> = rows_of(all.clone(), Phase::Fast).iter().map(id_of).collect();
    let slow: Vec<String> = rows_of(all, Phase::Slow).iter().map(id_of).collect();
    assert_eq!(fast.len() + slow.len(), ids.len(), "nothing lost, nothing twice");
    for want in ["fmt crates", "conformance", "clippy crates", "commit-lint", "lockfile apps/dyno"] {
        assert!(fast.contains(&want.to_string()), "{want} is fast: {fast:?}");
    }
    for want in [
        "clippy-windows crates",
        "clippy-linux crates",
        "doctest crates",
        "conformance-tests",
        "member-audits",
        "a-row-added-next-year",
    ] {
        assert!(slow.contains(&want.to_string()), "{want} is slow: {slow:?}");
    }
}

/// A slow row that cannot be measured is planned as NotMeasured: it must land in the slow phase, where it cancels the trial.
#[test]
fn a_not_measured_or_broken_slow_row_is_planned_into_the_slow_phase() {
    let nm = Planned::NotMeasured {
        id: "clippy-linux crates".into(),
        reason: "no docker".into(),
        next: "start it".into(),
    };
    let broken = Planned::Broken {
        id: "member-audits".into(),
        cause: "x".into(),
        next: "y".into(),
    };
    let all = vec![nm, broken, skip("fmt crates")];
    assert_eq!(rows_of(all.clone(), Phase::Slow).len(), 2);
    assert_eq!(rows_of(all, Phase::Fast).len(), 1);
}

fn base() -> Slow {
    Slow {
        sha: "ab".repeat(20),
        state: State::Running,
        row: String::new(),
        why: String::new(),
        next: String::new(),
        pid: 5,
        start_micros: 6,
        beat: 7,
        posted: false,
        orphaned: false,
        logs: "/l".into(),
        remote_ref: String::new(),
        server: String::new(),
        repo: String::new(),
    }
}

fn failing(id: &str) -> (String, Row) {
    let subject = crate::pre_push::exec::test_subject();
    (
        id.into(),
        Row::failed(id, &subject, "it failed".into(), "tail".into(), format!("fix {id}"), 1.0),
    )
}

fn not_measured(id: &str) -> (String, Row) {
    (id.into(), Row::not_measured("no runner", format!("re-run {id}")))
}

/// ER-33: only all-passed slow rows give `Passed`; a failure, a broken check, a not-measured row, an excused one and an empty phase
/// give something that cancels.
#[test]
fn the_slow_record_passes_only_when_every_slow_row_passed() {
    let sha = base().sha;
    let ok = |id: &str| (id.to_string(), Row::skipped("not reached"));
    let none_ran = fold(&sha, &[ok("doctest crates")], &base());
    assert_eq!(none_ran.state, State::Passed, "skipped rows are not findings");
    assert_eq!((none_ran.pid, none_ran.start_micros), (5, 6), "the holder stays in the record");

    let failed = fold(
        &sha,
        &[ok("a"), failing("clippy-linux crates"), not_measured("doctest crates")],
        &base(),
    );
    assert_eq!(failed.state, State::Failed, "a failure outranks a not-measured row");
    assert_eq!(failed.row, "clippy-linux crates");
    assert!(
        failed.why.contains("it failed") && failed.next == "fix clippy-linux crates",
        "{failed:?}"
    );

    let nm = fold(&sha, &[ok("a"), not_measured("clippy-windows crates")], &base());
    assert_eq!((nm.state, nm.row.as_str()), (State::NotMeasured, "clippy-windows crates"));

    let mut excused = not_measured("clippy-windows crates");
    excused.1.excused_by = Some("PADDOCK_ACCEPT_NOT_MEASURED".into());
    assert_eq!(
        fold(&sha, &[excused], &base()).state,
        State::NotMeasured,
        "an override never makes a trial's evidence"
    );

    let empty = fold(&sha, &[], &base());
    assert_eq!(empty.state, State::NotMeasured, "a slow phase that ran no row is no evidence");
}

#[test]
fn the_child_runs_the_slow_phase_of_the_same_commit_with_the_hooks_log_folder() {
    let a = child_args("abc123", Some("origin"), Path::new("/repo"), Path::new("/logs/run-1"));
    let joined = a.join(" ");
    assert!(
        joined.starts_with("check --pre-push --refs-from-stdin --split-phase slow --base abc123"),
        "{joined}"
    );
    assert!(
        joined.contains("--remote origin") && joined.ends_with("--split-logs /logs/run-1"),
        "{joined}"
    );
}

/// The real start, with a stand-in program: it returns the child's identity at once, gives it no pipe of the hook and leaves its
/// output and its input in the log folder.
#[cfg(unix)]
#[test]
fn the_slow_phase_starts_detached_with_its_output_and_input_in_the_log_folder() {
    let d = tempfile::tempdir().unwrap();
    let env: std::collections::HashMap<String, String> = [("PATH".to_string(), "/usr/bin:/bin".to_string())].into();
    let (pid, start) = start_slow("cat", &[], "pushed lines\n", d.path(), d.path(), &env).unwrap();
    assert!(pid > 1 && start > 0, "{pid} {start}");
    // The input is the pushed lines in a file of the log folder and the output goes to a log file there, never to git's pipes.
    assert_eq!(
        std::fs::read_to_string(d.path().join("slow-phase.stdin")).unwrap(),
        "pushed lines\n"
    );
    assert!(d.path().join("slow-phase.log").exists());
    assert!(start_slow("/nonexistent/program", &[], "", d.path(), d.path(), &env).is_err());
}

#[test]
fn a_phase_named_for_a_landing_push_is_not_honoured() {
    for landing in ["refs/heads/main", "refs/heads/train/x", "refs/heads/feat/x"] {
        for given in [Phase::Fast, Phase::Slow] {
            assert_eq!(
                phase_for(Some(given), true, false, false, &line(landing)),
                None,
                "{given:?} on {landing}"
            );
        }
    }
    assert_eq!(
        phase_for(Some(Phase::Slow), true, false, false, &line("refs/heads/wip/a")),
        Some(Phase::Slow)
    );
}

#[test]
fn the_status_server_and_repository_come_from_the_remotes_url() {
    for (url, want) in [
        (
            "https://dev.decryptology.net/decryp7/paddock.git",
            Some(("https://dev.decryptology.net", "decryp7/paddock")),
        ),
        ("https://h.example/o/n", Some(("https://h.example", "o/n"))),
        ("git@h.example:o/n.git\n", Some(("https://h.example", "o/n"))),
        ("/some/path/repo", None),
        ("https://h.example/onlyone", None),
        ("https://h.example/a/b/c", None),
        ("https://h.example/a/b\"x", None),
    ] {
        assert_eq!(server_repo(url).as_ref().map(|(s, r)| (s.as_str(), r.as_str())), want, "{url}");
    }
    let d = tempfile::tempdir().unwrap();
    assert!(status_target(d.path(), Some("origin"), None).unwrap_err().contains("GITEA_TOKEN"));
    assert!(
        status_target(d.path(), Some("origin"), Some(""))
            .unwrap_err()
            .contains("GITEA_TOKEN")
    );
    assert!(
        status_target(d.path(), Some("origin"), Some("t")).is_err(),
        "not a repository: no URL"
    );
}

#[test]
fn the_child_gets_the_repository_root_and_the_phase_flag_is_not_called_trial() {
    let a = child_args("abc", None, Path::new("/the/repo"), Path::new("/logs/x"));
    let pos = a.iter().position(|x| x == "--root").expect("--root");
    assert_eq!(a[pos + 1], "/the/repo");
    assert!(
        a.contains(&"--split-phase".to_string()) && !a.iter().any(|x| x.contains("trial")),
        "{a:?}"
    );
}

/// Round 2 P2: the fast phase probes write access first; a token that cannot write statuses means the full gate runs.
#[test]
fn the_fast_phase_needs_a_status_target_and_a_token_that_can_write_statuses() {
    use crate::trial_guard::tests::FakeGitea;
    let target = || Ok(("https://gitea.invalid".to_string(), "o/r".to_string()));
    let probe = |push: Option<bool>| {
        move |server: &str, repo: &str| {
            probe_with(
                &mut FakeGitea {
                    repo_push: push,
                    ..Default::default()
                },
                server,
                repo,
                "t",
            )
        }
    };
    assert_eq!(
        fast_allowed(target(), &probe(Some(true))),
        Ok(("https://gitea.invalid".into(), "o/r".into()))
    );
    assert!(
        fast_allowed(target(), &probe(Some(false)))
            .unwrap_err()
            .contains("no write permission")
    );
    assert!(fast_allowed(target(), &probe(None)).unwrap_err().contains("could not be checked"));
    assert_eq!(
        fast_allowed(Err("no token".into()), &|_, _| panic!("no probe without a target")),
        Err("no token".into())
    );
}

/// Round 3 P2-5: the pending post reads the record again on each try and stops once it is not running, so a stale copy can never
/// post `pending` after the final status.
#[test]
fn the_pending_post_reads_the_record_again_each_try_and_stops_once_it_is_not_running() {
    let sha = "ab".repeat(20);
    let d = tempfile::tempdir().unwrap();
    let rec = |state| Slow {
        sha: sha.clone(),
        state,
        row: String::new(),
        why: String::new(),
        next: String::new(),
        pid: 1,
        start_micros: 1,
        beat: u64::MAX,
        posted: false,
        orphaned: false,
        logs: String::new(),
        remote_ref: String::new(),
        server: "https://gitea.invalid".into(),
        repo: "o/r".into(),
    };
    trial_guard::write_slow(d.path(), &rec(State::Running)).unwrap();
    let posts = std::cell::Cell::new(0);
    let got = pending_loop(
        d.path(),
        &sha,
        |_| {
            posts.set(posts.get() + 1);
            Err("404".to_string())
        },
        |_| {
            // Between two tries the rows end and the final record is written.
            trial_guard::write_slow(d.path(), &rec(State::Failed)).unwrap();
            false
        },
        5,
    );
    assert_eq!(posts.get(), 1, "no second post once the record is settled: {got}");
    assert!(got.contains("no longer running"), "{got}");
    // A running record posts once and stops; a pause that says "ended" stops the loop.
    trial_guard::write_slow(d.path(), &rec(State::Running)).unwrap();
    assert!(pending_loop(d.path(), &sha, |_| Ok(true), |_| false, 5).contains("is pending"));
    assert!(pending_loop(d.path(), &sha, |_| Err("x".into()), |_| true, 5).contains("rows ended first"));
}

/// Round 3 P2-6: the run is cancelled BEFORE the final status is posted (a post that fails or stalls must not delay the cancel).
#[test]
fn the_run_is_cancelled_before_the_final_status_is_posted() {
    use crate::trial_guard::tests::FakeGitea;
    use crate::trial_guard::{Dispatched, Slow, State};
    let sha = "ab".repeat(20);
    let d = tempfile::tempdir().unwrap();
    trial_guard::write_slow(
        d.path(),
        &Slow {
            sha: sha.clone(),
            state: State::Failed,
            row: "doctest crates".into(),
            why: "boom".into(),
            next: "fix".into(),
            pid: 1,
            start_micros: 1,
            beat: u64::MAX,
            posted: false,
            orphaned: false,
            logs: String::new(),
            remote_ref: String::new(),
            server: "https://gitea.invalid".into(),
            repo: "o/r".into(),
        },
    )
    .unwrap();
    trial_guard::write_dispatched(
        d.path(),
        &Dispatched {
            sha: sha.clone(),
            run_id: 9001,
            workflow: "ci.yml".into(),
            git_ref: "wip/x".into(),
            server: "https://gitea.invalid".into(),
            repo: "o/r".into(),
        },
    )
    .unwrap();
    let mut g = FakeGitea {
        runs: vec![(9001, "in_progress")],
        ..Default::default()
    };
    let line = settle_then_post(
        d.path(),
        &sha,
        &mut g,
        "t",
        1_000,
        &|_, _| true,
        &mut Retry {
            tries: 3,
            pause: &mut |_| {},
        },
    );
    let order: Vec<&str> = g
        .writes
        .iter()
        .map(|(_, u)| if u.contains("/cancel") { "cancel" } else { "status" })
        .collect();
    assert_eq!(order, ["cancel", "status"], "{line}");
}

/// Round 3 minor: the heartbeat is paced by the clock, not by counting sleeps: it is due when `every` of wall time has passed since
/// the last beat (a stalled sleep or a slow write never makes beats pile up or go missing).
#[test]
fn the_heartbeat_is_due_by_wall_time_not_by_counted_sleeps() {
    let every = Duration::from_secs(30);
    let t0 = std::time::Instant::now();
    assert!(beat_due(None, t0, every), "the first beat is at once");
    assert!(!beat_due(Some(t0), t0 + Duration::from_secs(29), every));
    assert!(beat_due(Some(t0), t0 + Duration::from_secs(30), every));
    assert!(
        beat_due(Some(t0), t0 + Duration::from_secs(300), every),
        "a long stall is caught up with one beat"
    );
}
