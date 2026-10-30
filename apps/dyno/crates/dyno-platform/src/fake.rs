//! In-memory implementations of every port for the agent's tests: scriptable samples and processes, recorded
//! actions and injectable failures. All handles of one [`Fake`] share its state.
//!
//! ```
//! use dyno_platform::fake::{Fake, FakeOp};
//! use dyno_core::ports::PlatformError;
//! let fake = Fake::new();
//! fake.add_process(dyno_platform::fake::process(42, "busy.exe", 80.0));
//! fake.fail(FakeOp::Details, PlatformError::AccessDenied("no".into()));
//! let mut p = fake.platform();
//! assert_eq!(p.processes.processes().len(), 1);
//! ```

use crate::Platform;
use dyno_core::facts::SystemFacts;
use dyno_core::metrics::{CpuSample, DiskIo, MemoryPressure, MemorySample, Reading, SystemSample};
use dyno_core::ports::{FactsSource, FileLocks, NetworkLinks, PlatformError, ProcessSource, StartupItems, SystemSensors, UserActivity};
use dyno_core::process::{ListOrReason, Locker, Measured, Owner, Pid, ProcessDetails, ProcessRef, ProcessSample, RunState, SignatureInfo};
use dyno_core::startup::StartupEntry;
use dyno_core::time::Timestamp;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// A fallible fake operation, for [`Fake::fail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FakeOp {
    /// [`ProcessSource::details`].
    Details,
    /// [`FileLocks::lockers`].
    Lockers,
    /// [`StartupItems::list`].
    StartupList,
}

/// Everything the fakes read and record. Tests reach it through [`Fake::with`].
#[derive(Debug, Default)]
pub struct FakeState {
    /// Samples returned in order; the last one repeats (with `at` set to the call's time).
    pub samples: VecDeque<SystemSample>,
    /// The last sample returned.
    pub last_sample: Option<SystemSample>,
    /// The process table.
    pub processes: Vec<ProcessSample>,
    /// Details by process; missing ones are built from the table.
    pub details: HashMap<ProcessRef, ProcessDetails>,
    /// Lockers by path.
    pub lockers: HashMap<PathBuf, Vec<Locker>>,
    /// Startup entries (`set_state` changes them).
    pub startup: Vec<StartupEntry>,
    /// Facts (default: nothing known).
    pub facts: Option<SystemFacts>,
    /// Seconds since input.
    pub idle_secs: Option<u64>,
    /// The foreground process.
    pub foreground: Option<Pid>,
    /// What the network looks like now (default: nothing; `None` makes the read fail).
    pub net: Option<dyno_core::netchange::NetState>,
    /// Injected failures and how many more calls they affect (`None`: every call).
    pub failures: HashMap<FakeOp, (PlatformError, Option<u32>)>,
}

impl FakeState {
    fn failure(&mut self, op: FakeOp) -> Result<(), PlatformError> {
        let Some((err, left)) = self.failures.get_mut(&op) else {
            return Ok(());
        };
        let err = err.clone();
        match left {
            Some(1) => {
                self.failures.remove(&op);
            }
            Some(n) => *n -= 1,
            None => {}
        }
        Err(err)
    }

    fn live(&self, target: ProcessRef) -> Result<usize, PlatformError> {
        self.processes
            .iter()
            .position(|p| p.reference() == target)
            .ok_or_else(|| PlatformError::NotFound(format!("Process {} has exited.", target.pid)))
    }
}

/// A set of fakes sharing one state.
#[derive(Debug, Clone, Default)]
pub struct Fake {
    state: Arc<Mutex<FakeState>>,
}

impl Fake {
    /// An empty fake machine.
    pub fn new() -> Self {
        Fake::default()
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        lock(&self.state)
    }

