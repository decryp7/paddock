//! The load gate: wait while the 1-minute load average is at or above the threshold. Unix reads the load average.
//! Windows has no load average (sysinfo reports zeros); a CPU-queue counter needs PDH and is not worth the weight here,
//! so the gate is skipped there with a note (the lock still serialises heavy steps).

use std::sync::Mutex;

/// The 1-minute load, or None when this platform has none.
pub trait LoadSource: Send + Sync {
    /// The 1-, 5- and 15-minute load averages.
    fn load(&self) -> Option<[f64; 3]>;
    /// The share of CPU time that was idle since the last call, 0 to 100; `None` when this platform or this first call cannot say.
    fn idle_percent(&self) -> Option<f64> {
        None
    }
    /// Memory a new step could use now, in MiB: free plus what the system can reclaim (sysinfo's `available_memory`); `None` when the
    /// platform cannot say. Unknown is never read as "plenty" (ER-24, ER-33).
    fn available_memory_mb(&self) -> Option<u64> {
        None
    }
}

/// The operating system's load average and CPU idle share. The idle share is measured between two readings at least
/// `MINIMUM_CPU_UPDATE_INTERVAL` apart (a shorter span gives a meaningless 0 % use, which would read as "idle"): the first reading is taken when
/// this is made (primed), and until a reading older than the interval exists the answer is `None`, never a guess.
#[derive(Debug)]
pub struct SysLoad {
    cpu: Mutex<Cpu>,
}

#[derive(Debug)]
struct Cpu {
    sys: Option<sysinfo::System>,
    at: std::time::Instant,
    idle: Option<f64>,
}

impl Default for SysLoad {
    fn default() -> SysLoad {
        let sys = (!cfg!(windows)).then(|| sysinfo::System::new().ok()).flatten().map(|mut s| {
            s.refresh_cpu_usage();
            s
        });
        SysLoad {
            cpu: Mutex::new(Cpu {
                sys,
                at: std::time::Instant::now(),
                idle: None,
            }),
        }
    }
}

impl LoadSource for SysLoad {
    fn load(&self) -> Option<[f64; 3]> {
        if cfg!(windows) {
            return None;
        }
        // Since sysinfo 0.40 the read can fail; an unknown load is `None`, like on Windows.
        let l = sysinfo::System::load_average().ok()?;
        Some([l.one, l.five, l.fifteen])
    }

    fn idle_percent(&self) -> Option<f64> {
        let mut guard = self.cpu.lock().ok()?;
        let Cpu { sys, at, idle } = &mut *guard;
        let sys = sys.as_mut()?;
        if at.elapsed() >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            sys.refresh_cpu_usage();
            *idle = Some((100.0 - f64::from(sys.global_cpu_usage())).clamp(0.0, 100.0));
            *at = std::time::Instant::now();
        }
        *idle
    }

    fn available_memory_mb(&self) -> Option<u64> {
        let mut sys = sysinfo::System::new().ok()?;
        sys.refresh_memory();
        let bytes = sys.available_memory();
        // sysinfo answers 0 when it cannot read the figure: that is "unknown", not "no memory left".
        (bytes > 0).then_some(bytes / (1024 * 1024))
    }
}

/// A load made by hand, for tests.
#[derive(Debug)]
pub struct FakeLoad(Mutex<Option<[f64; 3]>>, Mutex<Option<f64>>, Mutex<Option<u64>>);

