use std::sync::{Arc, Mutex};
use std::time::Duration;

use pacenotes_core::RepoPath;
use pacenotes_worker::stall::{RunState, StallLimits};
use pacenotes_worker::{OsProbe, ParseWorkers, ProcSample, ProcessProbe, WorkerSettings};

pub fn p(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}

pub const WORKER: &str = env!("CARGO_BIN_EXE_pacenotes-parse-worker");

/// Fast settings for tests: a beat of 40 ms (a unit of ticks, not a deadline) and small stall limits.
pub fn settings() -> WorkerSettings {
    WorkerSettings {
        stall: StallLimits {
            blocked_ticks: 5,
            spin_cpu_micros: 300_000,
        },
        beat: Duration::from_millis(40),
        // The test workers' coverage profiles go nowhere: a worker the host ends (a crash, a stall, a Cancel, a pool shut down) leaves a
        // torn profile that fails `cargo llvm-cov` ("no profile can be merged"). The worker's own code is covered in-process
        // (`serve::tests`), where it can be.
        envs: vec![("LLVM_PROFILE_FILE".into(), "/dev/null".into())],
        ..WorkerSettings::new(WORKER)
    }
}

pub fn workers() -> ParseWorkers {
    ParseWorkers::new(settings(), Arc::new(OsProbe))
}

/// A probe that reports what the test says, whatever the process does.
pub struct FixedProbe(pub Mutex<ProcSample>, pub std::sync::atomic::AtomicUsize);

impl FixedProbe {
    pub fn stopped() -> FixedProbe {
        FixedProbe(
            Mutex::new(ProcSample {
                state: RunState::Stopped,
                cpu_micros: 0,
            }),
            std::sync::atomic::AtomicUsize::new(0),
        )
    }

    /// How many times the probe was asked.
    pub fn count(&self) -> usize {
        self.1.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ProcessProbe for FixedProbe {
    fn sample(&self, _pid: u32) -> Option<ProcSample> {
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Some(*self.0.lock().unwrap())
    }
}
