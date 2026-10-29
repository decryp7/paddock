//! Doubles: a sink that keeps every snapshot and lets a test wait on a condition, a symbol source that knows no language
//! (every file stays at tier 1: listed, not parsed), and a service over a scratch workspace.

use pacenotes_core::RepoPath;
use pacenotes_engine::{IndexService, IndexSetup};
use pacenotes_index::{Denylist, FileFacts, IndexLimits, SourceError, SymbolSource};
use paddock_jobs::time::FakeClock;
use paddock_jobs::{CancelToken, EventSink, JobPool, JobRegistry, JobSnapshot, Workers};
use paddock_testkit::Scratch;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Stops a test that waits for something that never happens; never the thing a test measures.
const HANG_GUARD: Duration = Duration::from_secs(120);

#[derive(Default)]
pub struct Sink {
    seen: Mutex<Vec<JobSnapshot>>,
    changed: Condvar,
}

impl EventSink for Sink {
    fn emit(&self, snapshot: &JobSnapshot) {
        self.seen.lock().unwrap().push(snapshot.clone());
        self.changed.notify_all();
    }
}

impl Sink {
    pub fn all(&self) -> Vec<JobSnapshot> {
        self.seen.lock().unwrap().clone()
    }

    /// Waits until `ready` holds for the snapshots seen so far.
    pub fn wait_for(&self, ready: impl Fn(&[JobSnapshot]) -> bool) -> Vec<JobSnapshot> {
        let mut seen = self.seen.lock().unwrap();
        while !ready(&seen) {
            let (next, timeout) = self.changed.wait_timeout(seen, HANG_GUARD).unwrap();
            assert!(!timeout.timed_out(), "no event satisfied the wait");
            seen = next;
        }
        seen.clone()
    }
}

impl Sink {
    /// Waits for `ready` while moving the fake clock forward whenever nothing happens, so the watcher's quiet time passes
    /// whatever order its thread took the events in.
    pub fn wait_driving(&self, clock: &FakeClock, ready: impl Fn(&[JobSnapshot]) -> bool) -> Vec<JobSnapshot> {
        let started = std::time::Instant::now();
        loop {
            clock.advance(Duration::from_secs(2));
            let seen = self.seen.lock().unwrap();
            if ready(&seen) {
                return seen.clone();
            }
            let (seen, _) = self.changed.wait_timeout(seen, Duration::from_millis(20)).unwrap();
            if ready(&seen) {
                return seen.clone();
            }
            assert!(started.elapsed() < HANG_GUARD, "no event satisfied the wait");
        }
    }
}

pub fn ended(seen: &[JobSnapshot], runs: usize) -> bool {
    seen.iter()
        .filter(|s| {
            matches!(
                s.state,
                paddock_jobs::JobState::Done | paddock_jobs::JobState::Failed { .. } | paddock_jobs::JobState::Cancelled { .. }
            )
        })
        .count()
        >= runs
}

pub fn phase_ids(seen: &[JobSnapshot]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in seen {
        if let paddock_jobs::JobState::Running { phase: Some(p) } = &s.state
            && out.last() != Some(&p.phase.id)
        {
            out.push(p.phase.id.clone());
        }
    }
    out
}

pub struct NoLanguages;

impl SymbolSource for NoLanguages {
    fn extract(&self, _: &RepoPath, _: &[u8], _: &CancelToken) -> Result<Option<FileFacts>, SourceError> {
        Ok(None)
    }
}

pub struct Rig {
    pub scratch: Scratch,
    pub sink: Arc<Sink>,
    pub registry: Arc<JobRegistry>,
    pub service: IndexService,
    pub clock: FakeClock,
    pub pool: Arc<JobPool>,
}

pub fn rig(files: &[(&str, &str)]) -> Rig {
    let scratch = Scratch::new();
    for (name, content) in files {
        scratch.write(&format!("ws/{name}"), content).unwrap();
    }
    let sink = Arc::new(Sink::default());
    let clock = FakeClock::new();
    let registry = Arc::new(JobRegistry::with_clock(sink.clone(), Arc::new(clock.clone()), 0));
    let pool = Arc::new(JobPool::new(Workers::new(2)));
    let setup = Arc::new(IndexSetup {
        root: scratch.path("ws"),
        store_path: scratch.path("index/index.db"),
        source: Arc::new(NoLanguages),
        denylist: Arc::new(Denylist::new()),
        limits: IndexLimits::default(),
        read_workers: Workers::new(2),
        parse_workers: Workers::one(),
    });
    let service = IndexService::new(registry.clone(), pool.clone(), setup);
    Rig {
        scratch,
        sink,
        registry,
        service,
        clock,
        pool,
    }
}