    /// Reads or changes the shared state.
    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        f(&mut self.lock())
    }

    /// Queues a sample.
    pub fn push_sample(&self, s: SystemSample) {
        self.lock().samples.push_back(s);
    }

    /// Adds a process.
    pub fn add_process(&self, p: ProcessSample) {
        self.lock().processes.push(p);
    }

    /// Makes `op` fail with `err` on every call until [`Fake::clear_failures`].
    pub fn fail(&self, op: FakeOp, err: PlatformError) {
        self.lock().failures.insert(op, (err, None));
    }

    /// Makes `op` fail with `err` on the next `times` calls.
    pub fn fail_times(&self, op: FakeOp, err: PlatformError, times: u32) {
        if times > 0 {
            self.lock().failures.insert(op, (err, Some(times)));
        }
    }

    /// Removes every injected failure.
    pub fn clear_failures(&self) {
        self.lock().failures.clear();
    }

    /// A [`Platform`] whose ports are these fakes, in UTC (a test that cares about local days sets `zone`).
    pub fn platform(&self) -> Platform {
        Platform {
            sensors: Box::new(self.clone()),
            processes: Box::new(self.clone()),
            locks: Box::new(self.clone()),
            startup: Box::new(self.clone()),
            facts: Box::new(self.clone()),
            activity: Box::new(self.clone()),
            issues: Vec::new(),
            links: Box::new(self.clone()),
            zone: dyno_core::time::FixedZone::shared(0),
        }
    }
}

fn lock(m: &Mutex<FakeState>) -> MutexGuard<'_, FakeState> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl NetworkLinks for Fake {
    fn links(&self) -> Result<dyno_core::netchange::NetState, PlatformError> {
        self.lock()
            .net
            .clone()
            .ok_or_else(|| PlatformError::Unsupported("The fake machine has no network state.".into()))
    }
}

impl SystemSensors for Fake {
    fn sample(&mut self, now: Timestamp) -> SystemSample {
        let mut st = self.lock();
        let mut s = match st.samples.pop_front() {
            Some(s) => s,
            None => st.last_sample.clone().unwrap_or_else(|| quiet_sample(now)),
        };
        s.at = now;
        st.last_sample = Some(s.clone());
        s
    }
}

impl ProcessSource for Fake {
    fn processes(&mut self) -> Vec<ProcessSample> {
        self.lock().processes.clone()
    }

    fn details(&mut self, target: ProcessRef) -> Result<ProcessDetails, PlatformError> {
        let mut st = self.lock();
        st.failure(FakeOp::Details)?;
        let idx = st.live(target)?;
        if let Some(d) = st.details.get(&target) {
            return Ok(d.clone());
        }
        Ok(details_of(st.processes[idx].clone()))
    }
}

impl FileLocks for Fake {
    fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
        let mut st = self.lock();
        st.failure(FakeOp::Lockers)?;
        Ok(st.lockers.get(path).cloned().unwrap_or_default())
    }
}

impl StartupItems for Fake {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        let mut st = self.lock();
        st.failure(FakeOp::StartupList)?;
        Ok(st.startup.clone())
    }
}

impl FactsSource for Fake {
    fn facts(&mut self) -> SystemFacts {
        self.lock().facts.clone().unwrap_or_else(|| SystemFacts::unknown("Fake OS 1.0"))
    }
}

impl UserActivity for Fake {
    fn idle_secs(&mut self) -> Option<u64> {
        self.lock().idle_secs
    }

    fn foreground_pid(&mut self) -> Option<Pid> {
        self.lock().foreground
    }
}

/// A quiet 4-core machine with 8 GiB at `at`.
pub fn quiet_sample(at: Timestamp) -> SystemSample {
    SystemSample {
        at,
        cpu: CpuSample {
            total: 5.0,
            per_core: vec![5.0; 4],
            frequency_mhz: 2_400,
            base_frequency_mhz: 2_400,
            ..Default::default()
        },
        memory: MemorySample {
            total: 8 << 30,
            used: 4 << 30,
            available: 4 << 30,
            commit: Reading::unavailable("Fake"),
            swap_used: 0,
            swap_total: 2 << 30,
            pressure: Reading::Available(MemoryPressure::Normal),
        },
        disks: Vec::new(),
        disk_io: DiskIo {
            read_bps: 0.0,
            write_bps: 0.0,
            queue: Reading::unavailable("Fake"),
            busy: Reading::unavailable("Fake"),
        },
        network: Vec::new(),
        gpu: Reading::unavailable("Fake"),
        battery: Reading::unavailable(crate::battery::NO_BATTERY),
        temperatures: Reading::unavailable("Fake"),
        fans: Reading::unavailable("Fake"),
        uptime_secs: 60,
    }
}

