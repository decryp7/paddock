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
/// How long a start may take before the agent counts as hung. Readiness waits on the discovery file and on the child being alive, so
/// a healthy agent returns at once; the guard only ends a hung one. It is long because a loaded shared CI host can starve a fresh
/// process for tens of seconds (CI run 7918: 40 s was not enough with three runners busy); the nextest waiver covers it.
const READY_GUARD: Duration = Duration::from_secs(120);

/// An agent this test started. It is ended and reaped when dropped, so a failed assertion never leaves one running; a test that
/// stops it itself takes the child out with [`Agent::into_child`].
struct Agent(Option<Child>);

impl Agent {
    fn id(&self) -> u32 {
        self.0.as_ref().expect("the agent is held").id()
    }

    fn into_child(mut self) -> Child {
        self.0.take().expect("the agent is held")
    }

    /// `None` while it runs, else how it ended.
    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.0.as_mut().and_then(|c| c.try_wait().ok().flatten())
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            if let Some(id) = ProcessId::of(child.id()) {
                let _ = paddock_proc::end(id);
            }
            let _ = child.wait();
        }
    }
}

/// Waits until `ready` holds. It stops at once, saying how, when the agent has exited (it will never be ready), and after `guard`
/// when it is still running (hung or starved); both messages say which, so a failure explains itself (ER-27).
fn wait_ready(agent: &mut Agent, mut ready: impl FnMut() -> bool, guard: Duration) -> Result<(), String> {
    let began = std::time::Instant::now();
    loop {
        if ready() {
            return Ok(());
        }
        if let Some(status) = agent.exited() {
            return Err(format!(
                "the agent exited before it was ready ({status:?}, after {:?})",
                began.elapsed()
            ));
        }
        if began.elapsed() >= guard {
            return Err(format!("the agent was still running but not ready after {guard:?}"));
        }
        // pace: how often the file and the child are looked at; the guard above is the harness's bound.
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test starts the real agent binary, with a temporary data folder and no console"
)]
fn start(dir: &std::path::Path) -> Agent {
    // Its standard error goes to a file beside the data folder, one per agent, for `evidence` (the line a forced end writes).
    let stderr = std::fs::File::create(stderr_file(dir, &format!("{}", next_agent()))).expect("the stderr file");
    Agent(Some(
        Command::new(env!("CARGO_BIN_EXE_dyno-agent"))
            .arg("run")
            .env("DYNO_DATA_DIR", dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("the agent program starts"),
    ))
}

fn next_agent() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

fn stderr_file(dir: &std::path::Path, n: &str) -> std::path::PathBuf {
    dir.parent()
        .expect("the data folder has a parent")
        .join(format!("agent-{n}.stderr"))
}

/// What the agents of `dirs` wrote (their standard error and their dated log), for a failing assertion's message (ER-27): a
/// forced end says there whether the start or the stop ran out of time and what the stop was waiting on.
fn evidence(dirs: &dyno_agent::paths::Dirs) -> String {
    let mut out = String::new();
    let parent = dirs.root().parent().expect("the data folder has a parent");
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(parent)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.extend(
        std::fs::read_dir(dirs.logs())
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    for f in files.iter().filter(|p| p.extension().is_some_and(|x| x == "stderr" || x == "log")) {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        out.push_str(&format!("\n---- {name} ----\n{text}"));
    }
    out
}

/// The agent is ready when its discovery file is there; a failure says how, with what the agent wrote (ER-27).
fn ready(agent: &mut Agent, dirs: &dyno_agent::paths::Dirs) {
    wait_ready(
        agent,
        || dyno_agent::discovery::Discovery::read(&dirs.discovery()).is_some(),
        READY_GUARD,
    )
    .unwrap_or_else(|why| panic!("{why}{}", evidence(dirs)));
}

/// Asks the child to stop with SIGTERM through paddock-proc, by the identity of the process this test holds (its own unreaped
/// `Child`, so its pid cannot have been handed on).
fn term(child: &Agent) {
    let id = ProcessId::of(child.id()).expect("the child is running");
    assert!(paddock_proc::ask(id), "the TERM was not delivered");
}

/// Waits for the child to exit (on a thread, so the wait itself is bounded by the guard); ends it and fails when it does not.
fn exits(agent: Agent) -> std::process::ExitStatus {
    let mut child = agent.into_child();
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
    let mut child = start(dirs.root());
    // Ready: the agent has written its discovery file (the handshake the app uses). The wait is on the file and on the child.
    ready(&mut child, &dirs);
    term(&child);
    let status = exits(child);
    assert_eq!(
        status.code(),
        Some(0),
        "a clean stop is exit code 0, got {status:?}{}",
        evidence(&dirs)
    );
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
    let mut child = start(dirs.root());
    ready(&mut child, &dirs);
    term(&child);
    let status = exits(child);
    assert_eq!(status.code(), Some(0), "{status:?}{}", evidence(&dirs));
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
    let mut first = start(dirs.root());
    let first_pid = first.id();
    ready(&mut first, &dirs);
    let second = exits(start(dirs.root()));
    assert_eq!(
        second.code(),
        Some(dyno_agent::cli::EXIT_ALREADY_RUNNING),
        "{second:?}{}",
        evidence(&dirs)
    );
    term(&first);
    let status = exits(first);
    assert_eq!(status.code(), Some(0), "{status:?}{}", evidence(&dirs));
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

/// ER-14: an agent on a data folder of its own (`DYNO_DATA_DIR`, as every test and development agent runs) never talks to this
/// computer's helper. The installed helper reads with `powermetrics` and sets and cancels the OS's scheduled wakes: a test agent
/// that reached it read the real machine, made its own stop wait out those calls (CI job 33005) and, on start, cancelled every
/// Dyno wake its fresh folder did not own.
#[test]
fn an_agent_on_a_data_folder_of_its_own_never_asks_this_computers_helper() {
    use dyno_core::protocol::{Reply, Request};
    let tmp = tempfile::tempdir().unwrap();
    let dirs = dyno_agent::paths::Dirs::at(tmp.path().join("data"));
    let mut child = start(dirs.root());
    ready(&mut child, &dirs);
    let (peer, _) = dyno_agent::cli::connect(&dirs).unwrap();
    let status = match dyno_agent::cli::ask_within(&peer, Request::HelperStatus, HANG_GUARD) {
        Ok(Reply::HelperStatus(s)) => *s,
        other => panic!("{other:?}{}", evidence(&dirs)),
    };
    peer.close();
    term(&child);
    let stopped = exits(child);
    assert_eq!(
        status.state,
        dyno_core::helper_wire::HelperState::NotIncluded,
        "{status:?}{}",
        evidence(&dirs)
    );
    assert!(status.message.contains("data folder of its own"), "{status:?}");
    let log = evidence(&dirs);
    // The status above came from the agent's own "no helper" answer; nothing reached a helper process either.
    for asked in ["Helper call", "Helper build checked"] {
        assert!(!log.contains(asked), "the agent asked a helper ({asked}):{log}");
    }
    assert_eq!(stopped.code(), Some(0), "{stopped:?}{log}");
}

/// A stand-in for an agent that never writes its discovery file: a shell that exits at once, or sleeps.
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts a plain shell as a stand-in child, to test the harness's wait"
)]
fn fake_agent(script: &str) -> Agent {
    Agent(Some(
        Command::new("sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the shell starts"),
    ))
}

/// The harness's readiness wait (CI run 7918: four tests failed after 40 s on a starved host with nothing to say why). An agent
/// that has already exited is reported at once and not waited for to the end of the guard, and the report says it exited.
#[test]
fn the_wait_for_ready_reports_an_agent_that_exited_at_once() {
    let mut agent = fake_agent("exit 7");
    let began = std::time::Instant::now();
    let why = wait_ready(&mut agent, || false, Duration::from_secs(30)).unwrap_err();
    assert!(why.contains("exited before it was ready"), "{why}");
    assert!(why.contains('7'), "the exit code is in the message: {why}");
    assert!(
        began.elapsed() < Duration::from_secs(20),
        "an exited agent is not waited for to the guard: {:?}",
        began.elapsed()
    );
}

/// A hung or starved agent that never writes its file is reported after the guard as still running, and ending the wait's owner ends
/// the agent: no process is left behind (ER-14).
#[test]
fn the_wait_for_ready_names_a_hung_agent_and_dropping_it_ends_the_process() {
    let mut agent = fake_agent("sleep 300");
    let pid = agent.id();
    let why = wait_ready(&mut agent, || false, Duration::from_millis(300)).unwrap_err();
    assert!(why.contains("still running but not ready"), "{why}");
    assert!(ProcessId::of(pid).is_some(), "the stand-in is still running before the drop");
    drop(agent);
    assert!(ProcessId::of(pid).is_none(), "the dropped agent was left running (pid {pid})");
}

/// A discovery file that appears makes the wait return without waiting for the guard.
#[test]
fn the_wait_for_ready_returns_when_the_file_appears() {
    let mut agent = fake_agent("sleep 300");
    let mut looks = 0;
    wait_ready(
        &mut agent,
        || {
            looks += 1;
            looks > 3
        },
        Duration::from_secs(30),
    )
    .unwrap();
}