impl FakeLoad {
    /// A load of `one` (the other averages equal it), 100 % idle and 64 GiB of memory available.
    pub fn new(one: f64) -> FakeLoad {
        FakeLoad(
            Mutex::new(Some([one, one, one])),
            Mutex::new(Some(100.0)),
            Mutex::new(Some(64 * 1024)),
        )
    }
    /// Changes the memory available, in MiB (None: unknown).
    pub fn set_memory_mb(&self, mb: Option<u64>) {
        if let Ok(mut m) = self.2.lock() {
            *m = mb;
        }
    }
    /// Changes the idle share (None: unknown).
    pub fn set_idle(&self, idle: Option<f64>) {
        if let Ok(mut m) = self.1.lock() {
            *m = idle;
        }
    }
    /// Changes the load.
    pub fn set(&self, one: f64) {
        if let Ok(mut m) = self.0.lock() {
            *m = Some([one, one, one]);
        }
    }
    /// A platform without a load average.
    pub fn none() -> FakeLoad {
        FakeLoad(Mutex::new(None), Mutex::new(None), Mutex::new(None))
    }
}

impl LoadSource for FakeLoad {
    fn load(&self) -> Option<[f64; 3]> {
        self.0.lock().ok().and_then(|m| *m)
    }
    fn idle_percent(&self) -> Option<f64> {
        self.1.lock().ok().and_then(|m| *m)
    }
    fn available_memory_mb(&self) -> Option<u64> {
        self.2.lock().ok().and_then(|m| *m)
    }
}

/// What the machine looks like to the slot rule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pressure {
    /// The 1-minute load average: the fast brake (a burst that the 15-minute average has not seen yet).
    pub load1: Option<f64>,
    /// The 15-minute load average, when the platform has one.
    pub load15: Option<f64>,
    /// The CPU idle share since the last reading, 0 to 100, when known.
    pub idle: Option<f64>,
    /// Memory available (free plus reclaimable) in MiB, when known.
    pub memory_mb: Option<u64>,
}

/// Cores one heavy step is counted to use (`CARGO_BUILD_JOBS=2` is the family's cap for a builder, ER-24).
pub const CORES_PER_SLOT: f64 = 2.0;
/// Below this CPU idle share no second step starts.
pub const MIN_IDLE_PERCENT: f64 = 30.0;
/// Memory one heavy step is counted to use, in MiB: a builder at `CARGO_BUILD_JOBS=2` (rustc with LTO off, a linker, a test binary).
/// The one place this is decided; the admission rule and its tests read it from here (ER-22).
pub const MEMORY_PER_SLOT_MB: u64 = 3072;
/// Memory that must stay available after a step is admitted, in MiB. On 2026-11-22 free memory fell to about 1 GB with 16 GB compressed
/// while load and CPU still looked acceptable (ER-24): twice that is kept back for the rest of the machine.
pub const MEMORY_RESERVE_MB: u64 = 2048;
/// The most rows expected to be quick (see [`CHEAP_SECS`]) that run beside the full slots at once.
pub const CHEAP_LANE: u32 = 2;
/// A row whose measured run time is under this many seconds goes first: it does not wait behind long steps.
pub const CHEAP_SECS: f64 = 10.0;
/// A quick row is admitted beside the full slots only while the CPU has at least this idle share: it adds `CORES_PER_SLOT` cores for
/// seconds, so the 15-minute load is no reason to hold it, but a CPU with no idle left is.
pub const CHEAP_MIN_IDLE_PERCENT: f64 = 15.0;

