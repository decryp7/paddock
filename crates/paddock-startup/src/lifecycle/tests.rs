use super::*;
use crate::session::SessionInfo;
use serde_json::json;
use std::cell::RefCell;
use std::collections::HashMap;

/// A fake process table: pid to start time.
#[derive(Default)]
struct Procs(RefCell<HashMap<u32, u64>>);

impl Procs {
    fn with(self, pid: u32, started: u64) -> Self {
        self.0.borrow_mut().insert(pid, started);
        self
    }
    fn gone(&self, pid: u32) {
        self.0.borrow_mut().remove(&pid);
    }
    fn probe(&self) -> impl Fn(u32) -> Option<u64> + '_ {
        move |pid| self.0.borrow().get(&pid).copied()
    }
}

fn start(dir: &Path) -> Start<'_> {
    Start {
        data_dir: dir,
        process: "app",
        mode: Mode::Normal,
        scope: InstanceScope::Main,
        app_version: "1.2.3",
    }
}

fn begin(dir: &Path, procs: &Procs, pid: u32) -> Lifecycle {
    Lifecycle::begin_with(start(dir), pid, &procs.probe())
}

fn begin_as(dir: &Path, procs: &Procs, pid: u32, mode: Mode, scope: InstanceScope) -> Lifecycle {
    Lifecycle::begin_with(
        Start {
            mode,
            scope,
            ..start(dir)
        },
        pid,
        &procs.probe(),
    )
}

fn write_crash(dir: &Path, pid: u32) {
    let marker = CrashMarker {
        version: CrashMarker::VERSION,
        process: "app".into(),
        pid,
        time: "2026-10-24T10:00:00.000Z".into(),
        thread: "worker-3".into(),
        message: "secret C:\\Users\\someone\\file.log".into(),
        location: "x.rs:9".into(),
        log_file: None,
    };
    std::fs::write(
        CrashMarker::path_for(dir, "app"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
}

#[test]
fn a_clean_run_leaves_no_marker_and_the_next_start_reports_nothing() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    let run = begin(t.path(), &procs, 100);
    assert_eq!(run.previous_run(), PreviousRun::Clean);
    let on_disk = std::fs::read_to_string(t.path().join(MARKER_FILE)).unwrap();
    assert!(
        on_disk.contains("\"pid\": 100") && on_disk.contains("\"startedAt\": 5000"),
        "{on_disk}"
    );
    assert!(on_disk.contains("\"version\": 1") && on_disk.contains("1.2.3"));
    run.end();
    assert!(!t.path().join(MARKER_FILE).exists());
    procs.gone(100);
    let next = begin(t.path(), procs.with_pid(101, 6000), 101);
    assert_eq!(next.previous_run(), PreviousRun::Clean);
    assert_eq!(next.report(true).unclean, None);
}

impl Procs {
    fn with_pid(&self, pid: u32, started: u64) -> &Self {
        self.0.borrow_mut().insert(pid, started);
        self
    }
}

#[test]
fn killing_the_process_leaves_the_marker_and_the_next_start_offers_restore_and_safe_mode() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    let run = begin(t.path(), &procs, 100);
    run.checkpoint("main", json!({"query": {"text": "error"}}))
        .unwrap();
    run.flush();
    // Killed: no end(). The process is gone.
    std::mem::forget(run);
    procs.gone(100);
    assert!(t.path().join(MARKER_FILE).exists());

    let next = begin(t.path(), procs.with_pid(200, 9000), 200);
    assert_eq!(next.previous_run(), PreviousRun::Crashed);
    let unclean = next.report(true).unclean.expect("the notice");
    assert_eq!(
        unclean.offers,
        Offers {
            restore: true,
            safe_mode: true,
            start_fresh: true
        }
    );
    assert_eq!(
        unclean.session,
        SessionInfo {
            available: true,
            windows: 1,
            problem: None
        }
    );
    assert_eq!(unclean.crash, None, "no panic was recorded");
    // An app without a safe-mode restart does not offer it.
    assert!(!next.report(false).unclean.unwrap().offers.safe_mode);
}

