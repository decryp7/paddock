//! TERM ends the real `dyno-agent` (macOS sends it at logout and shutdown, launchd at unload). The first VM run found an agent
//! that ignored it and needed KILL. These tests start the real binary on a temporary data folder (`DYNO_DATA_DIR`), send TERM and
//! wait for it to exit; the wait is the family hang guard of 40 s, which only keeps a broken agent from hanging the test.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

const HANG_GUARD: Duration = Duration::from_secs(40);

fn start(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_dyno-agent"))
        .arg("run")
        .env("DYNO_DATA_DIR", dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the agent program starts")
}

fn term(child: &Child) {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("kill runs");
    assert!(status.success());
}

/// Waits for the child to exit (on a thread, so the wait itself is bounded by the guard); ends it and fails when it does not.
fn exits(mut child: Child) -> std::process::ExitStatus {
    let id = child.id();
    let (tx, rx) = channel();
    let waiter = std::thread::spawn(move || {
        let status = child.wait().expect("wait");
        let _ = tx.send(status);
    });
    match rx.recv_timeout(HANG_GUARD) {
        Ok(status) => {
            waiter.join().unwrap();
            status
        }
        Err(_) => {
            let _ = Command::new("kill").arg("-KILL").arg(id.to_string()).status();
            panic!("the agent ignored TERM (it had to be killed)");
        }
    }
}

#[test]
fn term_stops_a_running_agent_cleanly_and_removes_its_discovery_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = dyno_agent::paths::Dirs::at(tmp.path().join("data"));
    let child = start(dirs.root());
    // Ready: the agent has written its discovery file (the handshake the app uses). The wait is on the file.
    let end = std::time::Instant::now() + HANG_GUARD;
    while dyno_agent::discovery::Discovery::read(&dirs.discovery()).is_none() {
        assert!(std::time::Instant::now() < end, "the agent never became ready");
        // pace: how often the file is looked at; the guard above is the harness's bound.
        std::thread::sleep(Duration::from_millis(20));
    }
    term(&child);
    let status = exits(child);
    assert_eq!(status.code(), Some(0), "a clean stop is exit code 0, got {status:?}");
    assert!(!dirs.discovery().exists(), "the discovery file was left behind");
    assert!(!dyno_agent::instance::is_held(&dirs), "the agent lock was not released");
}

#[test]
fn term_sent_while_the_agent_is_starting_still_ends_it() {
    // TERM right after the spawn lands before the handlers, or in the middle of the start, or after it: whichever, the agent
    // ends (the start does not look at the flag, a thread of its own does).
    for _ in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = dyno_agent::paths::Dirs::at(tmp.path().join("data"));
        let child = start(dirs.root());
        term(&child);
        let _ = exits(child);
        assert!(!dyno_agent::instance::is_held(&dirs), "the agent lock was left held");
    }
}
