#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Per-program traffic end to end (helper-wiring plan section 2.5, decision D6): a real agent over the fake platform whose helper is
//! a real `Dispatcher` (`common/helper_rig.rs`). The helper scopes the counters to the account that asks, so the same machine
//! shows a standard user only that user's programs and the system's, and the other accounts' programs as one total. (The rows are
//! rates between two reads, so the rows are tested below with two reads whose time is passed to the tracker as a value, and the
//! total, which a first read already has, in the other tests.)

use dyno_agent::cli::{ask_within, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::helper::HelperHost;
use dyno_core::helper_processes::{ProcState, RawOwner, RawProcess};
use dyno_core::helper_wire::HelperUser;
use dyno_core::network::{NetworkQuery, NetworkSnapshot};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::protocol::{Reply, Request};
use dyno_core::read_via::ReadVia;
use dyno_platform::fake::Fake;
use std::time::Duration;

#[path = "common/helper_rig.rs"]
mod helper_rig;
use helper_rig::{dispatcher, wiring, Caller, DispatcherTransport, Readers};

const SLOW: Duration = Duration::from_secs(60);

fn process(pid: u32, owner: RawOwner) -> RawProcess {
    RawProcess {
        pid,
        started: Some(1),
        parent: Some(1),
        name: format!("program{pid}"),
        path: None,
        owner,
        state: ProcState::Running,
        threads: Some(1),
        cpu_time_ms: Some(1),
        memory_bytes: Some(1),
        disk_read_bytes: Some(0),
        disk_written_bytes: Some(0),
    }
}

fn account(caller: Caller) -> RawOwner {
    RawOwner::Account {
        id: HelperUser::Uid(caller.uid()),
        name: Some(format!("user{}", caller.uid())),
    }
}

fn counter(pid: u32, name: &str, bytes_in: u64) -> TrafficCounter {
    TrafficCounter {
        pid,
        name: name.into(),
        bytes_in,
        bytes_out: 1,
    }
}

struct Rig {
    running: Option<Running>,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

impl Rig {
    /// A Windows agent whose helper is asked as `caller`. The owner's program is pid 10, the system's pid 30, another account's
    /// pids 20 and 21 (from the point of view of any caller but that account).
    fn start(caller: Caller, up: bool) -> Rig {
        Rig::start_with(caller, up, false)
    }

    fn start_with(caller: Caller, up: bool, grows: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        let mine = account(caller);
        let someone_else = RawOwner::Account {
            id: HelperUser::Uid(9_999),
            name: Some("someone-else".into()),
        };
        let readers = Readers {
            traffic: vec![
                counter(10, "mine.exe", 100),
                counter(20, "secret-tool.exe", 5_000),
                counter(21, "secret-two.exe", 6_000),
                counter(30, "svchost.exe", 7),
            ],
            processes: vec![
                process(10, mine),
                process(20, someone_else.clone()),
                process(21, someone_else),
                process(30, RawOwner::System),
            ],
            traffic_grows: grows,
            ..Readers::default()
        };
        let transport = DispatcherTransport::new(dispatcher(HelperHost::Windows, readers, tmp.path()), caller);
        transport.up.store(up, std::sync::atomic::Ordering::SeqCst);
        let running = Running::start_with_helper(
            dirs.clone(),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
            wiring(transport, HelperHost::Windows),
        )
        .unwrap();
        Rig {
            running: Some(running),
            dirs,
            _tmp: tmp,
        }
    }

    /// The network snapshot once the background traffic read has filled it (the first answer says it is still reading).
    fn network(&self) -> NetworkSnapshot {
        let mut last = None;
        let _ = paddock_testkit::poll_until_within(Duration::from_secs(30), || {
            let peer = connect(&self.dirs).unwrap().0;
            let reply = ask_within(
                &peer,
                Request::Network {
                    query: NetworkQuery::default(),
                },
                SLOW,
            );
            // A dropped peer keeps its connection open: a polling loop would pile them up (CI run 7763).
            peer.close();
            let Reply::Network(n) = reply.unwrap() else { panic!("network") };
            let reading = n.traffic_note.as_deref().is_some_and(|t| t.starts_with("Reading traffic"));
            last = Some(*n);
            !reading
        });
        last.expect("polled at least once")
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

#[test]
fn the_rows_of_a_standard_user_hold_only_its_own_and_the_systems_programs() {
    use dyno_core::network_traffic::TrafficTracker;
    use dyno_core::time::Timestamp;
    let rig = Rig::start_with(Caller::Other, true, true);
    let shared = rig.running.as_ref().unwrap().shared.clone();
    // The agent's own link to the helper, as the network centre reads it: two reads, the second 8 s after the first.
    let mut tracker = TrafficTracker::new();
    let first = shared.network.counters().unwrap();
    assert!(tracker.update(Timestamp(0), &first).is_empty(), "a first read has no rates");
    let second = shared.network.counters().unwrap();
    let rows = tracker.update(Timestamp(8_000), &second);
    let mut names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["program10", "program30"], "{rows:?}");
}

#[test]
fn the_owner_sees_every_programs_traffic_and_it_says_the_helper_read_it() {
    let rig = Rig::start(Caller::Owner, true);
    let n = rig.network();
    assert_eq!(n.traffic_via, ReadVia::Helper, "{:?}", n.traffic_note);
    assert!(n.traffic_others.is_none(), "the owner sees every program in detail, not a total");
}

#[test]
fn a_standard_user_sees_only_their_own_and_the_systems_programs_and_the_rest_as_one_total() {
    let rig = Rig::start(Caller::Other, true);
    let n = rig.network();
    assert_eq!(n.traffic_via, ReadVia::Helper, "{:?}", n.traffic_note);
    let others = n.traffic_others.expect("the others' total");
    assert_eq!((others.programs, others.bytes_in), (2, 11_000));
    let everything = format!("{n:?}");
    assert!(
        !everything.contains("secret-tool") && !everything.contains("secret-two"),
        "another account's program names reached the page"
    );
}

#[test]
fn without_the_helper_windows_own_refusal_is_the_reason_and_the_page_asks_for_the_helper() {
    let rig = Rig::start(Caller::Owner, false);
    let n = rig.network();
    assert!(n.traffic.is_empty());
    assert!(matches!(n.traffic_via, ReadVia::NeedsHelper { .. }), "{:?}", n.traffic_via);
}