/// How many heavy steps may run together now, between 1 and `max`: the cores the 15-minute load leaves free, `CORES_PER_SLOT` each,
/// and only while the CPU has at least `MIN_IDLE_PERCENT` idle and the 1-minute load is under the core count. The count is of ALL
/// steps running, ours included: their own load is already in the load average (late, as it is a 15-minute one, which is why
/// the 1-minute brake and the maximum bound a burst), so the rule is the stricter of the two readings of the same thing, never a
/// sum. `make -l N` and `ninja -l N` are the same shape (a count of jobs and a load brake); they use the 1-minute load, which reacts
/// at once and flaps, where this uses the 15-minute one to open and the 1-minute one only to close. One while a container run is going (or is the request: it runs
/// alone), and one when the load or the idle share is unknown: nothing is guessed in favour of running more (ER-24, ER-33).
/// Pure: numbers in, a count out.
pub fn allowed_slots(max: u32, cpus: u32, p: &Pressure, container: bool) -> u32 {
    if max <= 1 || container {
        return 1;
    }
    let (Some(load15), Some(idle), Some(memory)) = (p.load15, p.idle, p.memory_mb) else {
        return 1;
    };
    if idle < MIN_IDLE_PERCENT || p.load1.is_some_and(|l1| l1 >= f64::from(cpus)) {
        return 1;
    }
    let free = (f64::from(cpus) - load15) / CORES_PER_SLOT;
    if !free.is_finite() || free < 1.0 {
        return 1;
    }
    // The memory left after the reserve, `MEMORY_PER_SLOT_MB` a step: the second step is refused when it would not fit.
    let by_memory = memory.saturating_sub(MEMORY_RESERVE_MB) / MEMORY_PER_SLOT_MB;
    let by_memory = u32::try_from(by_memory).unwrap_or(u32::MAX);
    (free.floor() as u32).min(by_memory).clamp(1, max)
}

/// Whether a quick row (expected under [`CHEAP_SECS`]) may start beside the full slots now: the machine has the idle CPU and the memory
/// for a step more (the reserve stays), the 1-minute load is under twice the cores, and no container run is going. The 15-minute load
/// does not decide it. Unknown readings say no (ER-33). Pure: numbers in, a verdict out.
pub fn cheap_admitted(cpus: u32, p: &Pressure, container: bool) -> bool {
    if container {
        return false;
    }
    let (Some(load1), Some(idle), Some(memory)) = (p.load1, p.idle, p.memory_mb) else {
        return false;
    };
    idle >= CHEAP_MIN_IDLE_PERCENT && load1 < 2.0 * f64::from(cpus) && memory >= MEMORY_RESERVE_MB + MEMORY_PER_SLOT_MB
}

/// `CARGO_BUILD_JOBS` for one step (ER-48): `CORES_PER_SLOT`, fewer when the most steps at once times it would pass the cores, so
/// jobs per step times steps at once stays within the host's cores.
pub fn jobs_per_step(cpus: u32, max_slots: u32) -> u32 {
    (cpus / max_slots.max(1)).clamp(1, CORES_PER_SLOT as u32)
}

/// The test-worker variables a step gets its share in (ER-48): Playwright's (`e2eWorkers` of @decryp7/paddock-e2e) and Vitest's; the
/// names `./dev ci build-jobs` sets for a CI job (tools/paddock-dev/src/ci_build_jobs.rs).
pub const WORKER_VARS: [&str; 2] = ["PADDOCK_E2E_WORKERS", "VITEST_MAX_WORKERS"];

/// Test workers (browsers, test processes) for one step: the step's share of the cores, `cpus / max_slots`, at least one, so workers
/// per step times steps at once stays within the cores (ER-48).
pub fn workers_per_step(cpus: u32, max_slots: u32) -> u32 {
    (cpus / max_slots.max(1)).max(1)
}

/// The default maximum of the "Auto" setting: about half the cores, at most 5 builders (ER-24).
pub fn auto_max_slots(cpus: u32) -> u32 {
    (cpus / 2).clamp(1, 5)
}

/// Whether a command line is a container run (`docker`, `podman`, `nerdctl`, or the family's `linux-container.sh`): it counts as a
/// builder of its own and runs alone, because the VM behind it takes the memory and cores that `heavy` cannot see.
pub fn is_container_command(cmdline: &str) -> bool {
    cmdline.split_whitespace().any(|t| {
        let base = t.rsplit('/').next().unwrap_or(t);
        matches!(base, "docker" | "podman" | "nerdctl") || base == "linux-container.sh"
    })
}

/// `1.23 4.56 7.89`.
pub fn load_text(l: [f64; 3]) -> String {
    format!("{:.2} {:.2} {:.2}", l[0], l[1], l[2])
}
