//! Runs benchmarks on the user's click, one kind at a time, each in its own `dyno-bench` process, and keeps the run history
//! (ADR 0030). Nothing here starts on its own: only [`BenchCentre::start`] (from the `StartBenchmark` request) begins a run.
//!
//! A run is a job (ER-17, ER-18): `start` answers at once, the work runs on its own thread, the child's phases become the
//! job's phases with their own scale, and Cancel works in every phase (the person's Cancel on the page or in the Jobs view).
//! Nothing ends a slow or quiet child by itself (ER-19): the run shows "Still working" or "Not responding" and the person
//! decides; the only automatic end is a named safety stop (the computer switched to battery without consent). The scratch
//! folder of the disk test is created here, so it is removed here whatever the child did, including when it was killed.
//!
//! Priority: the measuring child is the one bulk-sized job that runs at normal priority, an explicit, narrow exception to rule 52
//! (ADR 0030 section 2); the marker is in [`crate::bench_child`], where the child is started. This thread only orchestrates and
//! stays at its normal role.

use crate::agent::{lock, now};
use crate::bench_child::{self, ChildEvents, ChildProcess, ChildRequest, ProcessChild, Stop, UnitControl, UnitHandle};
use crate::bench_scratch;
use crate::files;
use dyno_core::bench::unit::{self, UnitCommand, UnitRecord};
use dyno_core::bench::wire::{
    battery_preflight, plan_bound_full, stop_for_battery, Liveness, PhaseProgress, Preflight, BATTERY_STOP_REASON,
};
use dyno_core::bench::{
    BenchHistory, BenchKind, BenchMetric, BenchOverview, BenchPhase, BenchProgress, BenchRun, BenchStatus, KindResult, LoadAtRun, RunPlan,
    HISTORY_VERSION,
};
use dyno_core::protocol::{AgentError, ErrorCode};
use paddock_bench::{Clock, SystemClock};
use paddock_jobs::{CancelReason, JobContext, JobError, Phase};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::bench_disk_target as disk_target;

/// The executables the agent starts by name for benchmarks, declared once: each must be in `bundle.externalBin`
/// (`scripts/check-shipped.mjs` reads this list and fails when one is not bundled, ADR 0030 B0).
pub const RUNNER_NAMES: &[&str] = &["dyno-bench", "dyno-bench-gpu"];

/// What the person sees when the runner is not in the installation (an older install, or a damaged one).
pub const RUNNER_MISSING: &str =
    "Benchmarks need the dyno-bench program, which this installation does not include. Install Dyno 1.1 or later, or run the installer again.";

/// What the person is asked when a run starts on battery.
pub const BATTERY_CONSENT: &str = "On battery. Results will be lower and the battery drains faster. Run anyway?";

/// The power source is looked at every this many ticks of the child loop (about once a second).
const POWER_EVERY_TICKS: u32 = 4;

/// Runs one kind somewhere else. A port so tests need no real process.
pub trait BenchLauncher: Send + Sync {
    /// Measures `req.kind`, reporting through `events` and ending the child when `events.stop()` says so. Never panics; every
    /// failure is a [`BenchStatus`].
    fn run(&self, req: &ChildRequest, events: &dyn ChildEvents) -> BenchStatus;

    /// Cleans up what an earlier agent left behind (Linux: units it started). Called once when the history is loaded.
    fn cleanup_leftovers(&self) {}

    /// The graphics adapters the GPU runner lists, for the picker.
    fn gpu_adapters(&self) -> Result<Vec<dyno_core::bench::gpu::GpuAdapter>, String> {
        Err("This agent has no graphics runner.".into())
    }

    /// Set when this launcher cannot give the full-confidence conditions (Linux without a user manager): recorded with the run.
    fn limited(&self) -> Option<String> {
        None
    }
}

/// What the centre needs from the agent: a job to report in and the power source. A port so tests need neither.
pub trait BenchEnv: Send + Sync {
    /// Registers the run as a job (shown in the Jobs view); `None` where there is no job mechanism.
    fn register_job(&self, title: &str, subject: Option<&str>) -> Option<JobContext>;
    /// Whether the machine runs on battery now (`None` when unknown or no battery).
    fn on_battery(&self) -> Option<bool>;
    /// The volumes mounted now (the disk map's list); empty where the agent has none.
    fn volumes(&self) -> Vec<dyno_core::disk::volume::Volume> {
        Vec::new()
    }
    /// Memory available to start new work now, bytes (`None` when the agent has no reading).
    fn available_memory(&self) -> Option<u64> {
        None
    }
    /// The latest reading of clock, temperature and power for the throttle judgement (`None` when the agent has none).
    fn throttle_input(&self) -> Option<dyno_core::throttle::ThrottleInput> {
        None
    }
    /// Capacity and free bytes of the volume mounted at `mount` as of now (`None` when the OS will not say).
    fn space(&self, _mount: &str) -> Option<(u64, u64)> {
        None
    }
}

struct NoEnv;

impl BenchEnv for NoEnv {
    fn register_job(&self, _: &str, _: Option<&str>) -> Option<JobContext> {
        None
    }

    fn on_battery(&self) -> Option<bool> {
        None
    }
}

/// Runs a unit command: whether it succeeded, its stdout and its stderr; or why it could not run at all.
fn run_unit_raw(c: &UnitCommand, timeout: Duration) -> Result<(bool, String, String), String> {
    let mut cmd = dyno_platform::process::command(c.program);
    cmd.args(&c.args);
    match dyno_platform::process::run_with_timeout(cmd, timeout) {
        Ok(out) => Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        )),
        Err(e) => Err(format!("`{}` could not run: {e}", c.display())),
    }
}

/// Runs a unit command and returns its stdout, or why it failed.
fn run_unit_command(c: &UnitCommand, timeout: Duration) -> Result<String, String> {
    match run_unit_raw(c, timeout)? {
        (true, out, _) => Ok(out),
        (false, _, err) => Err(format!("`{}` failed: {err}", c.display())),
    }
}

/// The user manager through `systemctl --user`.
pub struct SystemctlControl;

impl SystemctlControl {
    /// Whether a user manager answers (Linux only; false elsewhere).
    pub fn available() -> bool {
        // `is-system-running` exits non-zero for `degraded`, so its text decides, not the exit code.
        cfg!(target_os = "linux")
            && run_unit_raw(&unit::manager_state_command(), Duration::from_secs(3)).is_ok_and(|(_, out, _)| unit::manager_usable(&out))
    }

    /// The InvocationID of a unit now.
    pub fn invocation_id(&self, unit_name: &str) -> Option<String> {
        run_unit_command(&unit::invocation_command(unit_name), Duration::from_secs(3))
            .ok()
            .and_then(|o| unit::parse_invocation_id(&o))
    }
}

impl UnitControl for SystemctlControl {
    fn live(&self) -> Vec<unit::LiveUnit> {
        let Ok(listed) = run_unit_command(&unit::list_command(), Duration::from_secs(5)) else {
            return Vec::new();
        };
        unit::parse_unit_list(&listed)
            .into_iter()
            .map(|u| {
                let invocation_id = self.invocation_id(&u);
                unit::LiveUnit { unit: u, invocation_id }
            })
            .collect()
    }

    fn stop(&self, unit_name: &str) {
        match run_unit_command(&unit::stop_command(unit_name), Duration::from_secs(10)) {
            Ok(_) => log::info!("dyno-agent: stopped the benchmark unit {unit_name}"),
            Err(e) => log::warn!("dyno-agent: could not stop the benchmark unit {unit_name}: {e}"),
        }
    }
}

/// Stops a run's unit when its child is ended (Cancel, "Not responding" Cancel, a safety stop, the agent's shutdown) and
/// forgets its record.
struct UnitStopper {
    unit: String,
    records: PathBuf,
}

impl UnitHandle for UnitStopper {
    fn stop(&mut self) {
        SystemctlControl.stop(&self.unit);
        forget_record(&self.records, &self.unit);
    }
}

fn remember_record(file: &Path, record: UnitRecord) {
    let mut all = bench_child::read_records(file);
    all.retain(|r| r.unit != record.unit);
    all.push(record);
    if let Err(e) = bench_child::write_records(file, &all) {
        log::warn!("dyno-agent: could not record the benchmark unit: {e}");
    }
}

fn forget_record(file: &Path, unit_name: &str) {
    let mut all = bench_child::read_records(file);
    all.retain(|r| r.unit != unit_name);
    let _ = bench_child::write_records(file, &all);
}

/// Whether the agent runs the benchmark child as a systemd user unit where a user manager exists. On since the B2 spike passed on a
/// real systemd (ADR 0030 D5; results in `docs/linux.md`); `tests/bench_units.rs` is the check and must stay green.
pub const LINUX_UNITS_VERIFIED: bool = true;

/// How long the agent waits for a started unit to show an InvocationID.
const INVOCATION_WAIT: Duration = Duration::from_secs(3);
const INVOCATION_STEP: Duration = Duration::from_millis(100);

/// What waiting for the InvocationID ended with.
#[derive(Debug, PartialEq, Eq)]
enum InvocationWait {
    Found(String),
    TimedOut,
    Stopped(Stop),
}

