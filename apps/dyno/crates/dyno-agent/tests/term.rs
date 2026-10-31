//! TERM ends the real `dyno-agent` (macOS sends it at logout and shutdown, launchd at unload). The first VM run found an agent
//! that ignored it and needed KILL. These tests start the real binary on a temporary data folder (`DYNO_DATA_DIR`), send TERM and
//! wait for it to exit; the wait is the family hang guard of 40 s, which only keeps a broken agent from hanging the test.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_proc::ProcessId;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::time::Duration;

const HANG_GUARD: Duration = Duration::from_secs(40);

#[expect(
    clippy::disallowed_methods,
    reason = "the test starts the real agent binary, with a temporary data folder and no console"
)]
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

/// Asks the child to stop with SIGTERM through paddock-proc, by the identity of the process this test holds (its own unreaped
/// `Child`, so its pid cannot have been handed on).
fn term(child: &Child) {
    let id = ProcessId::of(child.id()).expect("the child is running");
    assert!(paddock_proc::ask(id), "the TERM was not delivered");
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
            if let Some(id) = ProcessId::of(id) {
                let _ = paddock_proc::end(id);
            }
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

/// The agent's own log (login-start diagnosis, 2026-10-31): one dated paddock-log file per process, with a line per step of the start
/// and a millisecond count, so a slow connect after login can be told from the agent's side (store open, endpoint bound, running)
/// and from the app's. Names and numbers only. The agent started by launchd has no stderr that anyone reads: this file is it.
#[test]
fn the_agents_start_and_stop_are_in_its_own_dated_log_with_millisecond_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = dyno_agent::paths::Dirs::at(tmp.path().join("data"));
    let child = start(dirs.root());
    let end = std::time::Instant::now() + HANG_GUARD;
    while dyno_agent::discovery::Discovery::read(&dirs.discovery()).is_none() {
        assert!(std::time::Instant::now() < end, "the agent never became ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    term(&child);
    assert_eq!(exits(child).code(), Some(0));
    let file = std::fs::read_dir(dirs.logs())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("dyno-agent.") && n.ends_with(".log"))
        })
        .expect("a dyno-agent.<day>.log in the log folder");
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let at = |what: &str| {
        lines
            .iter()
            .position(|l| l["@m"].as_str() == Some(what))
            .unwrap_or_else(|| panic!("no {what:?} in {lines:#?}"))
    };
    let order = [
        "Agent starting",
        "Agent lock taken",
        "Agent store open",
        "Agent endpoint bound",
        "Agent running",
        "Agent stopping",
        "Agent stopped",
    ];
    let places: Vec<usize> = order.iter().map(|w| at(w)).collect();
    assert!(places.windows(2).all(|w| w[0] < w[1]), "in order: {places:?}");
    for step in [
        "Agent lock taken",
        "Agent store open",
        "Agent endpoint bound",
        "Agent running",
        "Agent stopped",
    ] {
        assert!(
            lines[at(step)]["ElapsedMs"].is_u64(),
            "{step} carries ElapsedMs: {}",
            lines[at(step)]
        );
    }
    let starting = &lines[at("Agent starting")];
    assert!(starting["ParentPid"].is_u64() && starting["Arch"].is_string(), "{starting}");
    assert_eq!(lines[at("Agent endpoint bound")]["DiscoveryWritten"], serde_json::Value::Bool(true));
    let text = serde_json::to_string(&lines).unwrap();
    assert!(
        !text.contains(tmp.path().to_str().unwrap()),
        "no path of the test folder in the log: {text}"
    );
}

/// A second agent on a held data folder is refused and says so in its own dated log: the exit code 3 the app reads, why, who holds the
/// folder (its pid, from its discovery file) and what started the refused one. This is the line a bundle needs when "another Dyno agent
/// holds the data folder" is shown.
#[test]
fn a_refused_second_agent_logs_why_who_holds_the_folder_and_exit_code_3() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = dyno_agent::paths::Dirs::at(tmp.path().join("data"));
    let first = start(dirs.root());
    let first_pid = first.id();
    let end = std::time::Instant::now() + HANG_GUARD;
    while dyno_agent::discovery::Discovery::read(&dirs.discovery()).is_none() {
        assert!(std::time::Instant::now() < end, "the first agent never became ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    let second = exits(start(dirs.root()));
    assert_eq!(second.code(), Some(dyno_agent::cli::EXIT_ALREADY_RUNNING), "{second:?}");
    term(&first);
    assert_eq!(exits(first).code(), Some(0));
    let file = std::fs::read_dir(dirs.logs())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("dyno-agent.") && n.ends_with(".log"))
        })
        .expect("a dyno-agent.<day>.log");
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(file)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let refused = lines
        .iter()
        .find(|l| l["@m"].as_str() == Some("Agent refused: another agent holds the data folder"))
        .unwrap_or_else(|| panic!("no refusal in {lines:#?}"));
    assert_eq!(refused["ExitCode"], 3);
    assert_eq!(refused["HolderPid"], u64::from(first_pid));
    assert!(refused["Reason"].is_string() && refused["ParentName"].is_string(), "{refused}");
}
