//! A child ends through one exit only. Debrief closing a child had two routes to `exit` that fire on the same event: the
//! lifeline thread (stdin closed) called `std::process::exit` while the child's `main` returned (the connection closed)
//! and ran `exit` too. Rust guards two `exit`s only on Linux (`unique_thread_exit`); on macOS two threads ran the `atexit`
//! handlers and the C++ static destructors together, and the store worker crashed in DuckDB's destructors (the 21
//! `debrief-store-worker` crash reports of 2026-11-05 to 2026-11-10, ADR 0076's exit-phase amendment).

use debrief_ipc::launch::launch;
use debrief_ipc::peer::{CallCtx, Handler};
use debrief_ipc::process::SpawnSpec;
use debrief_ipc::{launch::ChildSpec, RemoteError};
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
fn probe_spec(dir: &std::path::Path) -> ChildSpec {
    warm();
    let mut s = SpawnSpec::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child"), "the fake child");
    s.args = vec![format!("--exit-probe={}", dir.display())];
    s.endpoint_prefix = "debrief-test".into();
    s.connect_window = paddock_testkit::HANG_GUARD;
    ChildSpec::new(s)
}

fn warm() {
    paddock_testkit_start::warm_first_start(std::path::Path::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child")), &["--version"]);
}

fn size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// How a probe child ended: whether the probe handler ran at all and whether a second thread entered `exit`.
#[cfg(unix)]
struct Ended {
    first_entered: bool,
    second_entered: bool,
}

#[cfg(unix)]
/// Starts the fake child with the probe, stops it the way Debrief does (`Child::stop`: the connection closes and then
/// stdin), and reads what the probe recorded once the child is gone.
fn stop_a_probe_child() -> Ended {
    let dir = tempfile::tempdir().unwrap();
    let host: Arc<dyn Handler> = Arc::new(|_: &CallCtx, b: &[u8]| -> Result<Vec<u8>, RemoteError> { Ok(b.to_vec()) });
    let child = launch(&probe_spec(dir.path()), host).unwrap();
    assert_eq!(
        child.peer().call(b"echo up".to_vec(), Some(Duration::from_secs(10))).unwrap(),
        b"up"
    );
    child.stop();
    assert!(child.exited(), "the child is gone");
    Ended {
        first_entered: size(&dir.path().join("entered-1")) > 0,
        second_entered: size(&dir.path().join("entered-2")) > 0,
    }
}

#[cfg(unix)]
#[test]
fn two_exits_never_run_the_handlers_together() {
    let ended = stop_a_probe_child();
    // The probe ran (ER-33: a probe that did not run proves nothing): the child ended through `exit`, not killed.
    assert!(
        ended.first_entered,
        "the probe handler never ran: the child did not end through `exit`"
    );
    assert!(
        !ended.second_entered,
        "a second thread entered `exit` while the first was still in the handlers"
    );
}

/// A parent that stops a child waits for the child's own exit work before it kills it: the store worker joins its threads
/// for up to its stop budget, and a 150 ms grace killed it in the middle (`process::KILL_GRACE` covers the budget now).
#[test]
fn a_child_that_needs_time_to_finish_is_not_killed_in_the_middle() {
    warm();
    let dir = tempfile::tempdir().unwrap();
    let mut s = SpawnSpec::new(env!("CARGO_BIN_EXE_debrief-ipc-fake-child"), "the fake child");
    s.args = vec![format!("--slow-exit={}", dir.path().display())];
    s.endpoint_prefix = "debrief-test".into();
    s.connect_window = paddock_testkit::HANG_GUARD;
    let host: Arc<dyn Handler> = Arc::new(|_: &CallCtx, b: &[u8]| -> Result<Vec<u8>, RemoteError> { Ok(b.to_vec()) });
    let child = launch(&ChildSpec::new(s), host).unwrap();
    assert_eq!(
        child.peer().call(b"echo up".to_vec(), Some(Duration::from_secs(10))).unwrap(),
        b"up"
    );
    child.stop();
    assert!(child.exited(), "stop returns once the child is gone");
    assert!(
        size(&dir.path().join("finished")) > 0,
        "the child was killed before it finished its exit work"
    );
}