/// Looks for the unit's InvocationID until `budget` has passed on `clock`, asking `events` for a stop before each look, so Cancel
/// works while it waits. `pause` waits one step (the real one sleeps, the tests move a manual clock).
fn wait_for_invocation(
    find: &mut dyn FnMut() -> Option<String>,
    events: &dyn ChildEvents,
    clock: &dyn Clock,
    pause: &dyn Fn(Duration),
    budget: Duration,
) -> InvocationWait {
    let start = clock.now();
    loop {
        if let Some(stop) = events.stop() {
            return InvocationWait::Stopped(stop);
        }
        if let Some(id) = find() {
            return InvocationWait::Found(id);
        }
        if clock.now().saturating_sub(start) >= budget {
            return InvocationWait::TimedOut;
        }
        pause(INVOCATION_STEP);
    }
}

/// Starts the `dyno-bench` executable that sits next to the agent.
pub struct ProcessLauncher {
    exe: PathBuf,
    /// Linux with a user manager: the child runs as a named unit.
    units: bool,
    /// Where the units' InvocationIDs are written down.
    records: PathBuf,
}

impl ProcessLauncher {
    /// The launcher for the executable beside the running agent; `history` is the benchmark history file, beside which the unit
    /// records are kept.
    pub fn beside_agent(history: &Path) -> ProcessLauncher {
        let name = if cfg!(windows) {
            format!("{}.exe", RUNNER_NAMES[0])
        } else {
            RUNNER_NAMES[0].to_string()
        };
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let units = LINUX_UNITS_VERIFIED && SystemctlControl::available();
        if cfg!(target_os = "linux") {
            log::info!(
                "dyno-agent: benchmarks on Linux {}",
                if units {
                    "run as named systemd user units"
                } else {
                    "run inside the agent's cgroup (no user manager): limited"
                }
            );
        }
        ProcessLauncher {
            exe: dir.join(name),
            units,
            records: bench_child::units_file(history),
        }
    }

    /// A launcher that runs the child as a unit (needs a systemd user manager; the Linux check uses it).
    pub fn with_units(exe: impl Into<PathBuf>, records: impl Into<PathBuf>) -> ProcessLauncher {
        ProcessLauncher {
            exe: exe.into(),
            units: true,
            records: records.into(),
        }
    }

    /// A launcher for this executable, without units (tests and tools).
    pub fn at(exe: impl Into<PathBuf>) -> ProcessLauncher {
        ProcessLauncher {
            exe: exe.into(),
            units: false,
            records: PathBuf::new(),
        }
    }

    /// The program that measures `kind`: `dyno-bench`, or its sibling `dyno-bench-gpu` for graphics.
    fn exe_for(&self, kind: BenchKind) -> PathBuf {
        if kind != BenchKind::Gpu {
            return self.exe.clone();
        }
        let name = if cfg!(windows) {
            format!("{}.exe", RUNNER_NAMES[1])
        } else {
            RUNNER_NAMES[1].to_string()
        };
        self.exe.with_file_name(name)
    }

    /// The command line for a kind (the runner's own arguments).
    fn runner_args(req: &ChildRequest) -> Vec<String> {
        // The graphics runner is its own program and takes no kind (ADR 0030 D2); the default adapter is the discrete one.
        if req.kind == BenchKind::Gpu {
            return req.gpu_adapter.map_or_else(Vec::new, |i| vec!["--adapter".into(), i.to_string()]);
        }
        let mut args = vec![req.kind.name().to_string()];
        if req.kind == BenchKind::Disk {
            args.extend([
                "--dir".into(),
                req.scratch.to_string_lossy().into_owned(),
                "--disk-mib".into(),
                req.disk_mib.to_string(),
            ]);
            if req.rotational {
                args.push("--rotational".into());
            }
        }
        if let (BenchKind::Memory, Some(mib)) = (req.kind, req.available_mib) {
            args.extend(["--avail-mib".into(), mib.to_string()]);
        }
        if req.kind == BenchKind::Disk && req.sustained_write_mib > 0 {
            args.extend(["--sustained-write-mib".into(), req.sustained_write_mib.to_string()]);
        }
        if let (BenchKind::Cpu, Some(secs)) = (req.kind, req.sustained_secs) {
            args.extend(["--sustained-secs".into(), secs.to_string()]);
        }
        args
    }
}

impl BenchLauncher for ProcessLauncher {
    fn gpu_adapters(&self) -> Result<Vec<dyno_core::bench::gpu::GpuAdapter>, String> {
        let exe = self.exe_for(BenchKind::Gpu);
        if !exe.is_file() {
            return Err(RUNNER_MISSING.into());
        }
        let mut cmd = dyno_platform::process::command(&exe);
        cmd.arg("--list");
        let out = dyno_platform::process::run_with_timeout(cmd, Duration::from_secs(20))
            .map_err(|e| format!("The graphics runner did not answer ({e})."))?;
        serde_json::from_slice(&out.stdout).map_err(|e| format!("The graphics runner's list could not be read ({e})."))
    }

    fn run(&self, req: &ChildRequest, events: &dyn ChildEvents) -> BenchStatus {
        let exe = self.exe_for(req.kind);
        if !exe.is_file() {
            log::warn!("dyno-agent: benchmark {}: the runner is not in the installation", req.kind.name());
            return BenchStatus::Unavailable {
                reason: RUNNER_MISSING.into(),
            };
        }
        let mut args = ProcessLauncher::runner_args(req);
        let unit_name = self.units.then(|| unit::unit_name(&format!("{}-{}", req.run_id, req.kind.name())));
        let (cmd, shown) = match &unit_name {
            Some(name) => {
                args.push("--wait-go".into());
                let c = unit::run_command(name, &exe, &args);
                let mut cmd = dyno_platform::process::command(c.program);
                cmd.args(&c.args);
                (cmd, c.display())
            }
            None => {
                let mut cmd = dyno_platform::process::command(&exe);
                cmd.args(&args);
                (
                    cmd,
                    format!(
                        "{} {}",
                        exe.file_name().map_or_else(|| RUNNER_NAMES[0].into(), |n| n.to_string_lossy()),
                        args.join(" ")
                    ),
                )
            }
        };
        // Neither the program's folder nor the scratch folder goes into the log (they may be under the user's home).
        let shown = shown
            .replace(
                exe.to_string_lossy().as_ref(),
                RUNNER_NAMES[usize::from(req.kind == BenchKind::Gpu)],
            )
            .replace(req.scratch.to_string_lossy().as_ref(), "<scratch>");
        log::info!("dyno-agent: run {} {}: starting `{shown}`", req.run_id, req.kind.name());
        let handle = unit_name.as_ref().map(|name| {
            Box::new(UnitStopper {
                unit: name.clone(),
                records: self.records.clone(),
            }) as Box<dyn UnitHandle>
        });
        let t = Instant::now();
        let mut child = match ProcessChild::spawn(cmd, handle) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("dyno-agent: run {} {}: the runner did not start: {e}", req.run_id, req.kind.name());
                return BenchStatus::Failed {
                    reason: format!("Dyno could not start the benchmark runner ({e})."),
                };
            }
        };
        log::info!("dyno-agent: run {} {}: pid {:?}", req.run_id, req.kind.name(), child.pid());
        if let Some(name) = &unit_name {
            // The unit's InvocationID is written down before the child measures: it waits for `go`. Cancel works meanwhile.
            let clock = SystemClock::new();
            let waited = wait_for_invocation(
                &mut || SystemctlControl.invocation_id(name),
                events,
                &clock,
                &std::thread::sleep,
                INVOCATION_WAIT,
            );
            match waited {
                InvocationWait::Found(invocation_id) => remember_record(
                    &self.records,
                    UnitRecord {
                        unit: name.clone(),
                        invocation_id,
                    },
                ),
                InvocationWait::TimedOut => log::warn!(
                    "dyno-agent: run {}: the unit {name} has no readable InvocationID; a crash would not be cleaned up",
                    req.run_id
                ),
                InvocationWait::Stopped(stop) => {
                    child.end();
                    forget_record(&self.records, name);
                    return bench_child::stopped(&stop, req.kind);
                }
            }
            if let Err(e) = child.send_go() {
                log::warn!("dyno-agent: run {}: could not tell the runner to go: {e}", req.run_id);
            }
        }
        let status = bench_child::drive(
            &mut child,
            events,
            &SystemClock::new(),
            plan_bound_full(
                &[req.kind],
                req.disk_mib,
                1,
                req.sustained_secs,
                req.sustained_write_mib.div_ceil(1024),
            ),
            req.kind,
        );
        if let Some(name) = &unit_name {
            forget_record(&self.records, name);
        }
        log::info!(
            "dyno-agent: run {} {}: finished in {} ms",
            req.run_id,
            req.kind.name(),
            t.elapsed().as_millis()
        );
        status
    }

    fn cleanup_leftovers(&self) {
        if self.units {
            let d = bench_child::cleanup_leftovers(&SystemctlControl, &self.records);
            log::info!(
                "dyno-agent: benchmark unit cleanup: stopped {}, left alone {}",
                d.stop.len(),
                d.left_alone.len()
            );
        }
    }

    fn limited(&self) -> Option<String> {
        (cfg!(target_os = "linux") && !self.units).then(|| unit::LIMITED_NOTE.to_string())
    }
}

struct State {
    history: BenchHistory,
    running: Option<BenchProgress>,
    counter: u64,
}

struct Inner {
    state: Mutex<State>,
    /// Signalled after every change of the state, so waiters (tests, shutdown) wait on the condition and not on time.
    changed: std::sync::Condvar,
    cancel: AtomicBool,
    launcher: Box<dyn BenchLauncher>,
    env: Mutex<Arc<dyn BenchEnv>>,
    file: PathBuf,
    scratch_root: PathBuf,
}