#[test]
fn restore_gives_back_the_window_state_and_the_notice_does_not_return() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    let run = begin(t.path(), &procs, 100);
    run.checkpoint("main", json!({"query": "a"})).unwrap();
    run.checkpoint("w-2", json!({"query": "b"})).unwrap();
    run.flush();
    std::mem::forget(run);
    procs.gone(100);
    write_crash(t.path(), 100);

    let next = begin(t.path(), procs.with_pid(200, 9000), 200);
    let plan = next.restore("main");
    assert_eq!(plan.windows, ["main", "w-2"]);
    assert_eq!(plan.ui, Some(json!({"query": "a"})));
    assert_eq!(next.restore("w-9").ui, None);
    assert_eq!(next.decision(), Some(Decision::Restore));
    assert_eq!(next.report(true).unclean, None);
    assert!(
        !CrashMarker::path_for(t.path(), "app").exists(),
        "decided: the evidence is cleared"
    );
}

#[test]
fn start_fresh_forgets_the_session_for_good() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    let run = begin(t.path(), &procs, 100);
    run.checkpoint("main", json!({"q": 1})).unwrap();
    run.flush();
    std::mem::forget(run);
    procs.gone(100);
    let next = begin(t.path(), procs.with_pid(200, 9000), 200);
    next.start_fresh();
    assert_eq!(next.decision(), Some(Decision::StartFresh));
    assert_eq!(next.report(true).unclean, None);
    assert_eq!(next.restore("main").windows, Vec::<String>::new());
    next.end();
    procs.gone(200);
    // Even if this run also dies, the discarded session does not come back.
    let third = begin(t.path(), procs.with_pid(300, 9100), 300);
    assert!(!third.session().previous_info().available);
}

#[test]
fn choosing_safe_mode_ends_the_notice() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    std::mem::forget(begin(t.path(), &procs, 100));
    procs.gone(100);
    write_crash(t.path(), 100);
    let next = begin(t.path(), procs.with_pid(200, 9000), 200);
    assert!(next.report(true).unclean.is_some());
    next.chose_safe_mode();
    assert_eq!(next.decision(), Some(Decision::SafeMode));
    assert_eq!(next.report(true).unclean, None);
    assert!(!CrashMarker::path_for(t.path(), "app").exists());
}

#[test]
fn a_reused_pid_is_a_crash_and_a_live_process_is_taken_over() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    std::mem::forget(begin(t.path(), &procs, 100));
    // The OS gave pid 100 to another program (different start time).
    procs.0.borrow_mut().insert(100, 7777);
    let reused = begin(t.path(), procs.with_pid(200, 9000), 200);
    assert_eq!(reused.previous_run(), PreviousRun::Crashed);
    std::mem::forget(reused);

    // The marker's process runs, within a second of the recorded start: not a crash.
    let t2 = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    let old = begin(t2.path(), &procs, 100);
    procs.0.borrow_mut().insert(100, 5001);
    let new = begin(t2.path(), procs.with_pid(200, 9000), 200);
    assert_eq!(new.previous_run(), PreviousRun::StillRunning);
    assert_eq!(new.report(true).unclean, None);
    // The old process finishes exiting: it must not erase the new one's marker.
    old.end();
    assert!(t2.path().join(MARKER_FILE).exists());
    new.end();
    assert!(!t2.path().join(MARKER_FILE).exists());
}

