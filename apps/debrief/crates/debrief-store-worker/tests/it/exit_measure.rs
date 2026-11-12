//! How the worker ends when Debrief stops it, over many runs: the measurement behind the exit-phase amendment of ADR 0076
//! (WP-X0, ER-28). The store worker crashed in DuckDB's static destructors at exit (21 reports on one Mac in six days).
//!
//! - `the_worker_ends_cleanly_when_debrief_stops_it` runs one round on every `cargo test` (the worker exits with status 0
//!   within its stop budget plus the margin, never by a signal).
//! - `measure_the_workers_exit_under_load` is the measurement, `#[ignore]`d: each round starts a real worker, imports the
//!   samples (invented data), and stops the worker the way Debrief does (`StoreWorker::shutdown`) the moment the import is
//!   over, while the full-text indexer and the names catalogue are still working. It reads how the worker ended through
//!   `waitid` (`WNOWAIT`: the supervisor still reaps it) and prints one `EXIT-MEASURE` line per run and a summary.
//!
//!   `DEBRIEF_EXIT_RUNS=<n>` rounds, `DEBRIEF_EXIT_PAR=<k>` at once (default 1), `DEBRIEF_EXIT_BUSY=<m>` busy threads that
//!   load the machine meanwhile, `DEBRIEF_EXIT_ARM=<name>` a label for the output. Run:
//!   `DEBRIEF_EXIT_RUNS=100 cargo test -p debrief-store-worker --test it measure_the_workers_exit -- --ignored --nocapture`

#![cfg(unix)]

use super::worker::{confined, copy_dir, log_samples, TempDir};
use debrief_core::ports::Ingest;
use debrief_core::Profile;
use debrief_store_worker::{MemoryMode, StoreWorker};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How a worker process ended, read from its wait status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Exited(i32),
    Killed(i32),
    Dumped(i32),
    /// The supervisor reaped it before this read it.
    Unknown,
}

impl Ended {
    /// Ended by SIGKILL: the parent's kill after its grace, not a crash of the worker.
    fn parent_killed(self) -> bool {
        self == Ended::Killed(libc::SIGKILL)
    }

    /// A crash: any other signal (SIGSEGV, SIGBUS, SIGABRT, ...) or a non-zero exit status.
    fn crashed(self) -> bool {
        match self {
            Ended::Killed(sig) => sig != libc::SIGKILL,
            Ended::Dumped(_) => true,
            Ended::Exited(code) => code != 0,
            Ended::Unknown => false,
        }
    }
}

/// Blocks until `pid` (a child of this process) ends and says how, leaving it unreaped.
fn wait_how(pid: u32) -> Ended {
    let id = libc::id_t::try_from(pid).unwrap();
    // SAFETY: a zeroed siginfo_t is a valid out-parameter for waitid.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    loop {
        // SAFETY: a plain system call on our own child with a valid out-pointer.
        let r = unsafe { libc::waitid(libc::P_PID, id, &mut info, libc::WEXITED | libc::WNOWAIT) };
        if r == 0 {
            break;
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) => continue,
            _ => return Ended::Unknown,
        }
    }
    // SAFETY: `waitid` filled `info` for a child that ended.
    let status = unsafe { info.si_status() };
    match info.si_code {
        libc::CLD_EXITED => Ended::Exited(status),
        libc::CLD_KILLED => Ended::Killed(status),
        libc::CLD_DUMPED => Ended::Dumped(status),
        _ => Ended::Unknown,
    }
}

struct Round {
    ended: Ended,
    /// From the stop request to the process being gone.
    stop: Duration,
}

/// One worker: import, stop it the moment the import is over, read how it ended.
fn one_round(copies: usize) -> Round {
    let data = TempDir::new("exit-data");
    let logs: Vec<std::path::PathBuf> = (0..copies)
        .map(|i| {
            let d = data.0.join(format!("copy{i}"));
            for p in log_samples() {
                copy_dir(&p, &d.join(p.file_name().unwrap()));
            }
            d
        })
        .collect();
    let dir = TempDir::new("exit-store");
    crate::warm_binaries();
    let mut cfg = confined(env!("CARGO_BIN_EXE_debrief-store-worker"));
    cfg.memory_mode = MemoryMode::Normal;
    // Reaches the worker's environment: the hook of a temporary measurement build (the shipped worker has none).
    if let Ok(v) = std::env::var("DEBRIEF_EXIT_HOLD") {
        cfg.env.push(("DEBRIEF_EXIT_HOLD".into(), v));
    }
    let phase = crate::Phases::new("exit round");
    let w = StoreWorker::new(cfg).unwrap();
    let store = w.open_store(&dir.0, Profile::default_profile(), true, None).unwrap();
    let pid = w.status().pid.expect("the worker runs");
    phase.at(&format!("opened in the worker (pid {pid})"));
    store.open(&logs).unwrap().wait().unwrap();
    phase.at("imported in the worker; stopping it");
    let waiter = std::thread::spawn(move || wait_how(pid));
    let started = Instant::now();
    w.shutdown();
    let ended = waiter.join().unwrap();
    phase.at(&format!("the worker ended: {ended:?}"));
    Round {
        ended,
        stop: started.elapsed(),
    }
}