/// The benchmark runs: history on disk, at most one run at a time.
#[derive(Clone)]
pub struct BenchCentre {
    inner: Arc<Inner>,
}

/// How a run's kinds ended.
#[derive(Default)]
struct RunOutcome {
    results: Vec<KindResult>,
    /// Stopped before the end (Cancel or a safety stop).
    cancelled: bool,
    /// The words of the safety stop that ended it, if one did.
    safety: Option<String>,
}

/// What the child loop asks of the run, for one kind: progress to record, liveness to show, and when to stop.
struct RunEvents {
    centre: BenchCentre,
    kind: BenchKind,
    job: Option<JobContext>,
    consent: bool,
    /// The job phase last begun (a child phase id), so a new one starts a new scale.
    phase_id: Mutex<String>,
    ticks: AtomicU32,
    /// The last power reading, kept between the looks.
    on_battery: Mutex<Option<bool>>,
    /// Why a safety stop ended the run, once one did.
    safety: Mutex<Option<String>>,
    /// The sustained run's points, with what the agent read beside each.
    trace: Mutex<Vec<dyno_core::bench::cpu::TracePoint>>,
    /// Heat watch and the same throttle judgement as monitoring (ADR 0018), fed once a second during a sustained run.
    heat: Mutex<dyno_core::bench::cpu::HeatWatch>,
    tracker: Mutex<dyno_core::throttle::ThrottleTracker>,
    /// The episodes the tracker closed or still has open.
    episodes: Mutex<Vec<dyno_core::throttle::ThrottleEpisode>>,
    /// What the child said about how it measured.
    notes: Mutex<Vec<String>>,
    /// A heat stop decided by a sample, delivered by the next `stop`.
    heat_stop: Mutex<Option<String>>,
}

impl RunEvents {
    fn new(centre: BenchCentre, kind: BenchKind, job: Option<JobContext>, consent: bool) -> RunEvents {
        RunEvents {
            centre,
            kind,
            job,
            consent,
            phase_id: Mutex::new(String::new()),
            ticks: AtomicU32::new(0),
            on_battery: Mutex::new(None),
            safety: Mutex::new(None),
            trace: Mutex::new(Vec::new()),
            heat: Mutex::new(dyno_core::bench::cpu::HeatWatch::new()),
            tracker: Mutex::new(dyno_core::throttle::ThrottleTracker::new()),
            episodes: Mutex::new(Vec::new()),
            heat_stop: Mutex::new(None),
            notes: Mutex::new(Vec::new()),
        }
    }

    /// The trace so far, emptied.
    fn take_trace(&self) -> Vec<dyno_core::bench::cpu::TracePoint> {
        std::mem::take(&mut lock(&self.trace))
    }

    /// Adds what the throttle tracker found to a finished sustained run: the number of episodes, and a word per episode in the log.
    fn finish_sustained(&self, status: BenchStatus) -> BenchStatus {
        let BenchStatus::Done { mut metrics } = status else { return status };
        let mut episodes = std::mem::take(&mut *lock(&self.episodes));
        if let Some(open) = lock(&self.tracker).current() {
            episodes.retain(|e| e.id != open.id);
            episodes.push(open);
        }
        for e in &episodes {
            log::info!(
                "dyno-agent: sustained run: throttling from {} for {} s, cause {}, lost {:.0}% on average",
                e.start.millis(),
                e.duration_ms(e.end.unwrap_or(e.start)) / 1000,
                e.cause.label(),
                e.avg_lost_percent
            );
        }
        metrics.push(BenchMetric::lower(
            "cpu-sustained-episodes",
            "Throttling episodes",
            episodes.len() as f64,
            "episodes",
        ));
        BenchStatus::Done { metrics }
    }
}

impl ChildEvents for RunEvents {
    fn progress(&self, p: &PhaseProgress) {
        if let Some(job) = &self.job {
            let mut id = lock(&self.phase_id);
            if *id != p.phase {
                id.clone_from(&p.phase);
                // One steady phase per kind, on its own scale: the repetitions finished. The child's live wording is in the
                // overview; the job's label does not flicker with it (ER-18).
                job.phase(Phase::items(&p.phase, self.kind.title(), u64::from(p.of)));
            }
            job.set(u64::from(p.rep));
        }
        // The job is updated before the overview says so: whoever sees the overview sees the job at least as far.
        if let Some(r) = lock(&self.centre.inner.state).running.as_mut() {
            r.phase = Some(p.clone());
        }
        self.centre.touch();
    }

    fn liveness(&self, l: Liveness) {
        if let Some(r) = lock(&self.centre.inner.state).running.as_mut() {
            r.liveness = l;
        }
        self.centre.touch();
    }

    fn note(&self, text: &str) {
        let mut n = lock(&self.notes);
        if n.len() < 20 {
            n.push(text.to_string());
        }
    }

    fn sample(&self, secs: u32, throughput: f64) {
        let env = lock(&self.centre.inner.env).clone();
        let input = env.throttle_input();
        let point = dyno_core::bench::cpu::TracePoint {
            secs,
            throughput,
            freq_mhz: input.as_ref().map(|i| i.frequency_mhz).filter(|f| *f > 0),
            temp_c: input.as_ref().and_then(|i| i.temperature_c),
        };
        if let Some(i) = &input {
            let why = lock(&self.heat).push(
                secs,
                i.temperature_c,
                i.critical_c,
                dyno_core::bench::cpu::os_critical(&i.os),
                dyno_core::throttle::HOT_MARGIN_C,
                dyno_core::throttle::HOT_DEFAULT_C,
            );
            if let Some(why) = why {
                *lock(&self.heat_stop) = Some(why.to_string());
            }
            for ev in lock(&self.tracker).push(*i) {
                let (dyno_core::throttle::TrackerEvent::Ended(e) | dyno_core::throttle::TrackerEvent::Started(e)) = ev;
                let mut all = lock(&self.episodes);
                all.retain(|x| x.id != e.id);
                all.push(e);
            }
        }
        lock(&self.trace).push(point);
        if let Some(r) = lock(&self.centre.inner.state).running.as_mut() {
            r.trace = lock(&self.trace).clone();
        }
        self.centre.touch();
    }

    fn stop(&self) -> Option<Stop> {
        if let Some(why) = lock(&self.heat_stop).clone() {
            *lock(&self.safety) = Some(why.clone());
            return Some(Stop::Safety(why));
        }
        if self.centre.inner.cancel.load(Ordering::SeqCst) || self.job.as_ref().is_some_and(|j| j.cancel_token().is_cancelled()) {
            return Some(Stop::Cancelled);
        }
        let n = self.ticks.fetch_add(1, Ordering::Relaxed);
        if n.is_multiple_of(POWER_EVERY_TICKS) {
            let now = lock(&self.centre.inner.env).clone().on_battery();
            *lock(&self.on_battery) = now;
        }
        if stop_for_battery(*lock(&self.on_battery), self.consent) {
            *lock(&self.safety) = Some(BATTERY_STOP_REASON.to_string());
            return Some(Stop::Safety(BATTERY_STOP_REASON.to_string()));
        }
        None
    }
}

impl BenchCentre {
    /// Loads the history from `file` (damage means an empty history) and runs through `launcher`; scratch folders
    /// are made inside `scratch_root`. A unit an earlier agent left behind is cleaned up first.
    pub fn load(file: PathBuf, scratch_root: PathBuf, launcher: Box<dyn BenchLauncher>) -> (BenchCentre, Option<String>) {
        let (history, warning) = files::load(&file, BenchHistory::parse);
        launcher.cleanup_leftovers();
        for gone in bench_scratch::cleanup_leftovers(&bench_scratch::records_file(&file)) {
            log::info!("dyno-agent: removed a scratch folder an earlier agent left: {gone}");
        }
        let centre = BenchCentre {
            inner: Arc::new(Inner {
                changed: std::sync::Condvar::new(),
                state: Mutex::new(State {
                    history: history.unwrap_or_default(),
                    running: None,
                    counter: 0,
                }),
                cancel: AtomicBool::new(false),
                launcher,
                env: Mutex::new(Arc::new(NoEnv)),
                file,
                scratch_root,
            }),
        };
        (centre, warning)
    }

    /// Gives the centre the agent's job registry and power source (they exist only after the centre is built).
    pub fn attach(&self, env: Arc<dyn BenchEnv>) {
        *lock(&self.inner.env) = env;
    }

    /// The graphics adapters, listed by the graphics runner (the request starts it for a moment).
    pub fn gpu_adapters(&self) -> Result<Vec<dyno_core::bench::gpu::GpuAdapter>, String> {
        self.inner.launcher.gpu_adapters()
    }

    /// The history and the run in progress.
    pub fn overview(&self) -> BenchOverview {
        let s = lock(&self.inner.state);
        BenchOverview {
            running: s.running.clone(),
            runs: s.history.runs.clone(),
            disk_test_enabled: dyno_core::bench::DISK_TEST_ENABLED,
            estimates: dyno_core::bench::estimate::standard(),
        }
    }

    /// One saved run.
    pub fn run(&self, id: &str) -> Option<BenchRun> {
        lock(&self.inner.state).history.get(id).cloned()
    }

