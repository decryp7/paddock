//! The server's life against a stand-in sccache: its start is a process that listens on the port and answers nothing (a server),
//! its `--show-stats` answers at once, or never while the file `hung` exists (a hung server: it takes the connection and gives no
//! statistics, as measured with sccache 0.19.0). Every server a test starts is ended by its record before the test returns.

use super::*;
use paddock_testkit::{Actions, Case, Installed, StandIn};

/// A loopback port nothing listens on now, for a stand-in server to bind: the server under test binds its port itself, so the port
/// cannot stay held. A concurrent taker makes the server's bind fail with its own error (the test fails loud), never a wrong answer.
pub(crate) fn port_for_a_server() -> u16 {
    // conformance: freed-port — the stand-in server must bind this port itself (sccache takes its port from SCCACHE_SERVER_PORT)
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    // conformance: freed-port — see above: freed so the server under test can take it
    drop(l);
    port
}

/// The stand-in server's start case: a process that listens on `SCCACHE_SERVER_PORT` (what `start` spawns sets
/// `SCCACHE_START_SERVER`).
pub(crate) fn serves(fake: StandIn) -> StandIn {
    fake.when(
        &Case::new().env_is_set("SCCACHE_START_SERVER"),
        Actions::new().listens("{env:SCCACHE_SERVER_PORT}"),
    )
}

struct Rig {
    dir: tempfile::TempDir,
    bin: Installed,
    port: u16,
    env: HashMap<String, String>,
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let hung = dir.path().join("hung");
        let bin = serves(
            StandIn::new(dir.path(), "sccache")
                .records_args()
                .when(
                    &Case::new().args_start_with(&["--show-stats"]).file_missing(&hung),
                    Actions::new().stdout("Compile requests 0"),
                )
                .when(
                    &Case::new().args_start_with(&["--show-stats"]).file_exists(&hung),
                    Actions::new().sleeps(Duration::from_secs(30)),
                ),
        )
        .install()
        .unwrap();
        let port = port_for_a_server();
        let env: HashMap<String, String> = [
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("SCCACHE_SERVER_PORT".to_string(), port.to_string()),
        ]
        .into();
        Rig { dir, bin, port, env }
    }

    fn call(&self) -> ToolCall {
        ToolCall::new(self.bin.program().to_string_lossy()).pass(["SCCACHE_SERVER_PORT"])
    }

    fn start(&self) -> Result<ProcessId, String> {
        start(
            &self.call(),
            &self.env,
            self.port,
            self.dir.path(),
            &self.dir.path().join("start.log"),
            START_LIMIT,
        )
    }

    fn probe(&self) -> Result<String, String> {
        probe(&self.call().args(["--show-stats"]), &self.env, Duration::from_millis(500))
    }

    fn hang(&self, on: bool) {
        let f = self.dir.path().join("hung");
        if on {
            std::fs::write(f, "").unwrap();
        } else {
            let _ = std::fs::remove_file(f);
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = stop_recorded(self.dir.path(), self.port);
    }
}

#[test]
fn a_started_server_is_held_by_its_record_and_listens() {
    let r = Rig::new();
    assert!(!listening(r.port), "the positive control: nothing listens before the start");
    let id = r.start().unwrap();
    assert!(listening(r.port));
    let rec = read_record(&record_path(r.dir.path(), r.port)).expect("a record");
    assert_eq!(rec, Record { id, port: r.port });
    assert!(id.confirmed_running());
    assert_eq!(r.probe().unwrap().trim(), "Compile requests 0");
}

/// The user's decision of 2026-11-25: a server that does not answer is stopped by its handle and started fresh; then it answers.
#[test]
fn a_server_that_does_not_answer_is_restarted() {
    let r = Rig::new();
    let first = r.start().unwrap();
    r.hang(true);
    let mut probe = || r.probe();
    let mut restart = || match stop_recorded(r.dir.path(), r.port) {
        Stopped::NotOurs(why) => Err(why),
        Stopped::Ended | Stopped::Gone => {
            // The fresh server answers (the hang belonged to the one that was stopped).
            r.hang(false);
            r.start().map(|_| ())
        }
    };
    let health = answer_or_restart(&mut probe, &mut restart);
    assert!(
        matches!(&health, Health::Answered { restarted: true, stats } if stats.contains("Compile requests")),
        "{health:?}"
    );
    paddock_testkit::wait_until("the hung server was ended", || first.has_ended());
    let now = read_record(&record_path(r.dir.path(), r.port)).unwrap();
    assert_ne!(now.id, first, "a new server, a new record");
    assert!(now.id.confirmed_running());
}

/// The positive control of the probe: a hung server gives no answer within the limit, a healthy one does.
#[test]
fn the_probe_of_a_hung_server_ends_at_its_limit_with_the_cause() {
    let r = Rig::new();
    r.start().unwrap();
    r.hang(true);
    let why = r.probe().unwrap_err();
    assert!(why.contains("did not answer") && why.contains("within"), "{why}");
    r.hang(false);
    assert!(r.probe().is_ok());
}

