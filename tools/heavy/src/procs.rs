//! The process table port and the liveness rule (rule 53, ADR 0033): a pid counts as the recorded process only when its OS
//! start time is not later than the record's epoch (+ slack). Never a formatted start text, never the locale or the TZ.

use std::collections::HashMap;
use std::sync::Mutex;

/// What the OS says about a pid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// No such process (or a zombie that only waits to be reaped).
    Gone,
    /// A process holds the pid. `start_micros` is its OS start time (µs since the epoch) when readable; a process we may
    /// not inspect (EPERM, hidepid) is present with `None`.
    Present {
        /// OS start time in microseconds since the epoch.
        start_micros: Option<u64>,
    },
}

/// The process table.
pub trait Procs: Send + Sync {
    /// Who holds `pid` now.
    fn probe(&self, pid: u32) -> Presence;
}

/// Seconds a process may have started after the record's epoch and still be its writer (the record is written a moment
/// after the process starts; clocks tick in whole seconds).
pub const START_SLACK_SECS: u64 = 2;

/// Whether the process recorded as `pid`, with a record written at `record_epoch` (None: unknown), is still the holder.
/// A live pid is never called dead: unreadable start time, unknown epoch and EPERM all count as alive.
pub fn pid_alive(procs: &dyn Procs, pid: u32, record_epoch: Option<u64>) -> bool {
    match procs.probe(pid) {
        Presence::Gone => false,
        Presence::Present { start_micros: None } => true,
        Presence::Present { start_micros: Some(s) } => match record_epoch {
            None => true,
            Some(e) => s / 1_000_000 <= e.saturating_add(START_SLACK_SECS),
        },
    }
}

/// The evidence behind a liveness decision, for the log.
pub fn evidence(procs: &dyn Procs, pid: u32, record_epoch: Option<u64>) -> String {
    let seen = match procs.probe(pid) {
        Presence::Gone => "no such process".to_string(),
        Presence::Present { start_micros: None } => "present, start time unreadable".to_string(),
        Presence::Present { start_micros: Some(s) } => format!("os start {s} us"),
    };
    let ep = record_epoch.map_or("unknown".to_string(), |e| e.to_string());
    format!("pid {pid}: {seen}; record epoch {ep} (+{START_SLACK_SECS} s slack)")
}

/// The real process table.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsProcs;

impl Procs for OsProcs {
    fn probe(&self, pid: u32) -> Presence {
        if pid == 0 || i32::try_from(pid).is_err() {
            return Presence::Gone;
        }
        let id = paddock_proc::ProcessId::of(pid);
        let start = id.map(|p| p.started().as_micros());
        if !crate::probe::exists(pid) {
            return Presence::Gone;
        }
        // A zombie shows a start time but is not running (a container whose PID 1 never reaps would hold the lock forever).
        // An unreadable process (pid 1 to a non-root macOS user) is unknown, not gone: `Presence::may_be_alive` counts unknown as alive.
        if id.is_some_and(|p| !p.presence().may_be_alive()) {
            return Presence::Gone;
        }
        Presence::Present { start_micros: start }
    }
}

/// A process table made by hand, for tests.
#[derive(Debug, Default)]
pub struct FakeProcs(Mutex<HashMap<u32, Presence>>);

impl FakeProcs {
    /// An empty table: every pid is gone.
    pub fn new() -> FakeProcs {
        FakeProcs::default()
    }
    /// `pid` runs and started at `start_secs`.
    pub fn run(&self, pid: u32, start_secs: u64) {
        self.set(
            pid,
            Presence::Present {
                start_micros: Some(start_secs * 1_000_000),
            },
        );
    }
    /// `pid` exists but its start time cannot be read (EPERM).
    pub fn run_opaque(&self, pid: u32) {
        self.set(pid, Presence::Present { start_micros: None });
    }
    /// `pid` ended.
    pub fn end(&self, pid: u32) {
        if let Ok(mut m) = self.0.lock() {
            m.remove(&pid);
        }
    }
    fn set(&self, pid: u32, p: Presence) {
        if let Ok(mut m) = self.0.lock() {
            m.insert(pid, p);
        }
    }
}

impl Procs for FakeProcs {
    fn probe(&self, pid: u32) -> Presence {
        self.0.lock().ok().and_then(|m| m.get(&pid).copied()).unwrap_or(Presence::Gone)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// Pid 1 exists on every Unix and a non-root user may be refused a look at it (macOS lists it with no start time or
    /// status): "unknown" must read as present, or a lock held by it is broken as stale.
    #[test]
    fn pid_one_probes_as_present() {
        assert!(matches!(OsProcs.probe(1), Presence::Present { .. }), "{:?}", OsProcs.probe(1));
    }
}