    /// Deletes a saved run.
    pub fn delete(&self, id: &str) -> Result<(), AgentError> {
        let mut s = lock(&self.inner.state);
        if !s.history.remove(id) {
            return Err(AgentError::new(ErrorCode::NotFound, "That benchmark run no longer exists."));
        }
        self.save(&s.history);
        Ok(())
    }

    /// Asks the run in progress to stop now: the child is ended in whatever phase it is, the kind is stopped and the rest are
    /// cancelled before they start.
    pub fn cancel(&self) {
        self.inner.cancel.store(true, Ordering::SeqCst);
    }

    /// The agent is stopping: cancels the run and waits up to `wait` for its child (and unit) to be gone.
    pub fn shutdown(&self, wait: Duration) {
        self.cancel();
        self.wait_for(wait, |o| o.running.is_none());
    }

    fn touch(&self) {
        self.inner.changed.notify_all();
    }

    /// Waits up to `timeout` for `done` to hold of the overview, on the condition (the state's changes signal it), not on time.
    /// Returns the overview that satisfied it, or `None` at the timeout.
    pub fn wait_for(&self, timeout: Duration, done: impl Fn(&BenchOverview) -> bool) -> Option<BenchOverview> {
        let overview = |s: &State| BenchOverview {
            running: s.running.clone(),
            runs: s.history.runs.clone(),
            disk_test_enabled: dyno_core::bench::DISK_TEST_ENABLED,
            estimates: dyno_core::bench::estimate::standard(),
        };
        let guard = lock(&self.inner.state);
        let (guard, _) = self
            .inner
            .changed
            .wait_timeout_while(guard, timeout, |s| !done(&overview(s)))
            .unwrap_or_else(|p| p.into_inner());
        let o = overview(&guard);
        done(&o).then_some(o)
    }

    fn save(&self, h: &BenchHistory) {
        if let Err(e) = files::save(&self.inner.file, h) {
            log::error!("dyno-agent: could not save the benchmark history: {e}");
        }
    }

    /// Starts a run in the background and returns at once. Refused with `Busy` while another is running, and with `NeedsConsent`
    /// on battery unless `allow_battery` (consent for this run only, D6).
    pub fn start(
        &self,
        plan: RunPlan,
        phase: Option<BenchPhase>,
        load: LoadAtRun,
        machine: String,
        allow_battery: bool,
    ) -> Result<BenchOverview, AgentError> {
        if battery_preflight(load.on_battery, allow_battery) == Preflight::NeedsConsent {
            log::info!("dyno-agent: a benchmark was asked for on battery without consent; asking first");
            return Err(AgentError::new(ErrorCode::NeedsConsent, BATTERY_CONSENT));
        }
        if let Some(minutes) = plan.sustained_minutes {
            let readable = lock(&self.inner.env)
                .clone()
                .throttle_input()
                .is_some_and(|i| i.temperature_c.is_some());
            dyno_core::bench::cpu::check_length(minutes, readable).map_err(|e| AgentError::new(ErrorCode::BadRequest, e))?;
        }
        let started = now();
        let run_id = {
            let mut s = lock(&self.inner.state);
            if s.running.is_some() {
                return Err(AgentError::new(
                    ErrorCode::Busy,
                    "A benchmark is already running; wait for it or cancel it.",
                ));
            }
            s.counter += 1;
            let id = format!("{}-{}", started.millis(), s.counter);
            s.running = Some(BenchProgress {
                run_id: id.clone(),
                started_at: started,
                kinds: plan.kinds.clone(),
                current: None,
                done: Vec::new(),
                phase: None,
                liveness: Liveness::Running,
                limited: self.inner.launcher.limited(),
                trace: Vec::new(),
            });
            id
        };
        self.touch();
        self.inner.cancel.store(false, Ordering::SeqCst);
        let me = self.clone();
        // The orchestration thread stays at its normal role; the measuring child is the narrow rule 52 exception (bench_child).
        let spawned = std::thread::Builder::new().name("dyno-bench".into()).spawn({
            let run_id = run_id.clone();
            move || me.execute(&run_id, plan, phase, load, machine, allow_battery)
        });
        if let Err(e) = spawned {
            lock(&self.inner.state).running = None;
            self.touch();
            return Err(AgentError::new(ErrorCode::Internal, format!("Could not start the benchmark: {e}")));
        }
        Ok(self.overview())
    }

    fn execute(&self, run_id: &str, plan: RunPlan, phase: Option<BenchPhase>, load: LoadAtRun, machine: String, consent: bool) {
        let t = Instant::now();
        let (started, limited) = {
            let s = lock(&self.inner.state);
            let r = s.running.as_ref();
            (r.map_or_else(now, |r| r.started_at), r.and_then(|r| r.limited.clone()))
        };
        log::info!(
            "dyno-agent: run {run_id} starts: kinds {:?}, on battery {:?} (consent {consent}), cpu {:.0}% memory {:.0}% in use{}",
            plan.kinds.iter().map(|k| k.name()).collect::<Vec<_>>(),
            load.on_battery,
            load.cpu_percent,
            load.memory_percent,
            limited.as_deref().map_or(String::new(), |l| format!(", {l}"))
        );
        let env = lock(&self.inner.env).clone();
        let job = env.register_job("Running benchmarks", Some(&plan.label));
        let outcome = match &job {
            Some(ctx) => {
                let mut out = None;
                let _ = ctx.run(|c| {
                    let r = self.run_kinds(run_id, &plan, Some(c), consent);
                    let end = if let Some(why) = &r.safety {
                        // A named safety stop, not the person's cancel: the job ends failed with the words that say why.
                        Err(JobError::Failed(why.clone()))
                    } else if r.cancelled {
                        Err(JobError::Cancelled(CancelReason::User))
                    } else {
                        Ok(())
                    };
                    out = Some(r);
                    end
                });
                out.unwrap_or_default()
            }
            None => self.run_kinds(run_id, &plan, None, consent),
        };
        let (results, cancelled) = (outcome.results, outcome.cancelled);
        let failed = results.iter().filter(|r| matches!(r.status, BenchStatus::Failed { .. })).count();
        let run = BenchRun {
            id: run_id.to_string(),
            started_at: started,
            label: plan.label,
            group: plan.group,
            phase,
            load,
            machine,
            results,
            old_way: false,
            limited,
        };
        let mut s = lock(&self.inner.state);
        s.history.push(run);
        s.running = None;
        self.save(&s.history);
        self.touch();
        log::info!(
            "dyno-agent: run {run_id} ended after {} ms: {} (failed kinds {failed}); history saved, scratch cleaned up",
            t.elapsed().as_millis(),
            if cancelled { "stopped" } else { "finished" }
        );
    }