/// A server this tool did not start (no record, or a record whose start time is not the process's) is never touched; the positive
/// control is the same process with its true record, which is ended.
#[test]
fn a_server_this_tool_did_not_start_is_never_touched() {
    let r = Rig::new();
    let mut other = paddock_testkit::TestCommand::new(r.bin.program())
        .env("SCCACHE_START_SERVER", "1")
        .env("SCCACHE_SERVER_PORT", r.port.to_string())
        .spawn()
        .unwrap();
    paddock_testkit::wait_until("the other server listens", || listening(r.port));
    let id = ProcessId::of(other.id()).unwrap();
    // No record.
    assert!(matches!(stop_recorded(r.dir.path(), r.port), Stopped::NotOurs(why) if why.contains("no record")));
    assert!(id.confirmed_running() && listening(r.port));
    // A record with its pid but another start time (the pid was handed on, or another program's server).
    let path = record_path(r.dir.path(), r.port);
    let wrong = ProcessId::new(id.pid(), StartTime::from_micros(id.started().as_micros() + 5_000_000));
    write_record(&path, Record { id: wrong, port: r.port }).unwrap();
    assert!(matches!(stop_recorded(r.dir.path(), r.port), Stopped::NotOurs(why) if why.contains("something else listens")));
    assert!(id.confirmed_running() && listening(r.port), "still running, still listening");
    // A record of another port.
    write_record(
        &path,
        Record {
            id,
            port: r.port.wrapping_add(1),
        },
    )
    .unwrap();
    assert!(matches!(stop_recorded(r.dir.path(), r.port), Stopped::NotOurs(_)));
    assert!(id.confirmed_running());
    // The positive control: its true record ends it and frees the port.
    write_record(&path, Record { id, port: r.port }).unwrap();
    assert_eq!(stop_recorded(r.dir.path(), r.port), Stopped::Ended);
    let _ = other.wait();
    assert!(!listening(r.port));
    assert!(!path.exists(), "the record goes with the server");
}

/// No answer after the restart either: the caller gets one reason naming both failures, and builds without the cache.
#[test]
fn a_second_failure_is_unanswered_with_both_causes() {
    let r = Rig::new();
    r.start().unwrap();
    r.hang(true);
    let mut probe = || r.probe();
    let mut restart = || match stop_recorded(r.dir.path(), r.port) {
        Stopped::NotOurs(why) => Err(why),
        Stopped::Ended | Stopped::Gone => r.start().map(|_| ()),
    };
    match answer_or_restart(&mut probe, &mut restart) {
        Health::Unanswered(why) => assert!(
            why.contains("restarted, it still gave no answer") && why.matches("within").count() == 2,
            "{why}"
        ),
        other => panic!("{other:?}"),
    }
    // A restart that is refused (not ours) is named too, and nothing more is probed.
    let mut probes = 0;
    let mut p = || {
        probes += 1;
        Err("no answer".to_string())
    };
    let mut refuse = || Err("this tool did not start it".to_string());
    assert_eq!(
        answer_or_restart(&mut p, &mut refuse),
        Health::Unanswered("no answer, and it was not restarted: this tool did not start it".into())
    );
    assert_eq!(probes, 1);
}

/// A second server on a taken port exits at once; its start says so in sccache's words, which the callers read as "already running".
#[test]
fn a_start_on_a_taken_port_fails_at_once_with_address_in_use() {
    let r = Rig::new();
    r.start().unwrap();
    let why = r.start().unwrap_err();
    assert!(address_in_use(&why), "{why}");
    assert!(address_in_use("sccache: error: Address already in use (os error 48)"));
    assert!(address_in_use("Server startup failed: Address in use"));
    assert!(!address_in_use("permission denied"));
}

#[test]
fn a_record_that_cannot_be_read_is_no_record() {
    let d = tempfile::tempdir().unwrap();
    let p = record_path(d.path(), 4226);
    assert_eq!(read_record(&p), None);
    std::fs::write(&p, "{\"pid\": 1}").unwrap();
    assert_eq!(read_record(&p), None);
    let id = ProcessId::new(7, StartTime::from_micros(123));
    write_record(&p, Record { id, port: 4226 }).unwrap();
    assert_eq!(read_record(&p), Some(Record { id, port: 4226 }));
}

/// Two callers never restart one server at once: while one runs inside the lock, the lock file cannot be taken; after, it can.
#[test]
fn the_restart_lock_is_held_while_its_caller_runs() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("restart-4226.lock");
    let other = || std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    let inside = with_restart_lock(d.path(), 4226, || {
        matches!(other().try_lock(), Err(std::fs::TryLockError::WouldBlock))
    })
    .unwrap();
    assert!(inside, "a second caller is kept out while the first runs");
    assert!(other().try_lock().is_ok(), "the positive control: free once the first is done");
}
