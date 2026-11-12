//! Capturing a real child's standard error and exit into its child log: crash vs normal vs stopped by Debrief.

use debrief_ipc::child_log::{self, ChildKind, ChildLogSpec, ChildRunState};
use debrief_ipc::launch::{launch, ChildSpec};
use debrief_ipc::peer::NoCalls;
use debrief_ipc::process::{ChildProcess, SpawnSpec};
use std::sync::Arc;

/// The fake child's first start (a fresh binary the OS has not run yet) is paid here, before a test starts it with a bound it
/// cannot remove (a child that exits at once must be seen exiting within the OS's publication window).
fn warm() {
    paddock_testkit_start::warm_first_start(std::path::Path::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child")), &["--version"]);
}

fn spec(args: &[&str], log: &str) -> SpawnSpec {
    warm();
    let mut s = SpawnSpec::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child"), "the fake child");
    s.args = args.iter().map(|a| a.to_string()).collect();
    s.endpoint_prefix = "debrief-test".into();
    s.connect_window = paddock_testkit::HANG_GUARD;
    s.log = Some(ChildLogSpec::new(log, ChildKind::Plugin).with_version("9.9.9"));
    s
}

#[test]
fn a_crash_is_recorded_with_its_output_and_exit_code() {
    let p = ChildProcess::spawn(&spec(&["--say=boom token=s3cr3t", "--exit-now"], "capture-crash")).unwrap();
    let log = p.log().unwrap().clone();
    paddock_testkit::wait_until("the exit line", || log.status().state != ChildRunState::Running);
    let s = log.status();
    assert_eq!(s.state, ChildRunState::Crashed, "{s:?}");
    assert_eq!(s.last_exit.as_deref(), Some("exit code 3"));
    assert_eq!((s.pid, s.version.as_deref()), (Some(p.pid()), Some("9.9.9")));
    let text = log.tail_text();
    assert!(text.contains("boom token=***") && !text.contains("s3cr3t"), "{text}");
    // Stopping it afterwards does not turn the crash into a stop.
    p.kill();
    let lines = log
        .tail_text()
        .lines()
        .filter(|l| l.contains("\"Event\":\"crash\"") || l.contains("\"Event\":\"stop\""))
        .count();
    assert_eq!(lines, 1);
}

#[test]
fn a_child_stopped_by_debrief_and_restarted_counts_runs() {
    let s = ChildSpec::new(spec(&["--say=ready"], "capture-stop"));
    let first = launch(&s, Arc::new(NoCalls)).unwrap();
    first.stop();
    let second = launch(&s, Arc::new(NoCalls)).unwrap();
    drop(second);
    let log = child_log::find("capture-stop").unwrap();
    let st = log.status();
    assert_eq!((st.starts, st.restarts, st.state), (2, 1, ChildRunState::Stopped), "{st:?}");
    assert_eq!(st.last_exit.as_deref(), Some("stopped by Debrief"));
    let text = log.tail_text();
    assert_eq!(text.matches("\"Event\":\"stop\"").count(), 2, "{text}");
    assert!(text.contains("\"@m\":\"ready\""), "{text}");
}

/// A well-behaved child exits as soon as its connection closes, so its exit can be seen before Debrief's kill of it runs
/// (CI 7981, Linux: run 1 logged `"Event":"exit"` with "exit code 0" right after its ready line). That exit is still a stop
/// by Debrief. The order is forced here: the stop waits until the child has exited before it goes on to the kill.
#[test]
fn a_child_that_exits_as_debrief_closes_its_connection_is_stopped_by_debrief() {
    let s = ChildSpec::new(spec(&["--say=ready"], "capture-stop-order"));
    let child = launch(&s, Arc::new(NoCalls)).unwrap();
    child.stop_after(|| paddock_testkit::wait_until("the child to exit as its connection closed", || child.exited()));
    let log = child_log::find("capture-stop-order").unwrap();
    let st = log.status();
    assert_eq!(
        (st.state, st.last_exit.as_deref()),
        (ChildRunState::Stopped, Some("stopped by Debrief")),
        "{st:?}"
    );
    let text = log.tail_text();
    assert_eq!(text.matches("\"Event\":\"stop\"").count(), 1, "{text}");
    assert_eq!(text.matches("\"Event\":\"exit\"").count(), 0, "{text}");
}

/// A child that crashed before Debrief stops it is recorded as crashed, even when its stderr reader has not noted the
/// exit yet: the stop records the child's own exit before it marks the stop (review of db7d92a40: the crash test above let
/// the reader note it first, so "a crash stays a crash" was untested).
#[test]
fn a_child_that_crashed_before_debrief_stops_it_stays_a_crash() {
    debrief_ipc::process::hold_exit_reader("capture-crash-unnoted");
    let p = ChildProcess::spawn(&spec(&["--exit-now"], "capture-crash-unnoted")).unwrap();
    paddock_testkit::wait_until("the child to crash", || p.try_exit().is_some());
    let log = p.log().unwrap().clone();
    assert_eq!(
        log.status().state,
        ChildRunState::Running,
        "nobody has noted the exit yet (the reader is held)"
    );
    p.kill();
    let st = log.status();
    assert_eq!(
        (st.state, st.last_exit.as_deref()),
        (ChildRunState::Crashed, Some("exit code 3")),
        "{st:?}"
    );
    assert_eq!(log.tail_text().matches("\"Event\":\"stop\"").count(), 0);
}

/// A child whose connection broke is stopped after its own end is known: the OS is given a moment to publish the exit of a
/// child that just crashed, so it is recorded as a crash and not as "stopped by Debrief" (the supervisor's report path).
#[test]
fn a_child_whose_connection_broke_is_recorded_by_its_own_exit() {
    debrief_ipc::process::hold_exit_reader("capture-broken");
    let p = ChildProcess::spawn(&spec(&["--exit-now"], "capture-broken")).unwrap();
    // At once: the crash has usually not been published by the OS yet.
    p.kill_after_failure();
    let st = p.log().unwrap().status();
    assert_eq!(
        (st.state, st.last_exit.as_deref()),
        (ChildRunState::Crashed, Some("exit code 3")),
        "{st:?}"
    );
}

#[test]
fn a_child_that_cannot_start_leaves_a_note() {
    let mut s = spec(&[], "capture-missing");
    s.exe = "/no/such/debrief-child".into();
    assert!(ChildProcess::spawn(&s).is_err());
    assert!(child_log::find("capture-missing").unwrap().tail_text().contains("Could not start"));
}

#[test]
fn without_a_log_nothing_is_captured() {
    let mut s = spec(&["--exit-now"], "capture-none");
    s.log = None;
    let p = ChildProcess::spawn(&s).unwrap();
    assert!(p.log().is_none());
    p.kill();
    assert!(child_log::find("capture-none").is_none());
}