    /// Every kind in order (the disk test once per chosen drive); returns the results and how the run ended.
    fn run_kinds(&self, run_id: &str, plan: &RunPlan, job: Option<&JobContext>, consent: bool) -> RunOutcome {
        let mut results: Vec<KindResult> = Vec::new();
        let mut stopped = false;
        let mut safety = None;
        for kind in &plan.kinds {
            // The disk test is one measurement per drive; every other kind runs once.
            let targets: Vec<Option<disk_target::Target>> = if *kind == BenchKind::Disk {
                let env = lock(&self.inner.env).clone();
                disk_target::resolve(plan, &env.volumes(), &self.inner.scratch_root)
                    .into_iter()
                    .map(Some)
                    .collect()
            } else {
                vec![None]
            };
            let last = targets.len().saturating_sub(1);
            for (i, target) in targets.into_iter().enumerate() {
                // Cancel from the page or from the Jobs view (the job's token).
                let cancelled = self.inner.cancel.load(Ordering::SeqCst) || job.is_some_and(|j| j.cancel_token().is_cancelled());
                let device = target.as_ref().map(|t| t.name.clone());
                if stopped || cancelled {
                    stopped = true;
                    results.push(KindResult {
                        kind: *kind,
                        millis: 0,
                        method: kind.method_version(),
                        device,
                        needs_folder: false,
                        trace: Vec::new(),
                        details: Vec::new(),
                        status: BenchStatus::Stopped {
                            reason: "Cancelled before it started.".into(),
                        },
                    });
                    continue;
                }
                if let Some(r) = lock(&self.inner.state).running.as_mut() {
                    r.current = Some(*kind);
                    r.phase = None;
                    r.liveness = Liveness::Running;
                }
                self.touch();
                let t = Instant::now();
                let events = RunEvents::new(self.clone(), *kind, job.cloned(), consent);
                let (status, needs_folder) = match &target {
                    Some(t) => self.measure_disk(run_id, t, plan, &events),
                    None => (self.measure(run_id, *kind, plan, &events), false),
                };
                if let Some(why) = lock(&events.safety).take() {
                    safety = Some(why);
                }
                let millis = u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX);
                let outcome = match &status {
                    BenchStatus::Done { metrics } => format!("done, {} metrics", metrics.len()),
                    BenchStatus::Unavailable { reason } => format!("unavailable ({reason})"),
                    BenchStatus::Stopped { reason } => format!("stopped ({reason})"),
                    BenchStatus::Failed { reason } => format!("failed ({reason})"),
                };
                log::info!(
                    "dyno-agent: run {run_id} {}{}: {outcome} in {millis} ms",
                    kind.name(),
                    device.as_deref().map_or(String::new(), |d| format!(" on {d}"))
                );
                // A safety stop ends the run, not only the kind: the rest never start.
                if matches!(&status, BenchStatus::Stopped { .. }) {
                    stopped = true;
                }
                results.push(KindResult {
                    kind: *kind,
                    millis,
                    method: kind.method_version(),
                    device,
                    needs_folder,
                    trace: events.take_trace(),
                    details: disk_target::facts(target.as_ref())
                        .into_iter()
                        .chain(std::mem::take(&mut *lock(&events.notes)))
                        .collect(),
                    status,
                });
                if i == last {
                    if let Some(r) = lock(&self.inner.state).running.as_mut() {
                        r.current = None;
                        r.phase = None;
                        r.done.push(*kind);
                    }
                }
                self.touch();
            }
        }
        RunOutcome {
            results,
            cancelled: stopped,
            safety,
        }
    }

    /// One kind other than disk.
    fn measure(&self, run_id: &str, kind: BenchKind, plan: &RunPlan, events: &RunEvents) -> BenchStatus {
        let sustained_secs = (kind == BenchKind::Cpu).then_some(plan.sustained_minutes).flatten().map(|m| m * 60);
        let req = ChildRequest {
            run_id: run_id.to_string(),
            kind,
            scratch: PathBuf::new(),
            disk_mib: plan.disk_mib,
            rotational: false,
            sustained_secs,
            sustained_write_mib: 0,
            available_mib: (kind == BenchKind::Memory)
                .then(|| lock(&self.inner.env).clone().available_memory())
                .flatten()
                .map(|b| b / (1024 * 1024)),
            gpu_adapter: (kind == BenchKind::Gpu).then_some(plan.gpu_adapter).flatten(),
        };
        let status = self.launch(&req, events);
        if sustained_secs.is_some() {
            events.finish_sustained(status)
        } else {
            status
        }
    }

    /// The disk test on one drive: refusals and free space first, then the scratch folder (recorded before anything is written),
    /// the runner, and the folder removed whatever the runner did. The numbers get the drive in their ids and labels, so runs
    /// are compared drive by drive.
    fn measure_disk(&self, run_id: &str, target: &disk_target::Target, plan: &RunPlan, events: &RunEvents) -> (BenchStatus, bool) {
        let disk_mib = plan.disk_mib;
        if let Some(reason) = &target.refusal {
            log::info!("dyno-agent: run {run_id}: disk {}: not tested: {reason}", target.name);
            return (BenchStatus::Unavailable { reason: reason.clone() }, false);
        }
        let file_bytes = u64::from(disk_mib) * dyno_core::bench::disk::MIB;
        // The numbers as of now, not as of the last listing.
        let now_space = lock(&self.inner.env).clone().space(&target.mount.to_string_lossy());
        let (cap, free) = now_space.map_or((target.capacity, target.free), |(c, f)| (Some(c), Some(f)));
        if let Err(reason) = dyno_core::bench::disk::check_free(&target.name, file_bytes, cap, free) {
            log::info!("dyno-agent: run {run_id}: disk {}: not tested: {reason}", target.name);
            return (BenchStatus::Unavailable { reason }, false);
        }
        let records = bench_scratch::records_file(&self.inner.file);
        let folder = match bench_scratch::ScratchFolder::create(&target.folder(run_id), &records) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("dyno-agent: run {run_id}: disk {}: no scratch folder: {e}", target.name);
                return (
                    BenchStatus::Unavailable {
                        reason: format!(
                            "Dyno could not make a folder to test {} ({e}). Choose a folder on that drive that you can write to, or on a Mac allow Dyno under System Settings > Privacy & Security > Files and Folders, then run again.",
                            target.name
                        ),
                    },
                    wants_folder(&e, target),
                );
            }
        };
        // A permission prompt belongs here, before the run, while the person is looking at the screen.
        if let Err(e) = folder.probe() {
            log::warn!("dyno-agent: run {run_id}: disk {}: the probe was refused: {e}", target.name);
            return (
                BenchStatus::Unavailable {
                    reason: format!(
                        "{} did not let Dyno write to it ({e}). Choose a folder on that drive that you can write to, or on a Mac allow Dyno under System Settings > Privacy & Security > Files and Folders, then run again.",
                        target.name
                    ),
                },
                wants_folder(&e, target),
            );
        }
        // The sustained write the person confirmed, cut to what this drive can take (the smaller of 25 % of its free space and 64 GiB).
        let sustained_write_mib = plan.sustained_write_gib.map_or(0, |asked| {
            let cap = dyno_core::bench::disk::sustained_write_cap_gib(free.unwrap_or(0));
            if cap < asked {
                log::info!(
                    "dyno-agent: run {run_id}: disk {}: sustained write cut from {asked} to {cap} GiB by the free space",
                    target.name
                );
            }
            asked.min(cap) * 1024
        });
        let req = ChildRequest {
            run_id: run_id.to_string(),
            kind: BenchKind::Disk,
            scratch: folder.path().to_path_buf(),
            disk_mib,
            rotational: target.rotational == Some(true),
            sustained_secs: None,
            sustained_write_mib,
            available_mib: None,
            gpu_adapter: None,
        };
        let status = self.launch(&req, events);
        drop(folder);
        (disk_target::mark(status, target), false)
    }

    /// Hands the request to the launcher; a panic in it is one failed kind.
    fn launch(&self, req: &ChildRequest, events: &RunEvents) -> BenchStatus {
        let launcher = &self.inner.launcher;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| launcher.run(req, events))).unwrap_or_else(|_| {
            log::error!("dyno-agent: run {} {}: the launcher panicked", req.run_id, req.kind.name());
            BenchStatus::Failed {
                reason: "The benchmark runner crashed.".into(),
            }
        })
    }
}

/// Whether a refusal to write is one a folder chosen by the person can cure: the drive's top is not writable, no folder was chosen yet,
/// and this is not the system drive (whose scratch folder is in the temp folder).
fn wants_folder(e: &std::io::Error, t: &disk_target::Target) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
    ) && !t.is_system
        && t.folder_root.is_none()
}