#[test]
fn a_marker_without_a_start_time_counts_as_running_while_its_pid_is_alive() {
    let m = Marker {
        pid: 5,
        started_at: None,
    };
    let alive_probe = |_: u32| Some(1u64);
    let dead_probe = |_: u32| None;
    assert_eq!(
        previous_run(MarkerState::Found(m), 9, &alive_probe),
        PreviousRun::StillRunning
    );
    assert_eq!(
        previous_run(MarkerState::Found(m), 9, &dead_probe),
        PreviousRun::Crashed
    );
    assert_eq!(
        previous_run(MarkerState::Found(m), 5, &alive_probe),
        PreviousRun::Crashed,
        "own pid: a dead process's"
    );
    assert_eq!(
        previous_run(MarkerState::Missing, 9, &alive_probe),
        PreviousRun::Clean
    );
    assert_eq!(
        previous_run(MarkerState::Unreadable, 9, &alive_probe),
        PreviousRun::Crashed
    );
}

#[test]
fn a_damaged_or_future_marker_counts_as_a_crash_and_is_replaced() {
    let procs = Procs::default().with(200, 9000);
    for content in [
        "{ torn",
        "",
        r#"{"version": 99, "pid": 5}"#,
        r#"{"version": 1}"#,
    ] {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join(MARKER_FILE), content).unwrap();
        let run = begin(t.path(), &procs, 200);
        assert_eq!(run.previous_run(), PreviousRun::Crashed, "{content:?}");
        // This run's own marker is in place, so its clean end is recognised.
        run.end();
        assert!(!t.path().join(MARKER_FILE).exists(), "{content:?}");
    }
}

#[test]
fn a_run_that_cannot_write_its_marker_still_starts() {
    let t = tempfile::tempdir().unwrap();
    // A folder where the marker file should be.
    std::fs::create_dir(t.path().join(MARKER_FILE)).unwrap();
    let procs = Procs::default().with(200, 9000);
    let run = begin(t.path(), &procs, 200);
    assert_eq!(run.previous_run(), PreviousRun::Crashed);
    run.checkpoint("main", json!({})).unwrap();
    run.end();
}

#[test]
fn a_crash_marker_makes_the_start_a_crash_and_names_where_but_not_what() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(200, 9000);
    // The previous run ended its way out (the marker is gone) after a panic on a worker thread.
    write_crash(t.path(), 100);
    let run = begin(t.path(), &procs, 200);
    assert_eq!(run.previous_run(), PreviousRun::Crashed);
    let crash = run.report(true).unclean.unwrap().crash.unwrap();
    assert_eq!(crash.thread, "worker-3");
    assert_eq!(crash.location, "x.rs:9");
    assert!(!crash.in_safe_mode);
    let wire = serde_json::to_string(&run.report(true)).unwrap();
    assert!(
        !wire.contains("secret") && !wire.contains("Users"),
        "the panic message stays out of the UI: {wire}"
    );
    // A clean end clears the evidence: the next start has nothing to report.
    run.end();
    procs.gone(200);
    let next = begin(t.path(), procs.with_pid(300, 9100), 300);
    assert_eq!(next.previous_run(), PreviousRun::Clean);
}

#[test]
fn a_damaged_crash_marker_still_counts() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(CrashMarker::path_for(t.path(), "app"), "{ torn").unwrap();
    let procs = Procs::default().with(200, 9000);
    let run = begin(t.path(), &procs, 200);
    let unclean = run
        .report(true)
        .unclean
        .expect("a crash marker file is a crash");
    assert_eq!(unclean.crash, None);
}

#[test]
fn two_instances_keep_their_own_state() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000).with(101, 5100);
    let main = begin(t.path(), &procs, 100);
    main.checkpoint("main", json!({"main": true})).unwrap();
    main.flush();
    let second = begin_as(t.path(), &procs, 101, Mode::Normal, InstanceScope::Separate);
    assert_eq!(
        second.previous_run(),
        PreviousRun::Clean,
        "the main instance's marker is not its business"
    );
    assert_eq!(second.dir(), t.path().join(INSTANCES_DIR).join("101"));
    second.checkpoint("main", json!({"second": true})).unwrap();
    second.flush();
    // Neither disturbed the other.
    assert_eq!(
        main.session().current().windows["main"],
        json!({"main": true})
    );
    second.end();
    assert!(!t.path().join(INSTANCES_DIR).join("101").exists());
    assert!(
        t.path().join(MARKER_FILE).exists(),
        "the main marker is still its own"
    );
    main.end();
}

