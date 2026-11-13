//! The launcher and the child loop with real processes (Dyno's `tests/child.rs` and the transport half of Debrief's
//! `tests/launch.rs`): every child is started through `paddock-proc` and none is left behind.

use crate::common;
use common::{GUARD, T, child_program};
use paddock_ipc::{ChildSpec, IpcError, NoCalls, ProtocolVersion, RemoteError, codes, launch};
use std::sync::Arc;
use std::time::Duration;

fn spec(mode: &[&str]) -> ChildSpec {
    let mut s = ChildSpec::new(child_program(), "the test child");
    for a in mode {
        s = s.arg(*a);
    }
    s.start_timeout = GUARD;
    s.host_name = "Debrief".into();
    s
}

/// The identity of a running child, read while it lives: the wait for its end is on that identity reading `Gone`, never on a
/// failed read (rule 53).
fn identity(pid: u32) -> paddock_proc::ProcessId {
    paddock_proc::ProcessId::of(pid).expect("a running child has an identity")
}

fn gone(id: paddock_proc::ProcessId) -> bool {
    id.has_ended()
}

#[test]
fn a_child_connects_authenticates_answers_and_is_gone_after_drop() {
    let child = launch(&spec(&["echo"]), Arc::new(NoCalls)).unwrap();
    assert_eq!(child.peer().call(b"hi".to_vec(), T).unwrap(), b"hi");
    assert_eq!(
        child.peer().call(b"pid".to_vec(), T).unwrap(),
        child.pid().to_string().into_bytes()
    );
    assert_eq!(child.peer().peer_pid(), Some(child.pid()));
    assert!(
        matches!(child.peer().call(b"fail".to_vec(), T), Err(IpcError::Remote(e)) if e == RemoteError::new("bad", "typed failure"))
    );
    assert!(
        matches!(child.peer().call(b"panic".to_vec(), T), Err(IpcError::Remote(e)) if e.code == codes::PANIC)
    );
    // A big answer within the limit, and one over it: that call fails, the child keeps answering.
    assert_eq!(
        child.peer().call(b"big 100000".to_vec(), T).unwrap().len(),
        100_000
    );
    assert!(
        matches!(child.peer().call(b"big 20000000".to_vec(), T), Err(IpcError::Remote(e)) if e.code == codes::TOO_LARGE)
    );
    assert_eq!(child.peer().call(b"still".to_vec(), T).unwrap(), b"still");
    assert!(!child.exited());
    assert_eq!(child.in_job(), cfg!(windows));
    assert_eq!(child.memory_peak(), 0);
    assert_eq!(child.stopped_over_memory(), None);
    let id = identity(child.pid());
    drop(child);
    assert!(gone(id));
}

#[test]
fn closing_the_lifeline_ends_the_child() {
    let child = launch(&spec(&["echo"]), Arc::new(NoCalls)).unwrap();
    child.close_stdin();
    // The child exits on its own: its connection ends.
    assert!(child.peer().wait_closed(Some(GUARD)));
    child.stop(GUARD);
    assert!(child.exited());
}

#[test]
fn stop_ends_a_child_that_ignores_its_lifeline() {
    let child = launch(&spec(&["stubborn"]), Arc::new(NoCalls)).unwrap();
    assert_eq!(child.peer().call(b"x".to_vec(), T).unwrap(), b"x");
    // A short grace that the child sits out: it is ended after it.
    let id = identity(child.pid());
    child.stop(Duration::from_millis(30));
    assert!(child.exited());
    assert!(gone(id));
}

#[test]
fn children_that_exit_or_never_connect_fail_the_launch() {
    match launch(&spec(&["exit"]), Arc::new(NoCalls)) {
        Err(IpcError::Spawn(m)) => assert!(m.contains("exited"), "{m}"),
        Err(e) => panic!("{e:?}"),
        Ok(_) => panic!("launched"),
    }
    let mut silent = spec(&["silent"]);
    silent.start_timeout = Duration::ZERO;
    assert!(matches!(
        launch(&silent, Arc::new(NoCalls)),
        Err(IpcError::Timeout)
    ));
    // A program that does not exist: refused at the spawn, or (where the platform's spawn reports it only through the child's
    // exit, as under emulation) a child that exited before it took its bootstrap. Either way the launch fails with a reason.
    let missing = launch(
        &ChildSpec::new("/no/such/paddock-program", "nothing"),
        Arc::new(NoCalls),
    );
    assert!(
        matches!(&missing, Err(IpcError::Spawn(m)) if m.contains("could not be started") || m.contains("exited")),
        "{:?}",
        missing.err()
    );
}

#[test]
fn a_wrong_token_is_refused_and_another_protocol_is_told_what_to_update() {
    // The wrong-token child never gets a connection: the launch waits for the right one and gives up when it exits.
    assert!(matches!(
        launch(&spec(&["wrong-token"]), Arc::new(NoCalls)),
        Err(IpcError::Auth(_) | IpcError::Spawn(_))
    ));
    let mut newer = spec(&["protocol", "2.4"]);
    newer.protocol = ProtocolVersion::new(2, 3);
    match launch(&newer, Arc::new(NoCalls)) {
        Err(IpcError::Version(m)) => assert!(
            m.contains("needs a newer Debrief") && m.contains("2.4"),
            "{m}"
        ),
        Err(e) => panic!("{e:?}"),
        Ok(_) => panic!("launched"),
    }
    let mut same = spec(&["protocol", "2.1"]);
    same.protocol = ProtocolVersion::new(2, 3);
    let child = launch(&same, Arc::new(NoCalls)).unwrap();
    assert_eq!(child.peer().call(b"v".to_vec(), T).unwrap(), b"v");
}