/// A running process with `pid`, `name` and `cpu` (percent of the machine), started at `pid` seconds.
pub fn process(pid: Pid, name: &str, cpu: f32) -> ProcessSample {
    ProcessSample {
        pid,
        parent: None,
        name: name.into(),
        exe: Some(format!("C:\\Fake\\{name}")),
        user: Measured::Value("tester".into()),
        cpu: Measured::Value(cpu),
        memory: Measured::Value(100 << 20),
        disk_read_bps: Measured::Value(0.0),
        disk_write_bps: Measured::Value(0.0),
        started: Measured::Value(Timestamp(i64::from(pid) * 1000)),
        owner: Owner::Own,
        state: RunState::Running,
    }
}

/// Minimal details for a process.
pub fn details_of(process: ProcessSample) -> ProcessDetails {
    let none = || "Fake".to_string();
    ProcessDetails {
        command_line: vec![process.exe.clone().unwrap_or_else(|| process.name.clone())],
        process,
        cwd: None,
        priority: None,
        affinity: None,
        efficiency_mode: None,
        threads: None,
        handles: None,
        parents: Vec::new(),
        signature: SignatureInfo::Unknown(none()),
        open_files: ListOrReason::Unavailable(none()),
        connections: ListOrReason::Unavailable(none()),
        modules: ListOrReason::Unavailable(none()),
        startup_source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::startup::{ImpactRating, StartupImpact, StartupLocation};

    fn entry(id: &str, location: StartupLocation) -> StartupEntry {
        StartupEntry {
            id: id.into(),
            name: id.into(),
            command: "x.exe".into(),
            exe: Some("x.exe".into()),
            location,
            state: dyno_core::startup::StartupState::Enabled,
            publisher: None,
            impact: StartupImpact {
                cpu_secs: 0.0,
                disk_bytes: 0,
                rating: ImpactRating::NotMeasured,
            },
        }
    }

    #[test]
    fn sensors_replay_and_repeat() {
        let fake = Fake::new();
        let mut p = fake.platform();
        assert_eq!(p.sensors.sample(Timestamp(5)).at, Timestamp(5));
        let mut busy = quiet_sample(Timestamp(0));
        busy.cpu.total = 90.0;
        fake.push_sample(busy);
        assert_eq!(p.sensors.sample(Timestamp(6)).cpu.total, 90.0);
        let again = p.sensors.sample(Timestamp(7));
        assert_eq!((again.cpu.total, again.at), (90.0, Timestamp(7)));
    }

    #[test]
    fn failures_are_injectable() {
        let fake = Fake::new();
        fake.add_process(process(1, "a.exe", 50.0));
        let mut p = fake.platform();
        let a = p.processes.processes()[0].reference();
        fake.fail_times(FakeOp::Details, PlatformError::AccessDenied("denied".into()), 2);
        assert!(p.processes.details(a).is_err());
        assert!(p.processes.details(a).is_err());
        assert!(p.processes.details(a).is_ok());
        fake.fail(FakeOp::Lockers, PlatformError::Failed("x".into()));
        assert!(p.locks.lockers(Path::new("a")).is_err());
        assert!(p.locks.lockers(Path::new("a")).is_err());
        fake.clear_failures();
        assert!(p.locks.lockers(Path::new("a")).unwrap().is_empty());
        assert!(p.processes.details(a).is_ok());
    }

    #[test]
    fn startup_facts_and_activity() {
        let fake = Fake::new();
        fake.with(|s| {
            s.startup = vec![entry("u", StartupLocation::UserRunKey), entry("m", StartupLocation::MachineRunKey)];
            s.idle_secs = Some(30);
            s.foreground = Some(7);
            s.lockers.insert(PathBuf::from("f"), Vec::new());
        });
        let mut p = fake.platform();
        assert_eq!(p.startup.list().unwrap().len(), 2);
        fake.fail(FakeOp::StartupList, PlatformError::Failed("x".into()));
        assert!(p.startup.list().is_err());
        assert_eq!(p.facts.facts().os, "Fake OS 1.0");
        assert_eq!(p.activity.idle_secs(), Some(30));
        assert_eq!(p.activity.foreground_pid(), Some(7));
    }

    #[test]
    fn a_poisoned_lock_recovers() {
        let fake = Fake::new();
        let f2 = fake.clone();
        let _ = std::thread::spawn(move || f2.with(|_| panic!("boom"))).join();
        fake.add_process(process(1, "a", 1.0));
        assert_eq!(fake.with(|s| s.processes.len()), 1);
    }
}