/// The file version written next to the history (kept for the schema test).
pub const FILE_VERSION: u32 = HISTORY_VERSION;

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::bench::{plan_with, BenchMetric};
    use paddock_jobs::{EventSink, JobRegistry, JobSnapshot};
    use std::sync::atomic::AtomicUsize;

    struct Fake {
        calls: AtomicUsize,
        gate: Option<Arc<(Mutex<bool>, std::sync::Condvar)>>,
        panic_on: Option<BenchKind>,
        /// Reports this progress and liveness before it answers.
        talk: bool,
        /// Waits for the run to be stopped instead of answering.
        until_stopped: bool,
        /// The scratch folders the disk test was given, in order.
        seen: Arc<Mutex<Vec<PathBuf>>>,
        /// A sustained run reports a sample a second for this many seconds (zero: it answers at once).
        sustained_for: u32,
    }

    impl Fake {
        fn plain() -> Fake {
            Fake {
                calls: AtomicUsize::new(0),
                gate: None,
                panic_on: None,
                talk: false,
                until_stopped: false,
                seen: Arc::default(),
                sustained_for: 0,
            }
        }
    }

    impl BenchLauncher for Fake {
        fn run(&self, req: &ChildRequest, events: &dyn ChildEvents) -> BenchStatus {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if req.kind == BenchKind::Disk {
                lock(&self.seen).push(req.scratch.clone());
            }
            if let Some(g) = &self.gate {
                let mut open = lock(&g.0);
                while !*open {
                    open = g.1.wait(open).unwrap();
                }
            }
            if Some(req.kind) == self.panic_on {
                panic!("boom");
            }
            if self.talk {
                events.progress(&PhaseProgress {
                    phase: "cpu".into(),
                    label: "Pass 3 of 5 · Single core".into(),
                    rep: 2,
                    of: 5,
                    count: 2,
                    total: Some(5),
                });
                events.liveness(Liveness::NotResponding);
            }
            if req.sustained_secs.is_some() && self.sustained_for > 0 {
                for s in 1..=self.sustained_for {
                    events.sample(s, 100.0);
                    if let Some(Stop::Safety(reason)) = events.stop() {
                        return BenchStatus::Stopped { reason };
                    }
                }
                return BenchStatus::Done {
                    metrics: vec![BenchMetric::higher("cpu-sustained-peak", "Peak", 100.0, "GFLOPS")],
                };
            }
            if self.until_stopped {
                loop {
                    match events.stop() {
                        Some(Stop::Cancelled) => {
                            return BenchStatus::Stopped {
                                reason: "Cancelled.".into(),
                            }
                        }
                        Some(Stop::Safety(r)) => return BenchStatus::Stopped { reason: r },
                        None => std::thread::yield_now(),
                    }
                }
            }
            if req.kind == BenchKind::Disk {
                assert!(req.scratch.is_dir(), "the disk test gets its scratch folder");
                std::fs::write(req.scratch.join("left-behind"), b"x").unwrap();
            }
            BenchStatus::Done {
                metrics: vec![BenchMetric::higher(
                    if req.kind == BenchKind::Disk { "disk-seq1m-t1-read" } else { "m" },
                    "M",
                    1.0,
                    "u",
                )],
            }
        }
    }

    /// An environment with a chosen volume list and room (or not) on every drive; no jobs, no battery.
    struct Roomy {
        volumes: Vec<dyno_core::disk::volume::Volume>,
        space: Option<(u64, u64)>,
    }

    impl BenchEnv for Roomy {
        fn register_job(&self, _: &str, _: Option<&str>) -> Option<JobContext> {
            None
        }

        fn on_battery(&self) -> Option<bool> {
            None
        }

        fn volumes(&self) -> Vec<dyno_core::disk::volume::Volume> {
            self.volumes.clone()
        }

        fn space(&self, _: &str) -> Option<(u64, u64)> {
            self.space
        }
    }

    const TB: u64 = 1_000_000_000_000;

    fn roomy() -> Arc<Roomy> {
        Arc::new(Roomy {
            volumes: Vec::new(),
            space: Some((TB, TB / 2)),
        })
    }

    fn centre_with(dir: &Path, fake: Fake) -> BenchCentre {
        let c = BenchCentre::load(dir.join("bench.json"), dir.to_path_buf(), Box::new(fake)).0;
        c.attach(roomy());
        c
    }

    fn centre(dir: &Path, gate: Option<Arc<(Mutex<bool>, std::sync::Condvar)>>, panic_on: Option<BenchKind>) -> BenchCentre {
        centre_with(
            dir,
            Fake {
                gate,
                panic_on,
                ..Fake::plain()
            },
        )
    }

    fn wait_idle(c: &BenchCentre) -> BenchOverview {
        c.wait_for(Duration::from_secs(60), |o| o.running.is_none())
            .expect("the run did not finish")
    }

    /// Waits until the run is measuring a kind.
    fn wait_current(c: &BenchCentre) {
        c.wait_for(Duration::from_secs(60), |o| o.running.as_ref().is_some_and(|r| r.current.is_some()))
            .expect("no kind started");
    }

    fn start(c: &BenchCentre, kinds: &[BenchKind]) -> Result<BenchOverview, AgentError> {
        start_with(c, kinds, LoadAtRun::default(), false)
    }

    fn start_with(c: &BenchCentre, kinds: &[BenchKind], load: LoadAtRun, allow_battery: bool) -> Result<BenchOverview, AgentError> {
        c.start(
            plan_with(kinds, None, "test run", None, true).unwrap(),
            None,
            load,
            "m".into(),
            allow_battery,
        )
    }

    /// Hands every job snapshot to the test, which waits on the channel and not on time.
    struct ChannelSink(Mutex<std::sync::mpsc::Sender<JobSnapshot>>);

    impl EventSink for ChannelSink {
        fn emit(&self, s: &JobSnapshot) {
            let _ = lock(&self.0).send(s.clone());
        }
    }

    /// An environment with a real job registry and a power source the test flips.
    struct Env {
        registry: JobRegistry,
        battery: Arc<Mutex<Option<bool>>>,
        jobs: Mutex<Vec<JobContext>>,
        registered: std::sync::Condvar,
        snapshots: Mutex<std::sync::mpsc::Receiver<JobSnapshot>>,
    }

    impl Env {
        fn new() -> Arc<Env> {
            let (tx, rx) = std::sync::mpsc::channel();
            Arc::new(Env {
                registry: JobRegistry::new(Arc::new(ChannelSink(Mutex::new(tx)))),
                battery: Arc::new(Mutex::new(Some(false))),
                jobs: Mutex::new(Vec::new()),
                registered: std::sync::Condvar::new(),
                snapshots: Mutex::new(rx),
            })
        }

        /// The first registered job's id, waiting for the run to register it.
        fn job_id(&self) -> paddock_jobs::JobId {
            let jobs = lock(&self.jobs);
            let (jobs, _) = self
                .registered
                .wait_timeout_while(jobs, Duration::from_secs(60), |j| j.is_empty())
                .unwrap_or_else(|p| p.into_inner());
            jobs.first().expect("no job registered").id()
        }

        /// The first snapshot of job `id` for which `pred` holds.
        fn snapshot_where(&self, id: paddock_jobs::JobId, pred: impl Fn(&JobSnapshot) -> bool) -> JobSnapshot {
            let rx = lock(&self.snapshots);
            loop {
                let s = rx.recv_timeout(Duration::from_secs(60)).expect("the job sent no such snapshot");
                if s.id == id && pred(&s) {
                    return s;
                }
            }
        }
    }

    impl BenchEnv for Env {
        fn register_job(&self, title: &str, subject: Option<&str>) -> Option<JobContext> {
            let ctx = self.registry.register("benchmark", title, subject);
            lock(&self.jobs).push(ctx.clone());
            self.registered.notify_all();
            Some(ctx)
        }

        fn on_battery(&self) -> Option<bool> {
            *lock(&self.battery)
        }

        fn space(&self, _: &str) -> Option<(u64, u64)> {
            Some((TB, TB / 2))
        }
    }

    #[test]
    fn a_run_measures_each_kind_saves_and_removes_the_scratch_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, None);
        // Whether the run is still going when start returns is a race with the fake; the gated test below pins the running state.
        start(&c, &[BenchKind::Cpu, BenchKind::Disk]).unwrap();
        let o = wait_idle(&c);
        assert_eq!(o.runs.len(), 1);
        assert_eq!(o.runs[0].results.len(), 2);
        assert!(o.runs[0].results.iter().all(|r| matches!(r.status, BenchStatus::Done { .. })));
        assert!(o.runs[0].results.iter().all(|r| r.method == r.kind.method_version()));
        assert!(!o.runs[0].old_way);
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(
            leftovers
                .iter()
                .all(|n| !n.to_string_lossy().contains("dyno-bench-") && n != "bench-scratch.json"),
            "{leftovers:?}"
        );
        // The history survives a restart.
        let (again, warn) = BenchCentre::load(
            tmp.path().join("bench.json"),
            tmp.path().to_path_buf(),
            Box::new(ProcessLauncher::at("nope")),
        );
        assert!(warn.is_none());
        assert_eq!(again.overview().runs.len(), 1);
        assert!(again.run(&o.runs[0].id).is_some());
    }

    fn volume(mount: &Path, name: &str) -> dyno_core::disk::volume::Volume {
        dyno_core::disk::volume::Volume {
            id: mount.to_string_lossy().into_owned(),
            name: name.into(),
            mount: mount.to_string_lossy().into_owned(),
            fs: "apfs".into(),
            location: dyno_core::disk::volume::VolumeLocation::External { bus: Some("USB".into()) },
            capacity: Some(TB),
            free: Some(TB / 2),
            rotational: Some(false),
            read_only: false,
            reachable: true,
            whole_volume: true,
            measured_at: None,
            freshness: Default::default(),
        }
    }

    fn disk_centre(tmp: &Path, volumes: Vec<dyno_core::disk::volume::Volume>, space: Option<(u64, u64)>, fake: Fake) -> BenchCentre {
        let c = BenchCentre::load(tmp.join("bench.json"), tmp.to_path_buf(), Box::new(fake)).0;
        c.attach(Arc::new(Roomy { volumes, space }));
        c
    }

    fn start_disk(c: &BenchCentre, drives: &[&Path]) {
        let ids: Vec<String> = drives.iter().map(|d| d.to_string_lossy().into_owned()).collect();
        let plan = plan_with(&[BenchKind::Disk], None, "disk", None, true)
            .unwrap()
            .with_drives(&ids, false)
            .unwrap();
        c.start(plan, None, LoadAtRun::default(), "m".into(), false).unwrap();
    }

    #[test]
    fn the_disk_test_runs_once_per_chosen_drive_in_its_own_folder_and_marks_the_numbers() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let fake = Fake::plain();
        let seen = fake.seen.clone();
        let c = disk_centre(
            tmp.path(),
            vec![volume(a.path(), "Alpha"), volume(b.path(), "Beta")],
            Some((TB, TB / 2)),
            fake,
        );
        start_disk(&c, &[b.path(), a.path()]);
        let o = wait_idle(&c);
        let r = &o.runs[0].results;
        assert_eq!(
            r.iter().map(|k| k.device.as_deref()).collect::<Vec<_>>(),
            [Some("Beta"), Some("Alpha")]
        );
        assert!(r.iter().all(|k| matches!(k.status, BenchStatus::Done { .. })));
        let ids: Vec<String> = o.runs[0].metrics().map(|(_, m)| m.id.clone()).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "two drives never share a metric id");
        assert!(ids.iter().all(|i| i.starts_with("disk-") && i.ends_with("-seq1m-t1-read")));
        // Each drive's folder is at its own top; both are gone, with what the runner left in them.
        let seen = lock(&seen);
        assert_eq!(seen[0].parent(), Some(b.path()));
        assert_eq!(seen[1].parent(), Some(a.path()));
        assert!(seen.iter().all(|p| !p.exists()));
        assert_eq!(
            std::fs::read_dir(a.path()).unwrap().count() + std::fs::read_dir(b.path()).unwrap().count(),
            0
        );
        assert!(!tmp.path().join("bench-scratch.json").exists(), "nothing is recorded any more");
    }

    #[test]
    fn a_refused_or_cramped_or_vanished_drive_is_unavailable_with_its_reason_and_the_others_still_run() {
        let tmp = tempfile::tempdir().unwrap();
        let (ok, ro) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut readonly = volume(ro.path(), "Frozen");
        readonly.read_only = true;
        let fake = Fake::plain();
        let seen = fake.seen.clone();
        let c = disk_centre(tmp.path(), vec![readonly, volume(ok.path(), "Fine")], Some((TB, TB / 2)), fake);
        let gone = Path::new("/Volumes/not-here");
        start_disk(&c, &[ro.path(), gone, ok.path()]);
        let r = wait_idle(&c).runs[0].results.clone();
        assert!(
            matches!(&r[0].status, BenchStatus::Unavailable { reason } if reason.contains("read-only")),
            "{:?}",
            r[0]
        );
        assert!(matches!(&r[1].status, BenchStatus::Unavailable { reason } if reason.contains("not connected")));
        assert!(matches!(r[2].status, BenchStatus::Done { .. }));
        assert_eq!(lock(&seen).len(), 1, "the runner started only for the drive that can be tested");

        // Too little free space: refused before anything is written.
        let tmp2 = tempfile::tempdir().unwrap();
        let fake = Fake::plain();
        let seen = fake.seen.clone();
        let c = disk_centre(tmp2.path(), vec![volume(ok.path(), "Fine")], Some((TB, 1_000_000)), fake);
        start_disk(&c, &[ok.path()]);
        let r = wait_idle(&c).runs[0].results.clone();
        assert!(
            matches!(&r[0].status, BenchStatus::Unavailable { reason } if reason.contains("free") && reason.contains("Fine")),
            "{:?}",
            r[0]
        );
        assert!(lock(&seen).is_empty());

        // The OS cannot say how much is free: the test does not write blind.
        let tmp3 = tempfile::tempdir().unwrap();
        let mut unknown = volume(ok.path(), "Fine");
        (unknown.capacity, unknown.free) = (None, None);
        let c = disk_centre(tmp3.path(), vec![unknown], None, Fake::plain());
        start_disk(&c, &[ok.path()]);
        assert!(matches!(&wait_idle(&c).runs[0].results[0].status, BenchStatus::Unavailable { reason } if reason.contains("cannot tell")));
    }

    #[test]
    fn cancel_during_the_first_drive_removes_its_folder_and_the_second_never_starts() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let fake = Fake {
            until_stopped: true,
            ..Fake::plain()
        };
        let seen = fake.seen.clone();
        let c = disk_centre(
            tmp.path(),
            vec![volume(a.path(), "Alpha"), volume(b.path(), "Beta")],
            Some((TB, TB / 2)),
            fake,
        );
        start_disk(&c, &[a.path(), b.path()]);
        wait_current(&c);
        c.cancel();
        let r = wait_idle(&c).runs[0].results.clone();
        assert!(matches!(&r[0].status, BenchStatus::Stopped { .. }));
        assert!(matches!(&r[1].status, BenchStatus::Stopped { reason } if reason.contains("before it started")));
        assert_eq!(lock(&seen).len(), 1);
        assert_eq!(std::fs::read_dir(a.path()).unwrap().count(), 0, "the folder is gone after Cancel");
        assert!(!tmp.path().join("bench-scratch.json").exists());
    }

    #[test]
    fn a_crash_of_the_agent_is_cleaned_up_at_the_next_start_by_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let drive = tempfile::tempdir().unwrap();
        let folder = drive.path().join(".dyno-bench-crashed");
        // An earlier agent made the folder and its record, then died (no drop).
        std::mem::forget(
            bench_scratch::ScratchFolder::create(&folder, &bench_scratch::records_file(&tmp.path().join("bench.json"))).unwrap(),
        );
        std::fs::write(folder.join("dyno-bench.tmp"), b"x").unwrap();
        let _ = BenchCentre::load(tmp.path().join("bench.json"), tmp.path().to_path_buf(), Box::new(Fake::plain()));
        assert!(!folder.exists());
    }

    /// Readings that advance a second per ask, at a fixed temperature.
    struct Readings {
        temp: Option<f32>,
        at: std::sync::atomic::AtomicI64,
    }

    impl BenchEnv for Readings {
        fn register_job(&self, _: &str, _: Option<&str>) -> Option<JobContext> {
            None
        }

        fn on_battery(&self) -> Option<bool> {
            None
        }

        fn throttle_input(&self) -> Option<dyno_core::throttle::ThrottleInput> {
            Some(dyno_core::throttle::ThrottleInput {
                at: dyno_core::time::Timestamp(self.at.fetch_add(1000, Ordering::SeqCst)),
                load: 100.0,
                frequency_mhz: 3000,
                expected_mhz: 3000,
                temperature_c: self.temp,
                critical_c: self.temp.map(|_| 100.0),
                on_battery: Some(false),
                power_saver: None,
                os: dyno_core::hardware::OsThrottle::default(),
            })
        }
    }

    fn readings(temp: Option<f32>) -> Arc<Readings> {
        Arc::new(Readings {
            temp,
            at: std::sync::atomic::AtomicI64::new(1_000_000),
        })
    }

    fn start_sustained(c: &BenchCentre, minutes: u32) -> Result<BenchOverview, AgentError> {
        let plan = plan_with(&[BenchKind::Cpu], None, "hot", None, true)
            .unwrap()
            .with_sustained(Some(minutes), None)
            .unwrap();
        c.start(plan, None, LoadAtRun::default(), "m".into(), false)
    }

    #[test]
    fn a_sustained_run_keeps_its_trace_with_what_the_agent_read_beside_it() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(
            tmp.path(),
            Fake {
                sustained_for: 30,
                ..Fake::plain()
            },
        );
        c.attach(readings(Some(60.0)));
        start_sustained(&c, 5).unwrap();
        let o = wait_idle(&c);
        let r = &o.runs[0].results[0];
        assert_eq!(r.trace.len(), 30);
        assert_eq!(
            (r.trace[0].secs, r.trace[0].freq_mhz, r.trace[0].temp_c),
            (1, Some(3000), Some(60.0))
        );
        let BenchStatus::Done { metrics } = &r.status else {
            panic!("{:?}", r.status)
        };
        assert!(
            metrics.iter().any(|m| m.id == "cpu-sustained-episodes"),
            "the tracker's verdict is part of the result"
        );
    }

    #[test]
    fn staying_near_the_critical_temperature_stops_the_run_with_the_reason_and_keeps_the_trace() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(
            tmp.path(),
            Fake {
                sustained_for: 60,
                ..Fake::plain()
            },
        );
        c.attach(readings(Some(97.0)));
        start_sustained(&c, 5).unwrap();
        let o = wait_idle(&c);
        let r = &o.runs[0].results[0];
        assert!(
            matches!(&r.status, BenchStatus::Stopped { reason } if reason.contains("critical temperature")),
            "{:?}",
            r.status
        );
        // Ten seconds hot, then the stop: the points up to it are kept.
        assert_eq!(r.trace.len(), 11);
    }

    #[test]
    fn where_no_temperature_can_be_read_the_sustained_run_is_limited_to_five_minutes() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(tmp.path(), Fake::plain());
        c.attach(readings(None));
        let e = start_sustained(&c, 10).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
        assert!(e.message.contains("limited to 5 minutes"));
        assert!(start_sustained(&c, 5).is_ok());
        wait_idle(&c);
        // With a sensor the longer choices are open.
        c.attach(readings(Some(50.0)));
        assert!(start_sustained(&c, 10).is_ok());
        wait_idle(&c);
    }

    #[test]
    fn a_second_run_is_refused_while_one_runs_and_cancel_stops_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let c = centre(tmp.path(), Some(gate.clone()), None);
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        assert_eq!(start(&c, &[BenchKind::Cpu]).unwrap_err().code, ErrorCode::Busy);
        wait_current(&c);
        c.cancel();
        *lock(&gate.0) = true;
        gate.1.notify_all();
        let o = wait_idle(&c);
        assert!(matches!(o.runs[0].results[0].status, BenchStatus::Done { .. }));
        assert!(matches!(o.runs[0].results[1].status, BenchStatus::Stopped { .. }));
    }

    #[test]
    fn cancel_in_any_phase_stops_the_kind_and_the_rest_never_start() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(
            tmp.path(),
            Fake {
                until_stopped: true,
                ..Fake::plain()
            },
        );
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        wait_current(&c);
        c.cancel();
        let o = wait_idle(&c);
        let reasons: Vec<_> = o.runs[0]
            .results
            .iter()
            .map(|r| match &r.status {
                BenchStatus::Stopped { reason } => reason.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(reasons, vec!["Cancelled.", "Cancelled before it started."]);
        assert_eq!(o.runs.len(), 1, "a stopped run is still recorded, consistently");
    }

    #[test]
    fn the_childs_phase_and_liveness_reach_the_overview_while_it_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        // The fake reports after the gate opens; the check reads the state it left, then lets it finish.
        let c = centre_with(
            tmp.path(),
            Fake {
                talk: true,
                until_stopped: true,
                gate: Some(gate.clone()),
                ..Fake::plain()
            },
        );
        start(&c, &[BenchKind::Cpu]).unwrap();
        *lock(&gate.0) = true;
        gate.1.notify_all();
        let seen = c
            .wait_for(Duration::from_secs(60), |o| o.running.as_ref().is_some_and(|r| r.phase.is_some()))
            .and_then(|o| o.running)
            .and_then(|r| r.phase)
            .expect("no phase reported");
        assert_eq!((seen.phase.as_str(), seen.rep, seen.of), ("cpu", 2, 5));
        assert!(
            seen.label.contains("Pass 3 of 5"),
            "the child's live wording reaches the overview: {}",
            seen.label
        );
        c.cancel();
        wait_idle(&c);
    }

    #[test]
    fn a_job_is_registered_with_phases_and_its_cancel_stops_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(
            tmp.path(),
            Fake {
                talk: true,
                until_stopped: true,
                ..Fake::plain()
            },
        );
        let env = Env::new();
        c.attach(env.clone());
        start(&c, &[BenchKind::Cpu]).unwrap();
        let id = env.job_id();
        // The job's phase is steady per kind (the kind's name) on its own scale: the repetitions finished.
        c.wait_for(Duration::from_secs(60), |o| o.running.as_ref().is_some_and(|r| r.phase.is_some()))
            .expect("no phase reported");
        let snap = env.registry.snapshots().into_iter().find(|s| s.id == id).unwrap();
        let paddock_jobs::JobState::Running { phase: Some(phase) } = snap.state else {
            panic!("{:?}", snap.state)
        };
        assert_eq!(phase.phase.label, "Processor");
        assert_eq!((phase.done, phase.phase.total), (2, Some(5)));
        // Cancel from the Jobs view, not the page.
        assert!(env.registry.cancel(id));
        let o = wait_idle(&c);
        assert!(matches!(o.runs[0].results[0].status, BenchStatus::Stopped { .. }));
        let end = env.snapshot_where(id, |s| s.state.is_terminal());
        assert!(matches!(end.state, paddock_jobs::JobState::Cancelled { .. }), "{:?}", end.state);
    }

    #[test]
    fn battery_needs_consent_for_this_run_only() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, None);
        let on_battery = LoadAtRun {
            on_battery: Some(true),
            ..LoadAtRun::default()
        };
        let e = start_with(&c, &[BenchKind::Cpu], on_battery, false).unwrap_err();
        assert_eq!(e.code, ErrorCode::NeedsConsent);
        assert!(e.message.starts_with("On battery."));
        assert!(c.overview().running.is_none() && c.overview().runs.is_empty(), "nothing started");
        start_with(&c, &[BenchKind::Cpu], on_battery, true).unwrap();
        wait_idle(&c);
        // The next click asks again.
        assert_eq!(
            start_with(&c, &[BenchKind::Cpu], on_battery, false).unwrap_err().code,
            ErrorCode::NeedsConsent
        );
    }

    #[test]
    fn switching_to_battery_without_consent_stops_the_run_with_a_named_reason() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(
            tmp.path(),
            Fake {
                until_stopped: true,
                ..Fake::plain()
            },
        );
        let env = Env::new();
        c.attach(env.clone());
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        wait_current(&c);
        *lock(&env.battery) = Some(true);
        let o = wait_idle(&c);
        assert!(matches!(&o.runs[0].results[0].status, BenchStatus::Stopped { reason } if reason.starts_with("Stopped for safety")));
        assert!(matches!(&o.runs[0].results[1].status, BenchStatus::Stopped { reason } if reason.contains("before it started")));
        // The job names the safety stop; it is not shown as the person's cancel.
        let end = env.snapshot_where(env.job_id(), |s| s.state.is_terminal());
        assert!(
            matches!(&end.state, paddock_jobs::JobState::Failed { message, .. } if message.starts_with("Stopped for safety")),
            "{:?}",
            end.state
        );
    }

    #[test]
    fn a_cancel_from_the_jobs_view_between_kinds_stops_the_kinds_that_have_not_started() {
        let tmp = tempfile::tempdir().unwrap();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let c = centre(tmp.path(), Some(gate.clone()), None);
        let env = Env::new();
        c.attach(env.clone());
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        // Only the job's token is cancelled (not the page's flag) while the first kind is held at the gate.
        assert!(env.registry.cancel(env.job_id()));
        *lock(&gate.0) = true;
        gate.1.notify_all();
        let o = wait_idle(&c);
        assert!(matches!(&o.runs[0].results[1].status, BenchStatus::Stopped { reason } if reason.contains("before it started")));
    }

    struct Quiet(Option<Stop>);

    impl ChildEvents for Quiet {
        fn progress(&self, _: &PhaseProgress) {}
        fn liveness(&self, _: Liveness) {}
        fn stop(&self) -> Option<Stop> {
            self.0.clone()
        }
    }

    /// A clock the pause moves, so the wait for the InvocationID needs no real time.
    fn waiting(find_after: usize, events: &dyn ChildEvents, budget: Duration) -> (InvocationWait, usize, Duration) {
        let clock = paddock_bench::ManualClock::new();
        let looks = std::cell::Cell::new(0);
        let mut find = || {
            looks.set(looks.get() + 1);
            (looks.get() > find_after).then(|| "0123456789abcdef0123456789abcdef".to_string())
        };
        let r = wait_for_invocation(&mut find, events, &clock, &|d| clock.advance(d), budget);
        (r, looks.get(), clock.now())
    }

    #[test]
    fn the_invocation_wait_finds_the_id_times_out_and_honours_cancel_on_the_injected_clock() {
        let (r, looks, at) = waiting(3, &Quiet(None), Duration::from_secs(3));
        assert!(matches!(r, InvocationWait::Found(_)));
        assert_eq!((looks, at), (4, Duration::from_millis(300)));
        let (r, _, at) = waiting(usize::MAX, &Quiet(None), Duration::from_secs(3));
        assert_eq!(r, InvocationWait::TimedOut);
        assert_eq!(at, Duration::from_secs(3));
        // Cancel is seen before the first look: nothing waits for a unit that is not wanted.
        let (r, looks, at) = waiting(usize::MAX, &Quiet(Some(Stop::Cancelled)), Duration::from_secs(3));
        assert_eq!((r, looks, at), (InvocationWait::Stopped(Stop::Cancelled), 0, Duration::ZERO));
    }

    #[test]
    fn with_consent_a_switch_to_battery_does_not_stop_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, None);
        let env = Env::new();
        *lock(&env.battery) = Some(true);
        c.attach(env);
        let on_battery = LoadAtRun {
            on_battery: Some(true),
            ..LoadAtRun::default()
        };
        start_with(&c, &[BenchKind::Cpu], on_battery, true).unwrap();
        assert!(matches!(wait_idle(&c).runs[0].results[0].status, BenchStatus::Done { .. }));
    }

    #[test]
    fn a_crashing_launcher_is_one_failed_kind_not_a_dead_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, Some(BenchKind::Memory));
        start(&c, &[BenchKind::Cpu, BenchKind::Memory]).unwrap();
        let o = wait_idle(&c);
        assert!(matches!(o.runs[0].results[1].status, BenchStatus::Failed { .. }));
        assert!(matches!(o.runs[0].results[0].status, BenchStatus::Done { .. }));
        // And it can run again.
        start(&c, &[BenchKind::Cpu]).unwrap();
        assert_eq!(wait_idle(&c).runs.len(), 2);
    }

    #[test]
    fn deleting_a_run_and_a_damaged_history() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre(tmp.path(), None, None);
        start(&c, &[BenchKind::Cpu]).unwrap();
        let id = wait_idle(&c).runs[0].id.clone();
        c.delete(&id).unwrap();
        assert_eq!(c.delete(&id).unwrap_err().code, ErrorCode::NotFound);
        std::fs::write(tmp.path().join("bench.json"), "{broken").unwrap();
        std::fs::write(tmp.path().join("bench.json.bak"), "{broken too").unwrap();
        let (c2, warn) = BenchCentre::load(
            tmp.path().join("bench.json"),
            tmp.path().to_path_buf(),
            Box::new(ProcessLauncher::at("nope")),
        );
        assert!(warn.is_some());
        assert!(c2.overview().runs.is_empty());
    }

    #[test]
    fn a_missing_runner_is_unavailable_with_a_next_step() {
        let tmp = tempfile::tempdir().unwrap();
        let req = ChildRequest {
            run_id: "1".into(),
            kind: BenchKind::Cpu,
            scratch: tmp.path().to_path_buf(),
            disk_mib: 16,
            rotational: false,
            sustained_secs: None,
            sustained_write_mib: 0,
            available_mib: None,
            gpu_adapter: None,
        };
        struct Silent;
        impl ChildEvents for Silent {
            fn progress(&self, _: &PhaseProgress) {}
            fn liveness(&self, _: Liveness) {}
            fn stop(&self) -> Option<Stop> {
                None
            }
        }
        let s = ProcessLauncher::at(tmp.path().join("absent")).run(&req, &Silent);
        assert!(matches!(s, BenchStatus::Unavailable { reason } if reason == RUNNER_MISSING && !reason.contains("reinstall Dyno")));
    }

    #[test]
    fn the_runner_command_line_names_the_kind_and_the_disk_options_only_for_disk() {
        let req = |kind| ChildRequest {
            run_id: "1".into(),
            kind,
            scratch: PathBuf::from("/s"),
            disk_mib: 64,
            rotational: false,
            sustained_secs: None,
            sustained_write_mib: 0,
            available_mib: None,
            gpu_adapter: None,
        };
        assert_eq!(ProcessLauncher::runner_args(&req(BenchKind::Cpu)), vec!["cpu"]);
        assert_eq!(
            ProcessLauncher::runner_args(&req(BenchKind::Disk)),
            vec!["disk", "--dir", "/s", "--disk-mib", "64"]
        );
    }

    #[test]
    fn shutdown_cancels_and_waits_for_the_run_to_end() {
        let tmp = tempfile::tempdir().unwrap();
        let c = centre_with(
            tmp.path(),
            Fake {
                until_stopped: true,
                ..Fake::plain()
            },
        );
        start(&c, &[BenchKind::Cpu]).unwrap();
        c.shutdown(Duration::from_secs(10));
        assert!(c.overview().running.is_none());
        // Idle: returns at once.
        c.shutdown(Duration::from_secs(10));
    }

    #[test]
    fn the_file_version_is_the_history_version() {
        assert_eq!(FILE_VERSION, 2);
    }
}
