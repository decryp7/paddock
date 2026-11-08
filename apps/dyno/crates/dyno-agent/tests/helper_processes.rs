#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Every account's processes through a REAL helper dispatcher (helper-wiring plan section 2.4 and 4.2): the wire, the caller's
//! scope and the agent's merge are the real ones; the OS reader is a fixed list. Two sweeps give a CPU rate.

use dyno_agent::helper_first::CentrePort;
use dyno_agent::helper_readers::processes::{HelperFirstProcesses, SweepStore, SweepWorker};
use dyno_core::helper::HelperHost;
use dyno_core::helper_processes::{ProcState, RawOwner, RawProcess};
use dyno_core::helper_wire::HelperUser;
use dyno_core::ports::{PlatformError, ProcessSource};
use dyno_core::process::{Measured, Owner, ProcessDetails, ProcessRef};
use dyno_core::read_via::ReadVia;
use paddock_jobs::time::FakeClock;
use std::sync::Arc;
use std::time::Duration;

#[path = "common/helper_rig.rs"]
mod helper_rig;
use helper_rig::{centre_over, dispatcher, owner, Caller, DispatcherTransport, Readers};

struct NoLocal;
impl ProcessSource for NoLocal {
    fn processes(&mut self) -> Vec<dyno_core::process::ProcessSample> {
        Vec::new()
    }
    fn details(&mut self, _: ProcessRef) -> Result<ProcessDetails, PlatformError> {
        Err(PlatformError::Unsupported("none".into()))
    }
}

fn raw(pid: u32, who: RawOwner, cpu_ms: u64) -> RawProcess {
    RawProcess {
        pid,
        started: Some(100),
        parent: None,
        name: format!("proc{pid}"),
        path: None,
        owner: who,
        state: ProcState::Running,
        threads: None,
        cpu_time_ms: Some(cpu_ms),
        memory_bytes: Some(1 << 20),
        disk_read_bytes: None,
        disk_written_bytes: None,
    }
}

fn machine() -> Vec<RawProcess> {
    let sam = RawOwner::Account {
        id: HelperUser::parse("uid:502").unwrap(),
        name: Some("sam".into()),
    };
    let me = RawOwner::Account {
        id: owner(),
        name: Some("me".into()),
    };
    vec![
        raw(1, RawOwner::System, 10),
        raw(2, me, 10),
        raw(3, sam, 1_000),
        raw(4, RawOwner::Unknown, 10),
    ]
}

fn run(
    caller: Caller,
) -> (
    HelperFirstProcesses,
    Vec<dyno_core::process::ProcessSample>,
    Vec<dyno_core::process::ProcessSample>,
) {
    let dir = std::env::temp_dir().join(format!("dyno-helper-processes-{}-{:?}", std::process::id(), caller));
    std::fs::create_dir_all(&dir).unwrap();
    let d = dispatcher(
        HelperHost::Linux,
        Readers {
            processes: machine(),
            ..Readers::default()
        },
        &dir,
    );
    let c = Arc::new(centre_over(DispatcherTransport::new(d, caller), HelperHost::Linux));
    let clock = FakeClock::new();
    let store = SweepStore::new(Arc::new(clock.clone()));
    let mut w = SweepWorker::new(Box::new(CentrePort(c)), store.clone(), 1);
    w.sweep_once();
    let mut src = HelperFirstProcesses::new(Box::new(NoLocal), store);
    let first = src.processes();
    clock.advance(Duration::from_secs(5));
    w.sweep_once();
    let second = src.processes();
    let _ = std::fs::remove_dir_all(&dir);
    (src, first, second)
}

#[test]
fn the_owner_sees_every_account_and_a_second_sweep_gives_a_cpu_rate() {
    let (src, first, second) = run(Caller::Owner);
    assert_eq!(src.provenance().processes, ReadVia::Helper);
    let sam = second
        .iter()
        .find(|p| p.pid == 3)
        .expect("the other account's process crosses the real wire");
    assert_eq!((sam.owner, sam.user_name()), (Owner::Other, Some("sam")));
    assert!(second.iter().any(|p| p.pid == 1 && p.owner == Owner::System));
    assert!(
        first.iter().find(|p| p.pid == 3).unwrap().cpu.is_unavailable(),
        "no rate from one sweep"
    );
    // The fake machine's CPU time did not move between the two sweeps: a rate of 0 % that was measured is a value.
    assert_eq!(sam.cpu, Measured::Value(0.0));
}

#[test]
fn another_standard_user_gets_a_total_and_no_names() {
    let (src, _, table) = run(Caller::Other);
    assert!(
        table.iter().all(|p| p.pid != 3),
        "the other account's process is not listed to a non-owner"
    );
    assert!(
        table.iter().all(|p| p.pid != 2),
        "the owner's process is another account's to this caller too"
    );
    let prov = src.provenance();
    let o = prov.others_total.expect("the helper's total");
    // The owner's, sam's and the one with no known owner are all another account's to this caller: counted, never listed.
    assert_eq!((o.count, prov.others), (3, ReadVia::Helper));
}

#[test]
fn a_process_with_no_known_owner_is_another_accounts_never_the_systems() {
    // The agent's path uses the helper's one owner rule (`see()`, rule 55) and has no second one.
    let (_, _, owner_sees) = run(Caller::Owner);
    let p = owner_sees
        .iter()
        .find(|p| p.pid == 4)
        .expect("the owner sees it, as another account's");
    assert_eq!(p.owner, Owner::Other);
    let (_, _, other_sees) = run(Caller::Other);
    assert!(
        other_sees.iter().all(|p| p.pid != 4),
        "a standard account does not see it by name or pid"
    );
}

/// Rule 48: what one real sweep and its pages cost on this machine. A measurement, not a test (it reads this computer's own process
/// list, which ER-14 keeps out of the suite): `cargo test -p dyno-agent --release --test helper_processes -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement on the real machine, run by hand (rule 48)"]
fn measure_a_real_sweep() {
    use dyno_core::helper_policy::Visibility;
    use dyno_core::helper_processes::{build_page, ProcessSweeper};
    use dyno_core::helper_requests::ProcessQuery;
    let host = if cfg!(target_os = "macos") {
        HelperHost::Macos
    } else if cfg!(windows) {
        HelperHost::Windows
    } else {
        HelperHost::Linux
    };
    let sweeper = dyno_platform::helper_sweep::SystemSweeper::new(host, Box::new(dyno_platform::runner::SystemRunner));
    for round in 0..3 {
        let t = std::time::Instant::now();
        let sweep = sweeper.sweep().unwrap();
        let swept = t.elapsed();
        let t = std::time::Instant::now();
        let mut pages = 0;
        let mut after = None;
        loop {
            let q = ProcessQuery {
                after,
                limit: 1_024,
                ..Default::default()
            };
            let page = build_page(&sweep, &owner(), Visibility::Full, &q);
            pages += 1;
            match page.next {
                Some(n) => after = Some(n),
                None => break,
            }
        }
        println!(
            "round={round} processes={} sweep={swept:?} build_pages({pages})={:?}",
            sweep.processes.len(),
            t.elapsed()
        );
    }
}