#[test]
fn the_child_gets_its_environment_role_and_memory_watch() {
    let mut s = spec(&["echo"]).env("PADDOCK_IPC_TEST", "1");
    s.role = "elevated".into();
    s.extra = Some(serde_json::json!({"sandbox": "locked"}).into());
    s.clean_env = false;
    s.lower_priority = true;
    s.memory_limit = Some(4 << 30);
    s.memory_hard = true;
    s.current_dir = Some(std::env::temp_dir());
    s.inherit_stderr = true;
    let child = launch(&s, Arc::new(NoCalls)).unwrap();
    assert_eq!(child.peer().call(b"ok".to_vec(), T).unwrap(), b"ok");
    child.kill();
    assert!(child.exited());
}

#[test]
fn the_bootstrap_line_roundtrips_and_bad_lines_are_refused() {
    use paddock_ipc::Bootstrap;
    use paddock_ipc::child::{connect, read_bootstrap};
    let b = Bootstrap {
        endpoint: "e".into(),
        token: "t".into(),
        max_frame: 2048,
        role: String::new(),
        extra: None,
    };
    let text = format!("{}\nrest", serde_json::to_string(&b).unwrap());
    assert_eq!(read_bootstrap(text.as_bytes()).unwrap(), b);
    assert_eq!(
        read_bootstrap(&b"nope\n"[..]).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
    let long = format!("{{\"endpoint\":\"{}\"}}\n", "a".repeat(70_000));
    assert!(read_bootstrap(long.as_bytes()).is_err());
    assert!(matches!(
        connect(&b, ProtocolVersion::UNVERSIONED),
        Err(IpcError::Auth(_))
    ));
    let no_host = Bootstrap {
        token: "a".repeat(64),
        endpoint: "/no/such/socket".into(),
        ..b
    };
    assert!(connect(&no_host, ProtocolVersion::UNVERSIONED).is_err());
}

#[test]
fn a_memory_limit_without_the_hard_commit_limit_still_runs_and_is_watched() {
    let mut s = spec(&["echo"]);
    s.memory_limit = Some(4 << 30);
    s.memory_hard = false;
    let child = launch(&s, Arc::new(NoCalls)).unwrap();
    assert_eq!(child.peer().call(b"ok".to_vec(), T).unwrap(), b"ok");
    assert_eq!(child.stopped_over_memory(), None);
}

#[test]
fn exit_status_waits_for_the_exit_and_is_kept() {
    let child = launch(&spec(&["echo"]), Arc::new(NoCalls)).unwrap();
    // Still running: no status after a short wait.
    assert_eq!(child.exit_status(Duration::ZERO), None);
    child.close_stdin();
    assert!(child.peer().wait_closed(Some(GUARD)));
    let status = child.exit_status(GUARD).expect("it exited");
    assert!(status.success(), "{status}");
    // A second ask returns the same status (it is read once from the OS).
    assert_eq!(child.exit_status(Duration::ZERO), Some(status));
}

#[test]
fn exit_status_waits_on_the_injected_clock_not_on_the_wall() {
    use paddock_jobs::time::FakeClock;
    let clock = FakeClock::default();
    let mut s = spec(&["echo"]);
    s.peer.clock = Arc::new(clock.clone());
    let child = Arc::new(launch(&s, Arc::new(NoCalls)).unwrap());
    let waiting = child.clone();
    let asked = std::thread::spawn(move || waiting.exit_status(Duration::from_secs(5)));
    // The child runs, so the ask is waiting on the fake clock; moving the clock past its deadline ends it with no status.
    assert!(clock.wait_for_pending(1, GUARD));
    // A kill is not held up by the waiting ask: it holds the process lock only for one look at a time.
    clock.advance(Duration::from_secs(5));
    assert_eq!(asked.join().unwrap(), None);
    child.kill();
}

/// A child ends through one exit only. Its lifeline thread (stdin closed) called `std::process::exit` while the child's `main`
/// returned (the connection closed) and ran `exit` too; Rust guards two `exit`s only on Linux, so on macOS two threads ran
/// the `atexit` handlers and the C++ static destructors together (the store worker's crash, ADR 0076's exit-phase
/// amendment). The probe child records whether a second thread entered `exit` while the first was in the handlers.
#[cfg(unix)]
#[test]
fn two_exits_never_run_the_handlers_together() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().to_str().unwrap();
    let child = launch(&spec(&["exit-probe", marker]), Arc::new(NoCalls)).unwrap();
    assert_eq!(child.peer().call(b"up".to_vec(), T).unwrap(), b"up");
    // The way an owner ends a child: the connection and the lifeline close together, then the grace.
    child.stop(GUARD);
    assert!(child.exited());
    let size = |name: &str| std::fs::metadata(dir.path().join(name)).map_or(0, |m| m.len());
    // The probe ran (a probe that did not run proves nothing): the child ended through `exit`, not killed.
    assert!(
        size("entered-1") > 0,
        "the probe handler never ran: the child did not end through `exit`"
    );
    assert_eq!(
        size("entered-2"),
        0,
        "a second thread entered `exit` while the first was still in the handlers"
    );
}

/// The plain lifeline (a child with no peer to close) ends the process itself when stdin closes.
#[test]
fn the_plain_lifeline_ends_a_child_without_a_peer_loop() {
    let child = launch(&spec(&["plain-lifeline"]), Arc::new(NoCalls)).unwrap();
    child.close_stdin();
    let status = child
        .exit_status(GUARD)
        .expect("the child exited on its own");
    assert!(status.success(), "{status:?}");
}
