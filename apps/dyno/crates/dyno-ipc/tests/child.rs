//! Real child processes (the `dyno-ipc-test-child` binary). Every child is ours and is killed when its handle drops.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use dyno_ipc::child::{launch, spawn};
use dyno_ipc::{ChildSpec, Host, IpcError, PeerOptions};
use std::time::{Duration, Instant};

const LONG: Duration = Duration::from_secs(10);

fn spec(mode: &str) -> ChildSpec {
    let mut s = ChildSpec::new(env!("CARGO_BIN_EXE_dyno-ipc-test-child"));
    s.args.push(mode.into());
    s
}

#[test]
fn a_child_connects_answers_and_exits_when_stopped() {
    let (mut child, peer) = launch::<String>(&spec("echo"), PeerOptions::new(), LONG).unwrap();
    assert_eq!(peer.call("hello".to_string(), LONG).unwrap(), "hello");
    // Closing the connection ends the child's serve loop.
    drop(peer);
    let status = child
        .wait_timeout(LONG)
        .unwrap()
        .expect("the child exits when the connection closes");
    assert!(status.success());
}

#[test]
fn closing_stdin_the_lifeline_ends_the_child() {
    let (mut child, peer) = launch::<String>(&spec("echo"), PeerOptions::new(), LONG).unwrap();
    child.close_stdin();
    assert!(child.wait_timeout(LONG).unwrap().is_some());
    assert!(peer.wait_closed(Some(LONG)));
}

#[test]
fn a_child_that_exits_at_once_fails_the_launch_quickly() {
    let t = Instant::now();
    let err = launch::<String>(&spec("exit"), PeerOptions::new(), LONG).err().unwrap();
    assert!(
        matches!(&err, IpcError::Io { message, .. } if message.contains("exited")),
        "{err:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_child_that_never_connects_times_out() {
    let err = launch::<String>(&spec("silent"), PeerOptions::new(), Duration::from_millis(500))
        .err()
        .unwrap();
    assert!(matches!(err, IpcError::Timeout(_)), "{err:?}");
}

#[test]
fn a_child_with_the_wrong_token_is_refused() {
    let mut host = Host::listen().unwrap();
    let mut child = spawn(&spec("wrong-token"), &host).unwrap();
    assert!(matches!(host.accept(LONG), Err(IpcError::Auth(_))));
    assert!(child.wait_timeout(LONG).unwrap().unwrap().success());
}

#[test]
fn stop_kills_a_child_that_ignores_its_lifeline() {
    let host = Host::listen().unwrap();
    let mut child = spawn(&spec("silent"), &host).unwrap();
    let t = Instant::now();
    let status = child.stop(Duration::from_millis(100)).unwrap();
    assert!(!status.success());
    assert!(t.elapsed() < Duration::from_secs(5));
    // Killing an exited child is fine.
    child.kill().unwrap();
}

#[cfg(windows)]
#[test]
fn the_job_caps_the_childs_memory() {
    let mut s = spec("echo");
    s.memory_limit = Some(64 * 1024 * 1024);
    let (mut child, peer) = launch::<String>(&s, PeerOptions::new(), LONG).unwrap();
    assert!(peer.call("alloc 8".to_string(), LONG).unwrap().starts_with("allocated"));
    // Past the cap the allocation fails and the child aborts: the call ends with the connection.
    assert_eq!(peer.call("alloc 256".to_string(), LONG), Err(IpcError::Closed));
    let status = child.wait_timeout(LONG).unwrap().unwrap();
    assert!(!status.success());
}
