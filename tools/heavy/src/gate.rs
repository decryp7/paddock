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
    /// The 1-minute load average: the quick lane's brake.
    pub load1: Option<f64>,
    /// The 5-minute load average.
    pub load5: Option<f64>,
    /// The 15-minute load average, when the platform has one (logged only).
    pub load15: Option<f64>,
    /// The CPU idle share since the last reading, 0 to 100, when known.
    pub idle: Option<f64>,
    /// Memory available (free plus reclaimable) in MiB, when known.
    pub memory_mb: Option<u64>,
}

/// Build jobs one heavy step gets at most (`CARGO_BUILD_JOBS=2` is the family's cap for a builder, ER-24; the push gate sets the same).
pub const CORES_PER_SLOT: f64 = 2.0;
/// The most steps the "Auto" setting lets run at once, and the most any setting does. Two, not three: on the 14-core Mac three push-gate
/// test rows at once (4 test threads each) finished fewer per hour than two (7 each), docs/benchmarks.md, "Concurrent push-gate test rows
/// on the shared Mac" (2026-11-25).
pub const MAX_AUTO_SLOTS: u32 = 2;
/// The 5-minute load, as a share of the cores, under which more than one step may run: entry 0 is the limit for 2 steps at once. 0.85
/// (11.9 on 14 cores): every batch of that measurement started under a 5-minute load of 12, and two at once gave 22 % more rows per hour
/// than one there; above it there is no measurement that a second step helps.
pub const SLOT_LOAD_SHARE: [f64; MAX_AUTO_SLOTS as usize - 1] = [0.85];
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

/// The 5-minute load under which `n` steps may run at once on `cpus` cores; `None` for one (always allowed) and above
/// [`MAX_AUTO_SLOTS`].
pub fn slot_load_limit(cpus: u32, n: u32) -> Option<f64> {
    let i = usize::try_from(n.checked_sub(2)?).ok()?;
    SLOT_LOAD_SHARE.get(i).map(|share| share * f64::from(cpus))
}

/// How many heavy steps may run together now, between 1 and `max` (and never above [`MAX_AUTO_SLOTS`]): the most `n` whose
/// [`slot_load_limit`] the **5-minute load** is under, and as many as the memory available holds. The count is of ALL steps running,
/// ours included: their own load is in the average, which is why the limits come from a measurement of steps running together (user
/// decision 2026-11-25: several slots when the machine is idle enough, one otherwise). Every step admitted while `n` may run gets `1/n`
/// of the cores ([`step_threads`]), so a second or third step does not add threads beyond the cores (ER-48). One while a container run
/// is going (or is the request: it runs alone), and one when the load or the memory is unknown: nothing is guessed in favour of running
/// more (ER-24, ER-33). Pure: numbers in, a count out.
pub fn allowed_slots(max: u32, cpus: u32, p: &Pressure, container: bool) -> u32 {
    if max <= 1 || container {
        return 1;
    }
    let (Some(load5), Some(memory)) = (p.load5, p.memory_mb) else {
        return 1;
    };
    if !load5.is_finite() {
        return 1;
    }
    let by_load = (2..=MAX_AUTO_SLOTS)
        .rev()
        .find(|n| slot_load_limit(cpus, *n).is_some_and(|limit| load5 < limit))
        .unwrap_or(1);
    // The memory left after the reserve, `MEMORY_PER_SLOT_MB` a step: the second step is refused when it would not fit.
    let by_memory = memory.saturating_sub(MEMORY_RESERVE_MB) / MEMORY_PER_SLOT_MB;
    let by_memory = u32::try_from(by_memory).unwrap_or(u32::MAX);
    by_load.min(by_memory).clamp(1, max)
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

/// The default maximum of the "Auto" setting: a step per 4 cores, at most [`MAX_AUTO_SLOTS`] (ER-24).
pub fn auto_max_slots(cpus: u32) -> u32 {
    (cpus / 4).clamp(1, MAX_AUTO_SLOTS)
}

/// The threads one step may use (ER-48).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepThreads {
    /// `CARGO_BUILD_JOBS`.
    pub build_jobs: u32,
    /// `NEXTEST_TEST_THREADS` and `RUST_TEST_THREADS`.
    pub test_threads: u32,
}

/// The threads of a step admitted while `share` steps may run at once: `cores / share` test threads (all of them for a step alone, as
/// before), and that many build jobs up to the family's cap of `CORES_PER_SLOT`. Steps at once times threads stay within the cores.
pub fn step_threads(cpus: u32, share: u32) -> StepThreads {
    let per = (cpus / share.max(1)).max(1);
    StepThreads {
        build_jobs: per.min(CORES_PER_SLOT as u32),
        test_threads: per,
    }
}

/// The variables heavy gives the command for its share of the cores, leaving out any the caller set itself (its value wins, ER-47).
pub fn step_env(cpus: u32, share: u32, get: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String)> {
    let t = step_threads(cpus, share);
    [
        ("CARGO_BUILD_JOBS", t.build_jobs),
        ("NEXTEST_TEST_THREADS", t.test_threads),
        ("RUST_TEST_THREADS", t.test_threads),
    ]
    .into_iter()
    .filter(|(k, _)| get(k).is_none())
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
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