#[test]
fn a_separate_instance_that_died_is_pruned_and_never_reported_as_the_main_ones_crash() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(101, 5100);
    std::mem::forget(begin_as(
        t.path(),
        &procs,
        101,
        Mode::Normal,
        InstanceScope::Separate,
    ));
    procs.gone(101);
    assert!(t.path().join(INSTANCES_DIR).join("101").exists());
    let main = begin(t.path(), procs.with_pid(200, 9000), 200);
    assert_eq!(main.previous_run(), PreviousRun::Clean);
    assert!(!t.path().join(INSTANCES_DIR).join("101").exists(), "pruned");
}

#[test]
fn safe_mode_runs_in_its_own_folder_with_nothing_optional_and_no_notice() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000).with(150, 5050);
    // The normal run crashed with a session.
    let run = begin(t.path(), &procs, 100);
    run.checkpoint("main", json!({"n": 1})).unwrap();
    run.flush();
    std::mem::forget(run);
    procs.gone(100);
    write_crash(t.path(), 100);

    // Even asked for the main scope, safe mode is separate.
    let safe = begin_as(t.path(), &procs, 150, Mode::Safe, InstanceScope::Main);
    assert_eq!(safe.mode(), Mode::Safe);
    assert_eq!(safe.dir(), t.path().join(INSTANCES_DIR).join("150"));
    let report = safe.report(true);
    assert!(report.safe_mode && report.unclean.is_none());
    assert!(!safe.mode().allows(crate::safe_mode::Feature::LastSession));
    assert!(
        !safe.session().previous_info().available,
        "safe mode does not reopen the session"
    );
    // The crashed run's evidence and session are untouched for the next normal start.
    assert!(CrashMarker::path_for(t.path(), "app").exists());
    safe.end();
    procs.gone(150);
    let normal = begin(t.path(), procs.with_pid(200, 9000), 200);
    let unclean = normal.report(true).unclean.unwrap();
    assert!(unclean.offers.restore && unclean.offers.safe_mode);
}

#[test]
fn a_crash_during_safe_mode_does_not_offer_safe_mode_again_but_keeps_the_session() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000).with(150, 5050);
    let run = begin(t.path(), &procs, 100);
    run.checkpoint("main", json!({"n": 7})).unwrap();
    run.flush();
    std::mem::forget(run);
    procs.gone(100);
    // The user started safe mode; it panicked too.
    std::mem::forget(begin_as(
        t.path(),
        &procs,
        150,
        Mode::Safe,
        InstanceScope::Separate,
    ));
    procs.gone(150);
    write_crash(t.path(), 150);

    let next = begin(t.path(), procs.with_pid(200, 9000), 200);
    let unclean = next.report(true).unclean.unwrap();
    assert!(unclean.crash.as_ref().unwrap().in_safe_mode);
    assert_eq!(
        unclean.offers,
        Offers {
            restore: true,
            safe_mode: false,
            start_fresh: true
        }
    );
    assert_eq!(next.restore("main").ui, Some(json!({"n": 7})));
    assert!(
        !t.path().join(INSTANCES_DIR).join("150").exists(),
        "its folder is pruned"
    );
}