#[test]
fn the_worker_ends_cleanly_when_debrief_stops_it() {
    let round = one_round(1);
    assert!(
        matches!(round.ended, Ended::Exited(0) | Ended::Unknown),
        "the worker did not exit with status 0: {:?}",
        round.ended
    );
    assert!(
        round.stop < debrief_ipc::process::KILL_GRACE + Duration::from_secs(5),
        "the stop took {:?}",
        round.stop
    );
}

/// `run()` joins the store threads before `main` returns: the catalogue thread is held inside DuckDB by the test hook
/// (`DEBRIEF_EXIT_HOLD`, feature `exit-hold`) until it is stopped, and writes `stopped` when it is. A worker that returns
/// without stopping it never writes it (and can crash in DuckDB's static destructors with the thread live).
#[cfg(debug_assertions)] // the hold is not compiled into a release build
#[test]
fn the_worker_stops_its_store_threads_before_it_exits() {
    let phase = crate::Phases::new("stops_its_store_threads");
    crate::warm_binaries();
    phase.at("binaries warmed");
    let marker = TempDir::new("exit-hold");
    let note = marker.0.join("hold.log");
    let dir = TempDir::new("exit-hold-store");
    let mut cfg = confined(env!("CARGO_BIN_EXE_debrief-store-worker"));
    cfg.env.push(("DEBRIEF_EXIT_HOLD".into(), note.display().to_string()));
    let w = StoreWorker::new(cfg).unwrap();
    let _store = w.open_store(&dir.0, Profile::default_profile(), true, None).unwrap();
    let pid = w.status().pid.expect("the worker runs");
    phase.at(&format!("opened in the worker (pid {pid})"));
    let held = || std::fs::read_to_string(&note).is_ok_and(|t| t.lines().any(|l| l == "held"));
    paddock_testkit::wait_until("the catalogue thread to be held inside DuckDB", held);
    phase.at("the catalogue thread is held; stopping the worker");
    let waiter = std::thread::spawn(move || wait_how(pid));
    w.shutdown();
    let ended = waiter.join().unwrap();
    phase.at(&format!("the worker ended: {ended:?}"));
    assert!(
        matches!(ended, Ended::Exited(0) | Ended::Unknown),
        "the worker did not exit with status 0: {ended:?}"
    );
    let text = std::fs::read_to_string(&note).unwrap_or_default();
    assert!(
        text.lines().any(|l| l == "stopped by stop_for_exit"),
        "the worker did not stop the thread through stop_for_exit before it exited: {text:?}"
    );
}

#[test]
#[ignore = "a measurement: see the module docs (DEBRIEF_EXIT_RUNS, DEBRIEF_EXIT_PAR, DEBRIEF_EXIT_BUSY)"]
fn measure_the_workers_exit_under_load() {
    let env = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (runs, par, busy) = (
        env("DEBRIEF_EXIT_RUNS", 10),
        env("DEBRIEF_EXIT_PAR", 1).max(1),
        env("DEBRIEF_EXIT_BUSY", 0),
    );
    let copies = env("DEBRIEF_EXIT_COPIES", 1).max(1);
    let arm = std::env::var("DEBRIEF_EXIT_ARM").unwrap_or_else(|_| "unlabelled".into());
    let stop = Arc::new(AtomicBool::new(false));
    let hogs: Vec<_> = (0..busy)
        .map(|_| {
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut x = 1u64;
                while !stop.load(Ordering::Relaxed) {
                    x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407));
                }
            })
        })
        .collect();
    let next = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = (0..par)
        .map(|_| {
            let next = next.clone();
            let arm = arm.clone();
            std::thread::spawn(move || {
                let mut results = Vec::new();
                loop {
                    let n = next.fetch_add(1, Ordering::SeqCst);
                    if n >= runs {
                        break;
                    }
                    // A round on a thread of its own: its confined temp folder goes with the thread.
                    let r = std::thread::spawn(move || one_round(copies)).join().unwrap();
                    println!("EXIT-MEASURE arm={arm} run={n} ended={:?} stop_ms={}", r.ended, r.stop.as_millis());
                    results.push(r.ended);
                }
                results
            })
        })
        .collect();
    let all: Vec<Ended> = workers.into_iter().flat_map(|w| w.join().unwrap()).collect();
    stop.store(true, Ordering::Relaxed);
    for h in hogs {
        h.join().unwrap();
    }
    let crashed = all.iter().filter(|e| e.crashed()).count();
    let unknown = all.iter().filter(|e| **e == Ended::Unknown).count();
    let parent_killed = all.iter().filter(|e| e.parent_killed()).count();
    println!(
        "EXIT-MEASURE-SUMMARY arm={arm} runs={} crashed={crashed} parent_killed={parent_killed} unknown={unknown} par={par} busy={busy} copies={copies}",
        all.len()
    );
}