#[test]
fn a_damaged_session_starts_clean_with_a_notice_and_no_restore() {
    let t = tempfile::tempdir().unwrap();
    let procs = Procs::default().with(100, 5000);
    std::mem::forget(begin(t.path(), &procs, 100));
    procs.gone(100);
    std::fs::create_dir_all(t.path().join(SESSION_DIR)).unwrap();
    std::fs::write(t.path().join(SESSION_DIR).join("current.json"), "{ torn").unwrap();
    let next = begin(t.path(), procs.with_pid(200, 9000), 200);
    let unclean = next.report(true).unwrap_unclean();
    assert!(!unclean.offers.restore && unclean.offers.start_fresh);
    assert!(
        unclean
            .session
            .problem
            .unwrap()
            .contains("could not be read")
    );
}

impl StartReport {
    fn unwrap_unclean(self) -> UncleanShutdown {
        self.unclean.expect("an unclean shutdown")
    }
}

#[test]
fn the_offer_is_a_pure_function_of_what_exists() {
    let some = SessionInfo {
        available: true,
        windows: 1,
        problem: None,
    };
    let none = SessionInfo::default();
    let safe_crash = CrashInfo {
        time: String::new(),
        thread: String::new(),
        location: String::new(),
        in_safe_mode: true,
    };
    assert_eq!(
        offers(&some, None, true),
        Offers {
            restore: true,
            safe_mode: true,
            start_fresh: true
        }
    );
    assert_eq!(
        offers(&none, None, true),
        Offers {
            restore: false,
            safe_mode: true,
            start_fresh: true
        }
    );
    assert!(!offers(&some, None, false).safe_mode);
    assert!(!offers(&some, Some(&safe_crash), true).safe_mode);
}

#[test]
fn pruning_removes_only_dead_pid_folders() {
    let t = tempfile::tempdir().unwrap();
    let inst = t.path().join(INSTANCES_DIR);
    for name in ["101", "102", "103", "notapid", "300"] {
        std::fs::create_dir_all(inst.join(name)).unwrap();
    }
    std::fs::write(inst.join("a-file"), "x").unwrap();
    // A file with a pid for a name is not an instance folder: left alone.
    std::fs::write(inst.join("104"), "x").unwrap();
    // 101 runs (no marker: decided by pid), 102 is dead, 103 has a marker whose pid was reused, 300 is this process's.
    let m = marker_file(&inst.join("103"));
    m.save(&MarkerBody {
        pid: 103,
        started_at: Some(1000),
        ..MarkerBody::default()
    })
    .unwrap();
    let procs = Procs::default().with(101, 1).with(103, 4000).with(300, 1);
    let removed = prune_instances(t.path(), 300, &procs.probe());
    assert_eq!(removed, 3);
    assert!(
        inst.join("101").exists() && inst.join("notapid").exists() && inst.join("a-file").exists()
    );
    assert!(inst.join("104").is_file());
    assert!(!inst.join("102").exists() && !inst.join("103").exists() && !inst.join("300").exists());
    assert_eq!(
        prune_instances(&t.path().join("nowhere"), 1, &procs.probe()),
        0
    );
}

#[test]
fn the_scope_follows_the_single_instance_lock() {
    assert_eq!(InstanceScope::of(true), InstanceScope::Main);
    assert_eq!(InstanceScope::of(false), InstanceScope::Separate);
}

#[cfg(feature = "os-probe")]
#[test]
fn the_os_probe_knows_this_process_and_not_a_pid_nobody_has() {
    let started = os_probe(std::process::id()).expect("this process is in the process table");
    assert!(started > 0);
    // Linux caps pids at 2^22 and macOS at 99999: this one is never in use.
    assert_eq!(os_probe(u32::MAX - 1), None);
}

#[cfg(feature = "os-probe")]
#[test]
fn begin_uses_the_real_pid_and_process_table_and_ends_clean() {
    let t = tempfile::tempdir().unwrap();
    let run = Lifecycle::begin(start(t.path()));
    assert!(run.restore("main").windows.is_empty());
    run.checkpoint("main", json!({"n": 1})).unwrap();
    run.flush();
    run.forget_window("main");
    run.flush();
    assert!(run.report(true).unclean.is_none());
}
