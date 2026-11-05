//! The supervisor: one thread that owns every runner's state and carries out the core's decisions (ADR 0001-0003).
//!
//! Everything arrives as a [`Msg`] on one channel — the app's requests (from the IPC server), the runners' output
//! lines (from reader threads), the results of slow work (from worker threads) and log-on changes — and a tick
//! every second drives time. There are no locks: one runner's failure can only ever be a state of that runner.
//!
//! Per runner: a [`Controller`] (life, restarts, stalls, drain), a [`JobTracker`], the [`UpdateMachine`], the log
//! file and tail, and housekeeping. The service's own log (`<data>/logs/service.log`) never contains runner output
//! or secrets.

use crate::cleanup;
use crate::launch::{graceful_stop, kill_tree, LaunchSpec, LineSink, RunnerProcess};
use crate::launcher::Launcher;
use crate::logfile::LogWriter;
use crate::probe::Probe;
use crate::process_watch::ProcessWatch;
use crate::runner_files;
use crate::store;
use crate::terminate::{OsTerminator, Terminator};
use crate::work::{self, Registration, Sources};
use pitcrew_core::config::{self, check_server_url, Config, RunnerConfig};
use pitcrew_core::controller::{Action, Controller, ControllerSettings, Input, LaunchError, Life};
use pitcrew_core::health::{self, RunnerStatus};
use pitcrew_core::history::{summarize, HistoryKind, StopReason};
use pitcrew_core::housekeeping::{self, CleanupPlan, CleanupReport, CleanupTrigger, DiskScan};
use pitcrew_core::jobs::{FinishedJob, JobInfo, JobTracker};
use pitcrew_core::layout::{self, LOG_FILE};
use pitcrew_core::logline::{classify, RunnerLogEvent};
use pitcrew_core::logs::{LogTail, RotationPolicy, MAX_QUERY_LEN};
use pitcrew_core::metrics::{UsagePoint, SAMPLE_EVERY};
use pitcrew_core::names::RunnerName;
use pitcrew_core::notify::{self, Notification};
use pitcrew_core::platform::Platform;
use pitcrew_core::process_track;
use pitcrew_core::protocol::{
    DiagnosticsSection, DiskUse, ErrorCode, ProcessList, Request, Response, RestartPlan, RunnerSnapshot, ServiceError, ServiceEvent,
    ServiceInfo, UpdateRecord,
};
use pitcrew_core::redact::redact;
use pitcrew_core::runner_config::ConfigEdit;
use pitcrew_core::self_update::{self, BinaryStamp, RestartDecision};
use pitcrew_core::session::{self, LaunchTarget, NoLaunch, SessionState};
use pitcrew_core::state::{self, ServiceState, MAX_STATE_BYTES};
use pitcrew_core::time::Timestamp;
use pitcrew_core::update::{
    LocalMoment, TimeOfDay, UpdateCommand, UpdateDecision, UpdateEvent, UpdateMachine, UpdatePhase, UpdatePlan, Weekday,
};
use pitcrew_core::usage::{tree_members, Usage, UsageAcc, UsageTracker};
use pitcrew_core::version::RunnerVersion;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

mod config_requests;
mod holds;
mod ops;
mod processes;

/// The service's version, reported in the hello.
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");
/// The state file's name.
pub const STATE_FILE: &str = "state.json";
/// Lines kept per runner for the log view.
const TAIL_LINES: usize = 5000;
/// Bytes kept per runner for the log view.
const TAIL_BYTES: usize = 4 * 1024 * 1024;
/// How often processes are looked at (on the supervisor's clock; tests step it by this much to get one look).
pub const PROBE_EVERY: Duration = Duration::from_secs(5);
/// How often a runner folder is scanned for housekeeping.
const SCAN_EVERY: Duration = Duration::from_secs(30 * 60);
/// A cleanup is not repeated sooner than this, even under disk pressure.
const CLEANUP_GAP: Duration = Duration::from_secs(10 * 60);
/// The state file is written at most this often.
const SAVE_EVERY: Duration = Duration::from_secs(15);

/// The answer channel of a request.
pub type Reply = tokio::sync::oneshot::Sender<Response>;

/// Everything the supervisor reacts to.
pub enum Msg {
    /// A request from the app.
    Request(Box<Request>, Reply),
    /// A subscriber for pushed events.
    Subscribe(tokio::sync::mpsc::Sender<Response>),
    /// A line of a runner's output.
    Line {
        /// The runner.
        runner: RunnerName,
        /// Which process of it wrote the line.
        generation: u64,
        /// The line.
        text: String,
    },
    /// Slow work finished.
    Done {
        /// The runner.
        runner: RunnerName,
        /// The result.
        work: Done,
    },
    /// Someone logged on or off (Windows session change).
    LogonChanged,
    /// Stop supervising (the service is stopping).
    Shutdown,
}

/// Results of worker threads.
pub enum Done {
    /// A release check.
    Checked(Result<RunnerVersion, String>),
    /// The installed binary's version.
    Version(Option<RunnerVersion>),
    /// The download.
    Downloaded(Result<(), String>),
    /// The verification (a note on success).
    Verified(Result<String, String>),
    /// The swap.
    Swapped(Result<(), String>),
    /// Putting the previous binary back.
    Restored(Result<(), String>),
    /// A folder scan.
    Scanned(DiskScan, Option<Reply>),
    /// A cleanup.
    Cleaned(CleanupReport, Option<Reply>),
    /// A registration.
    Registered(Result<work::Registered, String>, Reply),
}

/// What the supervisor needs from its surroundings (all replaceable in tests).
pub struct Env {
    /// The data folder.
    pub data_dir: PathBuf,
    /// Starts runner processes.
    pub launcher: Box<dyn Launcher>,
    /// Release information and downloads.
    pub sources: Sources,
    /// This machine.
    pub platform: Platform,
    /// The `PATH` to give runners at each start (the user's login shell's on macOS and Linux); `None`: the service's own.
    pub base_path: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    /// A fingerprint of the service's own program file, or `None` when it cannot be told (or on Windows, where the
    /// app or installer restarts the service): a change makes an idle service restart itself (`self_update`).
    pub exe_stamp: Arc<dyn Fn() -> Option<BinaryStamp> + Send + Sync>,
    /// The clock.
    pub clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
    /// Local time: weekday, time of day and the UTC offset in minutes.
    pub local: Arc<dyn Fn(Timestamp) -> (LocalMoment, i32) + Send + Sync>,
}

/// The local time of the machine.
pub fn system_local(t: Timestamp) -> (LocalMoment, i32) {
    use chrono::{Datelike, Local, TimeZone, Timelike};
    let ms = i64::try_from(t.0).unwrap_or(i64::MAX);
    let dt = Local.timestamp_millis_opt(ms).single().unwrap_or_else(Local::now);
    let weekday = match dt.weekday() {
        chrono::Weekday::Mon => Weekday::Mon,
        chrono::Weekday::Tue => Weekday::Tue,
        chrono::Weekday::Wed => Weekday::Wed,
        chrono::Weekday::Thu => Weekday::Thu,
        chrono::Weekday::Fri => Weekday::Fri,
        chrono::Weekday::Sat => Weekday::Sat,
        chrono::Weekday::Sun => Weekday::Sun,
    };
    let time =
        TimeOfDay::new(u16::try_from(dt.hour()).unwrap_or(0), u16::try_from(dt.minute()).unwrap_or(0)).unwrap_or_else(unreachable_time);
    (LocalMoment { weekday, time }, dt.offset().local_minus_utc() / 60)
}

#[allow(clippy::unwrap_used)]
fn unreachable_time() -> TimeOfDay {
    // 00:00 is always valid.
    TimeOfDay::new(0, 0).unwrap()
}

/// The real environment.
pub fn os_env(data_dir: PathBuf) -> Env {
    let exe_stamp: Arc<dyn Fn() -> Option<BinaryStamp> + Send + Sync> = if cfg!(windows) {
        Arc::new(|| None)
    } else {
        let exe = std::env::current_exe().ok();
        Arc::new(move || {
            let meta = std::fs::metadata(exe.as_ref()?).ok()?;
            let modified_ms = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
            Some(BinaryStamp {
                len: meta.len(),
                modified_ms: u64::try_from(modified_ms).ok()?,
            })
        })
    };
    let base_path: Arc<dyn Fn() -> Option<String> + Send + Sync> = if cfg!(windows) {
        Arc::new(|| None)
    } else {
        let login = crate::login_env::LoginPath::new();
        Arc::new(move || Some(login.get()))
    };
    Env {
        base_path,
        exe_stamp,
        data_dir,
        launcher: Box::new(crate::launcher::OsLauncher),
        sources: Sources::public(),
        platform: Platform::current().unwrap_or(Platform {
            os: pitcrew_core::platform::Os::Linux,
            arch: pitcrew_core::platform::Arch::Amd64,
        }),
        clock: Arc::new(cleanup::now),
        local: Arc::new(system_local),
    }
}

/// An update step waiting for the runner to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Deferred {
    None,
    /// Put the previous binary back once the runner has stopped.
    Restore,
    /// Report "rolled back" once the restored runner has started.
    RolledBackOnStart,
}

struct Runner {
    cfg: RunnerConfig,
    ctl: Controller,
    jobs: JobTracker,
    tail: LogTail,
    log: LogWriter,
    process: Option<Box<dyn RunnerProcess>>,
    launched_as: Option<LaunchTarget>,
    generation: u64,
    update: UpdateMachine,
    update_worker: bool,
    watch_idle: bool,
    deferred: Deferred,
    current_version: Option<RunnerVersion>,
    registered: bool,
    last_status: Option<RunnerStatus>,
    scan: Option<DiskScan>,
    scan_at: Option<Timestamp>,
    scanning: bool,
    cleaning: bool,
    clean_wanted: bool,
    disk_low: bool,
    checking: bool,
    removing: bool,
    session_problem: Option<String>,
    /// CPU time of the runner's processes between probe ticks (ADR 0007).
    usage_tracker: UsageTracker,
    /// The runner's use at the last probe tick; `None` while it is not running.
    usage_now: Option<Usage>,
    /// The probe ticks of the current sample period.
    usage_period: UsageAcc,
    /// The runner's processes: jobs, leftovers and stops in progress (ADR 0012).
    watch: ProcessWatch,
    /// After a configuration change: the labels the runner must declare when it starts again (ADR 0011).
    label_check: Option<LabelCheck>,
    /// Whether the pause in force is Pitcrew's own hold (ADR 0013).
    hold: pitcrew_core::hold::HoldTracker,
    /// The low-disk hold is on (hysteresis).
    disk_hold: bool,
    /// Why the runner is held now.
    hold_reasons: Vec<pitcrew_core::hold::HoldReason>,
    /// Minutes until the open pause window closes.
    hold_minutes_left: Option<u32>,
    /// The reason of the hold Pitcrew applied last (for the release record).
    last_hold_reason: Option<pitcrew_core::hold::HoldReason>,
}

/// Labels to verify when the restarted runner declares itself.
struct LabelCheck {
    /// The label names the file asks for.
    expected: Vec<String>,
    /// Only a runner process started after this generation counts (the one that read the new file).
    after_generation: u64,
}

/// The supervisor.
pub struct Supervisor {
    env: Env,
    tx: Sender<Msg>,
    config: Config,
    config_problems: Vec<String>,
    read_only: bool,
    state: ServiceState,
    runners: BTreeMap<RunnerName, Runner>,
    probe: Probe,
    subscribers: Vec<tokio::sync::mpsc::Sender<Response>>,
    last_status: String,
    started_at: Timestamp,
    last_probe: Timestamp,
    started_stamp: Option<BinaryStamp>,
    last_exe_check: Timestamp,
    exe_wait_logged: bool,
    last_sample: Timestamp,
    /// All runners together, per probe tick, for the current sample period.
    usage_total: UsageAcc,
    last_save: Timestamp,
    dirty: bool,
    service_log: LogWriter,
    console: Option<u32>,
    pending_notes: Vec<Notification>,
    /// Asks processes to stop (replaceable in tests of the supervisor's own logic).
    terminator: Box<dyn Terminator>,
    /// Machine-wide hold inputs (ADR 0013).
    holds: holds::Holds,
}

fn err(code: ErrorCode, message: impl Into<String>) -> Response {
    Response::Error(ServiceError::new(code, message))
}

impl Supervisor {
    /// A supervisor over the configuration and state in `env.data_dir`. `tx` is the sending side of its channel.
    pub fn new(env: Env, tx: Sender<Msg>) -> Self {
        let now = (env.clock)();
        let env_stamp = env.exe_stamp.clone();
        let mut service_log = LogWriter::open(&env.data_dir.join("logs"), "service.log", RotationPolicy::default());
        service_log.supervisor(now, &format!("Pitcrew service {SERVICE_VERSION} starting"));

        let loaded = match load_config(&env.data_dir) {
            Ok(l) => l,
            Err(e) => {
                service_log.supervisor(now, &format!("configuration: {e}"));
                config::load(None, None)
            }
        };
        let cfg_path = env.data_dir.join(crate::paths::CONFIG_FILE);
        if loaded.set_aside_primary {
            let _ = store::set_aside(&cfg_path, now.0);
        }
        let config_problems: Vec<String> = loaded.problems.iter().map(ToString::to_string).collect();
        for p in &config_problems {
            service_log.supervisor(now, &format!("configuration: {p}"));
        }
        if loaded.needs_save && !loaded.read_only {
            if let Ok(text) = config::to_toml(&loaded.config) {
                if let Err(e) = store::write_atomic(&cfg_path, &text) {
                    service_log.supervisor(now, &format!("saving the configuration failed: {e}"));
                }
            }
        }
        let state_path = env.data_dir.join(STATE_FILE);
        let primary = store::read_bounded(&state_path, MAX_STATE_BYTES).unwrap_or(None);
        let backup = store::read_bounded(&store::sibling(&state_path, ".bak"), MAX_STATE_BYTES).unwrap_or(None);
        let loaded_state = state::load(primary.as_deref(), backup.as_deref());
        for p in &loaded_state.problems {
            service_log.supervisor(now, &format!("state: {p}"));
        }
        let console = env.launcher.console().session;
        let mut sup = Supervisor {
            tx,
            config: loaded.config.clone(),
            config_problems,
            read_only: loaded.read_only,
            state: loaded_state.state,
            runners: BTreeMap::new(),
            probe: Probe::new(),
            subscribers: vec![],
            last_status: String::new(),
            started_at: now,
            last_probe: Timestamp(0),
            started_stamp: (env_stamp)(),
            last_exe_check: now,
            exe_wait_logged: false,
            last_sample: Timestamp(0),
            usage_total: UsageAcc::default(),
            last_save: now,
            dirty: false,
            service_log,
            console,
            pending_notes: vec![],
            terminator: Box::new(OsTerminator),
            holds: holds::Holds::default(),
            env,
        };
        for r in loaded.config.runners {
            sup.add(r, now);
        }
        sup
    }

    fn settings_for(&self, r: &RunnerConfig) -> ControllerSettings {
        ControllerSettings {
            restart: self.config.defaults.restart,
            stall: r.stall_rule(&self.config.defaults),
            stop_timeout: Duration::from_secs(self.config.defaults.update_timeouts.stop_secs.max(10)),
        }
    }

    fn binary(&self, r: &RunnerConfig) -> layout::RunnerBinary {
        layout::binary(r, self.env.platform.os, |p| p.exists())
    }

    fn add(&mut self, cfg: RunnerConfig, now: Timestamp) {
        let st = self.state.runners.entry(cfg.name.to_string()).or_default().clone();
        let settings = self.settings_for(&cfg);
        let wanted = cfg.enabled && st.wanted;
        let mut log = LogWriter::open(&layout::log_dir(&cfg), LOG_FILE, self.config.defaults.logs);
        log.supervisor(now, "Pitcrew is supervising this runner");
        let registered = layout::registration(&cfg).exists();
        let name = cfg.name.clone();
        let mut runner = Runner {
            ctl: Controller::new(settings, wanted, st.paused),
            jobs: JobTracker::new(),
            tail: LogTail::new(TAIL_LINES, TAIL_BYTES),
            log,
            process: None,
            launched_as: None,
            generation: 0,
            update: UpdateMachine::new(self.config.defaults.update_timeouts),
            update_worker: false,
            watch_idle: false,
            deferred: Deferred::None,
            current_version: None,
            registered,
            last_status: None,
            scan: None,
            scan_at: None,
            scanning: false,
            cleaning: false,
            clean_wanted: false,
            disk_low: false,
            checking: false,
            removing: false,
            session_problem: None,
            usage_tracker: UsageTracker::new(),
            usage_now: None,
            usage_period: UsageAcc::default(),
            watch: ProcessWatch::new(),
            label_check: None,
            hold: st.hold,
            disk_hold: false,
            hold_reasons: Vec::new(),
            hold_minutes_left: None,
            last_hold_reason: None,
            cfg,
        };
        runner.watch.restore_keeps(&st.keeps);
        // A runner the previous service instance started and that still runs: adopt it instead of starting a second.
        let bin = self.binary(&runner.cfg);
        self.probe.refresh_processes();
        if let (Some(pid), Some(last)) = (self.probe.running_binary(&bin.path, None), st.pid) {
            // By identity, from the fresh look at the process table just taken (its executable is the runner's binary).
            if let (true, Some(id)) = (pid == last, paddock_proc::ProcessId::of(pid)) {
                runner.log.supervisor(
                    now,
                    &format!("adopted the runner left running by the previous service (pid {pid}); its output is visible again after its next restart"),
                );
                runner.ctl.handle(Input::Tick, now);
                runner.process = Some(Box::new(Adopted { id }));
                runner.launched_as = Some(LaunchTarget::ServiceAccount);
                runner.ctl.handle(Input::Launched, now);
                runner.ctl.handle(Input::Log(RunnerLogEvent::Declared), now);
                runner.ctl.handle(Input::Restart, now);
            }
        }
        self.runners.insert(name.clone(), runner);
        self.spawn_version_probe(&name);
    }

    // ---- the loop ------------------------------------------------------------------------------------------------

    /// Runs until [`Msg::Shutdown`] (or every sender is gone).
    pub fn run(mut self, rx: Receiver<Msg>) {
        let mut last_tick = (self.env.clock)();
        if cfg!(target_os = "macos") && !crate::launch::disclaim_supported() {
            self.log_service(
                last_tick,
                "this macOS does not offer responsibility_spawnattrs_setdisclaim: runners are started the old way, so macOS may \
                 blame Pitcrew.app for the protected folders a CI job touches (privacy prompts)",
            );
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Msg::Shutdown) => break,
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            let now = (self.env.clock)();
            if now.since(last_tick) >= Duration::from_secs(1) {
                last_tick = now;
                self.tick(now);
            }
            if self.restart_for_new_program(now) {
                break;
            }
        }
        self.shutdown();
    }

    /// Whether the service should exit now because its own program was replaced on disk (checked every 30 s). The
    /// service manager starts the new program, which adopts the runners; a running job or update postpones it.
    pub fn restart_for_new_program(&mut self, now: Timestamp) -> bool {
        if now.since(self.last_exe_check) < Duration::from_secs(30) {
            return false;
        }
        self.last_exe_check = now;
        let busy = self.runners.values().any(|r| r.jobs.running(now) || r.update_worker);
        match self_update::decide(self.started_stamp, (self.env.exe_stamp)(), busy) {
            RestartDecision::Stay => false,
            RestartDecision::WaitForIdle => {
                if !self.exe_wait_logged {
                    self.exe_wait_logged = true;
                    self.log_service(now, "the Pitcrew service program was replaced: restarting when no job runs");
                }
                false
            }
            RestartDecision::Restart => {
                self.log_service(now, "the Pitcrew service program was replaced: restarting to run the new one");
                true
            }
        }
    }

    /// Handles one message.
    pub fn handle(&mut self, msg: Msg) {
        let now = (self.env.clock)();
        match msg {
            Msg::Request(req, reply) => {
                if let Some((reply, resp)) = self.request(*req, reply, now) {
                    let _ = reply.send(resp);
                }
            }
            Msg::Subscribe(sub) => {
                let status = self.status_event(now);
                if sub.try_send(Response::Event { event: status }).is_ok() {
                    self.subscribers.push(sub);
                }
            }
            Msg::Line { runner, generation, text } => self.line(&runner, generation, text, now),
            Msg::Done { runner, work } => self.done(&runner, work, now),
            Msg::LogonChanged => self.logon_changed(now),
            Msg::Shutdown => {}
        }
        self.publish(now);
    }

    /// One tick of time.
    pub fn tick(&mut self, now: Timestamp) {
        let probe = now.since(self.last_probe) >= PROBE_EVERY;
        if probe {
            self.last_probe = now;
            self.probe.refresh_processes();
            self.sample_usage(now);
            let held: Vec<RunnerName> = self.runners.keys().cloned().collect();
            for name in &held {
                self.tick_hold(name, now);
            }
            let console = self.env.launcher.console().session;
            if console != self.console {
                self.console = console;
                self.logon_changed(now);
            }
        }
        let names: Vec<RunnerName> = self.runners.keys().cloned().collect();
        for name in &names {
            self.tick_runner(name, now, probe);
        }
        if now.since(self.last_sample) >= SAMPLE_EVERY {
            self.last_sample = now;
            let mut sample = self.probe.machine(now, &self.env.data_dir);
            sample.runners = self.usage_total.take().unwrap_or_default();
            self.observe_idle(now, sample.cpu_permille, sample.runners.cpu_permille);
            self.state.metrics.push(sample);
            for r in self.runners.values_mut() {
                if let Some(usage) = r.usage_period.take() {
                    self.state
                        .runners
                        .entry(r.cfg.name.to_string())
                        .or_default()
                        .usage
                        .push(UsagePoint { at: now, usage });
                }
            }
            self.dirty = true;
        }
        if self.dirty && now.since(self.last_save) >= SAVE_EVERY {
            self.save_state(now);
        }
        self.publish(now);
    }

    /// Reads the process table once and records what each runner's process tree uses (ADR 0007), and what its
    /// processes are and which ones a finished job left behind (ADR 0012).
    fn sample_usage(&mut self, now: Timestamp) {
        let table = self.probe.process_table();
        let cpus = self.probe.cpus();
        let mut all = Vec::with_capacity(self.runners.len());
        for r in self.runners.values_mut() {
            let root = r.process.as_ref().map(|p| p.pid());
            let members = root.map(|pid| tree_members(pid, &table)).unwrap_or_default();
            let Some(root) = root.filter(|_| !members.is_empty()) else {
                r.usage_tracker.reset();
                r.usage_now = None;
                r.watch.clear();
                continue;
            };
            let usage = r.usage_tracker.observe(now.0, &members, cpus);
            r.usage_period.add(&usage);
            r.usage_now = Some(usage);
            r.jobs.observe_usage(usage.cpu_permille, usage.mem_bytes);
            all.push(usage);
            let metas = self.probe.process_metas(root, &members, &r.usage_tracker);
            let st = self.state.runners.get(r.cfg.name.as_str());
            let finished = st
                .map(|st| st.history.recent_jobs(process_track::MAX_JOB_SPANS))
                .unwrap_or_default();
            let spans = process_track::spans(finished, r.jobs.current());
            let policy = r.cfg.process_policy(&self.config.defaults);
            r.watch.refresh(now, root, &metas, &spans, &policy);
        }
        self.usage_total.add(&Usage::sum(all.iter()));
    }

    fn tick_runner(&mut self, name: &RunnerName, now: Timestamp, probe: bool) {
        // Exits.
        let exited = self
            .runners
            .get_mut(name)
            .and_then(|r| r.process.as_mut().and_then(|p| p.try_wait()));
        if let Some(code) = exited {
            self.on_exit(name, code, now);
        }
        let Some(r) = self.runners.get_mut(name) else { return };
        if probe {
            r.registered = layout::registration(&r.cfg).exists();
            if let Some(p) = &r.process {
                let busy = self.probe.has_job_children(p.pid());
                if let Some(mut job) = r.jobs.observe_children(busy, now) {
                    job.leftovers = r.watch.survivors(&job);
                    record_job(&mut self.state, &r.cfg.name, job, now);
                    self.dirty = true;
                }
            }
        }
        let running = r.jobs.running(now);
        r.ctl.handle(Input::JobRunning(running), now);
        let actions = r.ctl.handle(Input::Tick, now);
        self.act(name, actions, now);
        self.tidy_processes(name, now, probe);

        // Updates.
        let Some(r) = self.runners.get_mut(name) else { return };
        if r.watch_idle {
            if !r.ctl.process_alive() {
                r.watch_idle = false;
                self.update_event(name, UpdateEvent::RunnerStopped, now);
            } else if !r.jobs.running(now) {
                r.watch_idle = false;
                self.update_event(name, UpdateEvent::RunnerIdle, now);
            }
        }
        let Some(r) = self.runners.get_mut(name) else { return };
        if r.deferred == Deferred::Restore && !r.ctl.process_alive() {
            r.deferred = Deferred::None;
            self.spawn_restore(name);
        }
        self.update_event(name, UpdateEvent::Tick, now);
        if probe {
            self.watch_updates(name, now);
        }
        self.maybe_check(name, now);
        self.maybe_housekeep(name, now);

        // Removal finishes once the runner is down.
        if let Some(r) = self.runners.get(name) {
            if r.removing && !r.ctl.process_alive() {
                self.runners.remove(name);
                self.state.runners.remove(name.as_str());
                self.dirty = true;
                self.log_service(now, &format!("runner {name} removed"));
            }
        }
    }

    // ---- process life --------------------------------------------------------------------------------------------

    fn act(&mut self, name: &RunnerName, actions: Vec<Action>, now: Timestamp) {
        for a in actions {
            match a {
                Action::Launch => self.launch(name, now),
                Action::StopGracefully => {
                    if let Some(r) = self.runners.get_mut(name) {
                        r.log
                            .supervisor(now, "asking the runner to stop (it finishes a job it may have just taken)");
                        if let Some(p) = r.process.as_mut() {
                            if let Err(e) = p.stop_gracefully() {
                                r.log.supervisor(
                                    now,
                                    &format!("the stop request failed: {e}; Pitcrew ends the runner after the stop timeout"),
                                );
                            }
                        }
                    }
                }
                Action::Kill => {
                    if let Some(r) = self.runners.get_mut(name) {
                        r.log.supervisor(now, "the runner did not stop in time and runs no job; ending it");
                        if let Some(p) = r.process.as_mut() {
                            p.kill();
                        }
                    }
                }
                Action::Record(kind) => self.record(name, kind, now),
            }
        }
    }

    fn record(&mut self, name: &RunnerName, kind: HistoryKind, now: Timestamp) {
        if let Some(r) = self.runners.get_mut(name) {
            let line = match &kind {
                HistoryKind::Stalled { timeouts } => Some(format!(
                    "runner stalled ({timeouts} failed polls for jobs in the watchdog window, no job running); restarting it"
                )),
                HistoryKind::CrashLoop { exits } => Some(format!(
                    "the runner exited {exits} times in a short time; restarts are paused (start it from Pitcrew to try again)"
                )),
                HistoryKind::LaunchFailed { reason } => Some(format!("the runner could not be started: {reason}")),
                HistoryKind::Paused => Some("paused for maintenance".to_string()),
                HistoryKind::Stopped => Some("stopped".to_string()),
                _ => None,
            };
            if let Some(l) = line {
                r.log.supervisor(now, &l);
            }
        }
        let st = self.state.runners.entry(name.to_string()).or_default();
        match kind {
            HistoryKind::Stopped => st.wanted = false,
            HistoryKind::Paused => st.paused = true,
            HistoryKind::Resumed | HistoryKind::Started => {
                st.wanted = true;
                st.paused = false;
            }
            _ => {}
        }
        st.history.push(now, kind);
        self.dirty = true;
    }

    fn launch(&mut self, name: &RunnerName, now: Timestamp) {
        let console = self.env.launcher.console().session;
        let Some(r) = self.runners.get(name) else { return };
        let bin = self.binary(&r.cfg);
        let failure = |reason: String| Input::LaunchFailed(LaunchError::Failed { reason });
        let input = if !bin.path.exists() {
            Some(failure(format!(
                "the runner binary {} is missing; register the runner from Pitcrew (it installs the binary) or put it there",
                bin.path.display()
            )))
        } else if !layout::registration(&r.cfg).exists() {
            Some(failure(
                "the runner is not registered with a server yet; register it from Pitcrew".into(),
            ))
        } else if let Some(pid) = self.probe.running_binary(&bin.path, None) {
            Some(failure(format!(
                "a runner from this folder already runs outside Pitcrew (process {pid}); stop the old supervisor (e.g. the GiteaRunnerDesktop scheduled task) so Pitcrew can start its own"
            )))
        } else {
            match session::launch_target(r.cfg.session, r.cfg.logged_off, console) {
                Err(NoLaunch::WaitingForLogon) => Some(Input::LaunchFailed(LaunchError::WaitingForLogon)),
                Ok(target) => {
                    let spec = LaunchSpec {
                        program: bin.path.clone(),
                        args: layout::daemon_args(&layout::runner_config(&r.cfg)),
                        dir: r.cfg.dir.clone(),
                        env: pitcrew_core::runner_env::runner_environment((self.env.base_path)().as_deref(), &r.cfg.env),
                    };
                    let generation = r.generation + 1;
                    let tx = self.tx.clone();
                    let runner = name.clone();
                    let sink: LineSink = Arc::new(move |text| {
                        let _ = tx.send(Msg::Line {
                            runner: runner.clone(),
                            generation,
                            text,
                        });
                    });
                    match self.env.launcher.launch(target, &spec, sink) {
                        Ok(p) => {
                            let pid = p.pid();
                            if let Some(r) = self.runners.get_mut(name) {
                                r.generation = generation;
                                r.process = Some(p);
                                r.launched_as = Some(target);
                                r.session_problem = None;
                                let where_ = match target {
                                    LaunchTarget::UserSession { session_id } => format!("in the user's session {session_id}"),
                                    LaunchTarget::ServiceAccount => "as the service account".to_string(),
                                };
                                r.log.supervisor(now, &format!("started the runner {where_} (process {pid})"));
                            }
                            self.state.runners.entry(name.to_string()).or_default().pid = Some(pid);
                            Some(Input::Launched)
                        }
                        Err(e) => {
                            if let Some(r) = self.runners.get_mut(name) {
                                r.session_problem = Some(e.clone());
                            }
                            Some(failure(e))
                        }
                    }
                }
            }
        };
        if let (Some(input), Some(r)) = (input, self.runners.get_mut(name)) {
            let launched = input == Input::Launched;
            let actions = r.ctl.handle(input, now);
            self.act(name, actions, now);
            if launched {
                if let Some(r) = self.runners.get_mut(name) {
                    if r.deferred == Deferred::RolledBackOnStart {
                        r.deferred = Deferred::None;
                        self.update_event(name, UpdateEvent::RolledBack, now);
                    }
                }
            } else if let Some(r) = self.runners.get_mut(name) {
                // A launch the update machine asked for failed.
                if r.deferred == Deferred::RolledBackOnStart {
                    r.deferred = Deferred::None;
                    let reason = r.ctl.problem().unwrap_or("the previous version did not start").to_string();
                    self.update_event(name, UpdateEvent::RollbackFailed { reason }, now);
                } else if matches!(r.update.phase(), UpdatePhase::Confirming { .. }) {
                    self.update_event(name, UpdateEvent::RunnerExited, now);
                }
            }
        }
    }

    fn on_exit(&mut self, name: &RunnerName, code: Option<i32>, now: Timestamp) {
        let Some(r) = self.runners.get_mut(name) else { return };
        r.process = None;
        r.launched_as = None;
        r.log.supervisor(
            now,
            &format!("the runner exited with code {}", code.map_or("unknown".into(), |c| c.to_string())),
        );
        if let Some(job) = r.jobs.process_exited(now) {
            record_job(&mut self.state, name, job, now);
        }
        self.state.runners.entry(name.to_string()).or_default().pid = None;
        let actions = r.ctl.handle(Input::Exited { code }, now);
        if let Life::WaitingToStart { at } = r.ctl.life() {
            r.log.supervisor(now, &format!("restarting in {} s", at.since(now).as_secs()));
        }
        self.act(name, actions, now);
        if let Some(r) = self.runners.get(name) {
            match r.update.phase() {
                UpdatePhase::Stopping { .. } => self.update_event(name, UpdateEvent::RunnerStopped, now),
                UpdatePhase::Confirming { .. } => self.update_event(name, UpdateEvent::RunnerExited, now),
                _ => {}
            }
        }
        self.dirty = true;
    }

    fn line(&mut self, name: &RunnerName, generation: u64, text: String, now: Timestamp) {
        let Some(r) = self.runners.get_mut(name) else { return };
        if generation != r.generation {
            return;
        }
        r.log.write_line(&text);
        r.tail.push(&text);
        let event = classify(&text);
        if event == RunnerLogEvent::TaskStarted {
            if let Some(mut job) = r.jobs.task_started(JobInfo::from_task_started(&text), now) {
                job.leftovers = r.watch.survivors(&job);
                record_job(&mut self.state, name, job, now);
                self.dirty = true;
            }
        }
        let declared = event == RunnerLogEvent::Declared;
        let declared_labels = if declared {
            pitcrew_core::logline::declared_labels(&text)
        } else {
            None
        };
        let actions = r.ctl.handle(Input::Log(event), now);
        self.act(name, actions, now);
        if let Some(labels) = declared_labels {
            self.verify_labels(name, &labels, generation, now);
        }
        if declared {
            if let Some(r) = self.runners.get(name) {
                if matches!(r.update.phase(), UpdatePhase::Confirming { .. }) {
                    self.update_event(name, UpdateEvent::Accepted, now);
                }
            }
        }
    }

    fn logon_changed(&mut self, now: Timestamp) {
        self.console = self.env.launcher.console().session;
        let console = self.console;
        let names: Vec<RunnerName> = self.runners.keys().cloned().collect();
        for name in names {
            let Some(r) = self.runners.get_mut(&name) else { continue };
            let mut actions = vec![];
            if console.is_some() {
                actions.extend(r.ctl.handle(Input::LogonAvailable, now));
            }
            if let Some(target) = r.launched_as {
                if session::should_relocate(r.cfg.session, r.cfg.logged_off, target, console) {
                    r.log.supervisor(
                        now,
                        "the logged-on user changed; the runner moves to the right session after its current job",
                    );
                    actions.extend(r.ctl.handle(Input::Relocate, now));
                }
            }
            self.act(&name, actions, now);
        }
    }

    // ---- updates -------------------------------------------------------------------------------------------------

    fn local(&self, now: Timestamp) -> (LocalMoment, i32) {
        (self.env.local)(now)
    }

    fn decision(&self, r: &Runner, now: Timestamp) -> Option<UpdateDecision> {
        let st = self.state.runners.get(r.cfg.name.as_str());
        let policy = r.cfg.update_policy(&self.config.defaults);
        let latest = policy.pinned.clone().or_else(|| st.and_then(|s| s.latest.clone()))?;
        Some(policy.decide(r.current_version.as_ref(), &latest, self.local(now).0))
    }

    /// The update loop of a runner, when its history shows one.
    fn update_loop_of(&self, name: &str, now: Timestamp) -> Option<pitcrew_core::update::watch::UpdateLoop> {
        let st = self.state.runners.get(name)?;
        let outcomes = st.history.events().filter_map(|e| match &e.kind {
            HistoryKind::UpdateFinished { outcome } => Some((e.at, outcome)),
            _ => None,
        });
        pitcrew_core::update::watch::detect_loop(outcomes, now)
    }

    /// The decision for updates Pitcrew starts by itself: a target that goes round in circles is not tried again (the
    /// owner can still install it by hand), it is offered instead.
    fn auto_decision(&self, r: &Runner, now: Timestamp) -> Option<UpdateDecision> {
        let d = self.decision(r, now)?;
        match (&d, self.update_loop_of(r.cfg.name.as_str(), now)) {
            (UpdateDecision::Apply { target } | UpdateDecision::WaitForWindow { target }, Some(l)) if &l.target == target => {
                Some(UpdateDecision::Available {
                    target: target.clone(),
                    notify: false,
                })
            }
            _ => Some(d),
        }
    }

    /// Keeps the "behind since" mark, and announces a deadline that is near or passed and a loop (each once).
    fn watch_updates(&mut self, name: &RunnerName, now: Timestamp) {
        use pitcrew_core::update::watch::{deadline, next_behind_since, DeadlineState};
        let Some(r) = self.runners.get(name) else { return };
        let policy = r.cfg.update_policy(&self.config.defaults).clone();
        let current = r.current_version.clone();
        let l = self.update_loop_of(name.as_str(), now);
        let st = self.state.runners.entry(name.to_string()).or_default();
        let Some(latest) = st.latest.clone() else { return };
        let behind = if policy.pinned.is_some() {
            None
        } else {
            next_behind_since(st.behind_since, current.as_ref(), &latest, now)
        };
        if behind != st.behind_since {
            st.behind_since = behind;
            st.deadline_notified = 0;
            self.dirty = true;
        }
        let d = deadline(policy.deadline_days, behind, now);
        let mut notes = Vec::new();
        if let Some(d) = d {
            let level = match d.state {
                DeadlineState::Ok => 0,
                DeadlineState::Soon => 1,
                DeadlineState::Overdue => 2,
            };
            if level > st.deadline_notified {
                st.deadline_notified = level;
                self.dirty = true;
                notes.push(notify::update_deadline(name, &latest, &d, now));
            }
        }
        if let Some(l) = l {
            if st.loop_notified.as_ref() != Some(&l.target) {
                st.loop_notified = Some(l.target.clone());
                self.dirty = true;
                notes.push(notify::update_loop(name, &l, now));
            }
        }
        for n in notes {
            self.notify(n);
        }
    }

    fn runner_policy_deadline(&self, r: &Runner) -> u32 {
        r.cfg.update_policy(&self.config.defaults).deadline_days
    }

    fn maybe_check(&mut self, name: &RunnerName, now: Timestamp) {
        let Some(r) = self.runners.get(name) else { return };
        if r.checking || !matches!(r.update.phase(), UpdatePhase::Idle) {
            return;
        }
        let policy = r.cfg.update_policy(&self.config.defaults).clone();
        let checked = self.state.runners.get(name.as_str()).and_then(|s| s.checked_at);
        // A little jitter per runner (its name) so many machines don't ask at the same second.
        let jitter = Duration::from_secs(u64::from(name.as_str().bytes().map(u32::from).sum::<u32>() % 600));
        let every = Duration::from_secs(u64::from(policy.check_every_hours.max(1)) * 3600) + jitter;
        let due = checked.is_none_or(|t| now.since(t) >= every);
        if policy.scheduled_checks() && due && r.cfg.binary_path.is_none() {
            self.spawn_check(name);
        } else if let Some(UpdateDecision::Apply { target }) = self.auto_decision(r, now) {
            // The window opened (or a pin was set) since the check.
            if layout::binary(&r.cfg, self.env.platform.os, |p| p.exists()).managed && r.current_version.is_some() {
                self.begin_update(name, target, now);
            }
        }
    }

    fn spawn_check(&mut self, name: &RunnerName) {
        let Some(r) = self.runners.get_mut(name) else { return };
        r.checking = true;
        let kind = r.cfg.kind;
        let channel = r.cfg.update_policy(&self.config.defaults).channel;
        let src = self.env.sources.clone();
        self.worker(name, move || Done::Checked(work::latest(&src, kind, channel)));
    }

    fn spawn_version_probe(&mut self, name: &RunnerName) {
        let Some(r) = self.runners.get(name) else { return };
        let bin = self.binary(&r.cfg).path;
        self.worker(name, move || {
            Done::Version(if bin.exists() { work::binary_version(&bin) } else { None })
        });
    }

    fn worker(&self, name: &RunnerName, f: impl FnOnce() -> Done + Send + 'static) {
        let tx = self.tx.clone();
        let runner = name.clone();
        let spawned = std::thread::Builder::new().name(format!("work-{runner}")).spawn(move || {
            let work = f();
            let _ = tx.send(Msg::Done { runner, work });
        });
        // Without a thread the work never reports back; the next tick or request tries again.
        drop(spawned);
    }

    fn begin_update(&mut self, name: &RunnerName, to: RunnerVersion, now: Timestamp) {
        let Some(r) = self.runners.get(name) else { return };
        if !matches!(r.update.phase(), UpdatePhase::Idle) {
            return;
        }
        let plan = UpdatePlan {
            from: r.current_version.clone(),
            to,
        };
        if let Some(r) = self.runners.get_mut(name) {
            r.log.supervisor(
                now,
                &format!(
                    "updating {} → {} (between jobs)",
                    plan.from.as_ref().map_or("?".into(), ToString::to_string),
                    plan.to
                ),
            );
        }
        self.update_event(name, UpdateEvent::Begin { plan }, now);
    }

    fn update_event(&mut self, name: &RunnerName, event: UpdateEvent, now: Timestamp) {
        let Some(r) = self.runners.get_mut(name) else { return };
        let commands = match r.update.handle(event, now) {
            Ok(c) => c,
            Err(e) => {
                r.log.supervisor(now, &format!("update: {e}"));
                return;
            }
        };
        for c in commands {
            self.update_command(name, c, now);
        }
    }

    fn update_command(&mut self, name: &RunnerName, c: UpdateCommand, now: Timestamp) {
        let Some(r) = self.runners.get_mut(name) else { return };
        let bin = layout::binary(&r.cfg, self.env.platform.os, |p| p.exists()).path;
        let kind = r.cfg.kind;
        let platform = self.env.platform;
        let src = self.env.sources.clone();
        match c {
            UpdateCommand::Download { version } => {
                r.update_worker = true;
                self.worker(name, move || Done::Downloaded(work::download(&src, kind, &version, platform, &bin)));
            }
            UpdateCommand::Verify => {
                let Some(version) = plan_of(r.update.phase()).map(|p| p.to.clone()) else {
                    return;
                };
                r.update_worker = true;
                self.worker(name, move || {
                    Done::Verified(work::verify_download(&src, kind, &version, platform, &bin))
                });
            }
            UpdateCommand::WatchForIdle => r.watch_idle = true,
            UpdateCommand::StopGracefully | UpdateCommand::StopRunner => {
                let mut actions = r.ctl.handle(Input::Hold(true), now);
                actions.extend(r.ctl.handle(Input::StopNow, now));
                self.act(name, actions, now);
            }
            UpdateCommand::Swap => {
                r.update_worker = true;
                self.worker(name, move || Done::Swapped(work::swap(&bin)));
            }
            UpdateCommand::StartRunner => {
                let mut actions = r.ctl.handle(Input::Hold(true), now);
                actions.extend(r.ctl.handle(Input::LaunchNow, now));
                self.act(name, actions, now);
            }
            UpdateCommand::RestorePrevious => {
                if r.ctl.process_alive() {
                    r.deferred = Deferred::Restore;
                } else {
                    self.spawn_restore(name);
                }
            }
            UpdateCommand::DiscardDownload => work::discard_download(&bin),
            UpdateCommand::Finish { outcome } => {
                r.log.supervisor(now, &format!("update finished: {}", outcome_text(&outcome)));
                let actions = r.ctl.handle(Input::Hold(false), now);
                let record = UpdateRecord {
                    outcome: outcome.clone(),
                    finished_at: now,
                };
                let st = self.state.runners.entry(name.to_string()).or_default();
                st.last_update = Some(record);
                st.history.push(now, HistoryKind::UpdateFinished { outcome: outcome.clone() });
                self.dirty = true;
                if let Some(n) = notify::update_finished(name, &outcome, now) {
                    self.notify(n);
                }
                self.act(name, actions, now);
                self.spawn_version_probe(name);
            }
        }
    }

    fn spawn_restore(&mut self, name: &RunnerName) {
        let Some(r) = self.runners.get(name) else { return };
        let bin = layout::binary(&r.cfg, self.env.platform.os, |p| p.exists()).path;
        self.worker(name, move || Done::Restored(work::restore_previous(&bin)));
    }

    fn done(&mut self, name: &RunnerName, work: Done, now: Timestamp) {
        let Some(r) = self.runners.get_mut(name) else {
            // The runner was removed meanwhile; answer waiting requests.
            match work {
                Done::Registered(_, reply) | Done::Scanned(_, Some(reply)) | Done::Cleaned(_, Some(reply)) => {
                    let _ = reply.send(err(ErrorCode::UnknownRunner, format!("{name} was removed")));
                }
                _ => {}
            }
            return;
        };
        match work {
            Done::Checked(result) => {
                r.checking = false;
                let st = self.state.runners.entry(name.to_string()).or_default();
                st.checked_at = Some(now);
                self.dirty = true;
                match result {
                    Ok(v) => {
                        st.latest = Some(v.clone());
                        let fresh = st.announced.as_ref() != Some(&v);
                        let Some(r) = self.runners.get(name) else { return };
                        let managed = self.binary(&r.cfg).managed;
                        let installed = r.current_version.is_some();
                        match self.auto_decision(r, now) {
                            Some(UpdateDecision::Apply { target }) if installed && managed => self.begin_update(name, target, now),
                            Some(UpdateDecision::Available { target, notify: true }) if fresh => {
                                self.state.runners.entry(name.to_string()).or_default().announced = Some(v);
                                self.notify(notify::update_available(name, &target, now));
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        if let Some(r) = self.runners.get_mut(name) {
                            r.log.supervisor(now, &format!("checking for a runner update failed: {e}"));
                        }
                    }
                }
            }
            Done::Version(v) => {
                if v.is_some() {
                    r.current_version = v;
                }
            }
            Done::Downloaded(res) => {
                r.update_worker = false;
                let ev = match res {
                    Ok(()) => UpdateEvent::Downloaded,
                    Err(reason) => UpdateEvent::DownloadFailed { reason },
                };
                self.update_event(name, ev, now);
            }
            Done::Verified(res) => {
                r.update_worker = false;
                let ev = match res {
                    Ok(note) => {
                        r.log.supervisor(now, &format!("update: {note}"));
                        UpdateEvent::Verified
                    }
                    Err(reason) => UpdateEvent::VerificationFailed { reason },
                };
                self.update_event(name, ev, now);
            }
            Done::Swapped(res) => {
                r.update_worker = false;
                let ev = match res {
                    Ok(()) => UpdateEvent::Swapped,
                    Err(reason) => UpdateEvent::SwapFailed { reason },
                };
                self.update_event(name, ev, now);
            }
            Done::Restored(res) => match res {
                Ok(()) => {
                    r.deferred = Deferred::RolledBackOnStart;
                    let actions = r.ctl.handle(Input::LaunchNow, now);
                    self.act(name, actions, now);
                }
                Err(reason) => self.update_event(name, UpdateEvent::RollbackFailed { reason }, now),
            },
            Done::Scanned(scan, reply) => {
                r.scanning = false;
                r.scan_at = Some(now);
                let policy = r.cfg.cleanup_policy(&self.config.defaults);
                let low = housekeeping::disk_low(&policy, scan.free_bytes);
                if low && !r.disk_low {
                    if let Some(free) = scan.free_bytes {
                        self.notify(notify::disk_low(name, free, now));
                    }
                }
                let Some(r) = self.runners.get_mut(name) else { return };
                r.disk_low = low;
                let st = self.state.runners.entry(name.to_string()).or_default();
                st.folder_sizes.record(now, scan.folder_bytes);
                self.dirty = true;
                if let Some(reply) = reply {
                    let preview = housekeeping::plan(&policy, &scan, CleanupTrigger::Manual, self.config.defaults.logs.keep, now);
                    let _ = reply.send(Response::Disk {
                        scan: scan.clone(),
                        preview,
                        last_cleanup: st.last_cleanup.clone(),
                        policy,
                    });
                }
                r.scan = Some(scan);
            }
            Done::Cleaned(report, reply) => {
                r.cleaning = false;
                r.scan_at = None; // rescan soon
                let failed = u32::try_from(report.failures.len()).unwrap_or(u32::MAX);
                r.log.supervisor(
                    now,
                    &format!(
                        "cleanup ({:?}): freed {}, deleted {}, {} could not be deleted",
                        report.trigger,
                        notify::human_bytes(report.freed_bytes),
                        report.deleted,
                        failed
                    ),
                );
                for f in report.failures.iter().take(10) {
                    r.log.supervisor(now, &format!("cleanup: {}: {}", f.path, f.reason));
                }
                let st = self.state.runners.entry(name.to_string()).or_default();
                st.history.push(
                    now,
                    HistoryKind::Cleanup {
                        trigger: report.trigger,
                        freed_bytes: report.freed_bytes,
                        deleted: report.deleted,
                        failed,
                    },
                );
                st.last_cleanup = Some(report.clone());
                self.dirty = true;
                if let Some(n) = notify::cleanup(name, &report) {
                    self.notify(n);
                }
                if let Some(reply) = reply {
                    let _ = reply.send(Response::Cleaned { report });
                }
            }
            Done::Registered(res, reply) => match res {
                Ok(how) => {
                    r.registered = true;
                    r.log.supervisor(
                        now,
                        match how {
                            work::Registered::New => "registered with the server",
                            work::Registered::Adopted => "adopted a folder that is registered already; the token was not used",
                        },
                    );
                    let actions = r.ctl.handle(Input::Start, now);
                    self.act(name, actions, now);
                    self.spawn_version_probe(name);
                    let _ = reply.send(Response::Ok);
                }
                Err(e) => {
                    r.log.supervisor(now, &format!("registration failed: {e}"));
                    let _ = reply.send(err(ErrorCode::BadRequest, e));
                }
            },
        }
    }

    // ---- housekeeping --------------------------------------------------------------------------------------------

    fn maybe_housekeep(&mut self, name: &RunnerName, now: Timestamp) {
        let Some(r) = self.runners.get(name) else { return };
        if r.scanning || r.cleaning {
            return;
        }
        let stale = r.scan_at.is_none_or(|t| now.since(t) >= SCAN_EVERY);
        if stale {
            self.spawn_scan(name, None);
            return;
        }
        let idle = !r.jobs.running(now) && matches!(r.update.phase(), UpdatePhase::Idle) && !r.update_worker;
        if !idle {
            return;
        }
        let Some(scan) = &r.scan else { return };
        let policy = r.cfg.cleanup_policy(&self.config.defaults);
        let last = self
            .state
            .runners
            .get(name.as_str())
            .and_then(|s| s.last_cleanup.as_ref().map(|c| c.at));
        let trigger = if r.clean_wanted {
            Some(CleanupTrigger::Manual)
        } else {
            housekeeping::due(&policy, scan, last, now, CLEANUP_GAP)
        };
        if let Some(trigger) = trigger {
            let plan = housekeeping::plan(&policy, scan, trigger, self.config.defaults.logs.keep, now);
            self.spawn_clean(name, plan, None);
        }
    }

    fn spawn_scan(&mut self, name: &RunnerName, reply: Option<Reply>) {
        let Some(r) = self.runners.get_mut(name) else { return };
        r.scanning = true;
        let cfg = r.cfg.clone();
        let bin = layout::binary(&cfg, self.env.platform.os, |p| p.exists()).path;
        let free = self.probe.disk_space(&cfg.dir, (self.env.clock)()).map(|(f, _)| f);
        self.worker(name, move || Done::Scanned(cleanup::scan(&cfg, &bin, free), reply));
    }

    fn spawn_clean(&mut self, name: &RunnerName, plan: CleanupPlan, reply: Option<Reply>) {
        let Some(r) = self.runners.get_mut(name) else { return };
        r.cleaning = true;
        r.clean_wanted = false;
        let cfg = r.cfg.clone();
        let clock = self.env.clock.clone();
        self.worker(name, move || {
            let report = cleanup::execute(&cfg, &plan, clock());
            Done::Cleaned(report, reply)
        });
    }

    // ---- requests ------------------------------------------------------------------------------------------------

    /// Answers a request; `None` when the answer comes later (from a worker).
    fn request(&mut self, req: Request, reply: Reply, now: Timestamp) -> Option<(Reply, Response)> {
        let resp = match req {
            Request::Hello { protocol } => pitcrew_core::protocol::answer_hello(protocol, SERVICE_VERSION),
            Request::Status => {
                let ServiceEvent::Status { runners, service } = self.status_event(now) else {
                    return Some((reply, err(ErrorCode::Internal, "no status")));
                };
                Response::Status { runners, service }
            }
            Request::Subscribe => err(ErrorCode::BadRequest, "subscribe on a connection of its own"),
            Request::Start { runner } => self.control(&runner, Input::Start, now),
            Request::Stop { runner } => self.control(&runner, Input::Stop, now),
            Request::Restart { runner } => self.control(&runner, Input::Restart, now),
            Request::Drain { runner } => self.control(&runner, Input::Drain, now),
            Request::Resume { runner } => self.control(&runner, Input::Resume, now),
            Request::Logs { runner, after, limit } => match self.runners.get(&runner) {
                Some(r) => Response::Logs {
                    lines: r.tail.since(after, limit.min(2000) as usize),
                },
                None => unknown(&runner),
            },
            Request::SearchLogs { runner, query, limit } => {
                if query.chars().count() > MAX_QUERY_LEN {
                    err(ErrorCode::BadRequest, format!("searches are at most {MAX_QUERY_LEN} characters"))
                } else {
                    match self.runners.get(&runner) {
                        Some(r) => Response::Logs {
                            lines: r.tail.search(&query, limit as usize),
                        },
                        None => unknown(&runner),
                    }
                }
            }
            Request::CheckUpdate { runner } => {
                if self.runners.contains_key(&runner) {
                    self.spawn_check(&runner);
                    Response::Ok
                } else {
                    unknown(&runner)
                }
            }
            Request::ApplyUpdate { runner } => self.apply_update(&runner, now),
            Request::CancelUpdate { runner } => match self.runners.get(&runner) {
                Some(r) if matches!(r.update.phase(), UpdatePhase::Idle) => err(ErrorCode::BadRequest, "no update is in progress"),
                Some(r)
                    if matches!(
                        r.update.phase(),
                        UpdatePhase::Swapping { .. } | UpdatePhase::Confirming { .. } | UpdatePhase::RollingBack { .. }
                    ) =>
                {
                    err(
                        ErrorCode::Busy,
                        "the new binary is already in place; the update finishes (or rolls back) by itself",
                    )
                }
                Some(_) => {
                    if let Some(r) = self.runners.get_mut(&runner) {
                        r.watch_idle = false;
                    }
                    self.update_event(&runner, UpdateEvent::Cancel, now);
                    Response::Ok
                }
                None => unknown(&runner),
            },
            Request::SkipVersion { runner, version } => self.change_policy(&runner, now, |p| *p = p.skip(version)),
            Request::PinVersion { runner, version } => self.change_policy(&runner, now, |p| p.pinned = version),
            Request::History {
                runner,
                days,
                utc_offset_minutes,
            } => match self.state.runners.get(runner.as_str()) {
                Some(st) if self.runners.contains_key(&runner) => Response::History {
                    summary: summarize(&st.history, now, days, utc_offset_minutes.clamp(-14 * 60, 14 * 60)),
                    events: st.history.recent(200),
                },
                _ => unknown(&runner),
            },
            Request::GetRunnerConfig { runner } => self.get_runner_config(&runner),
            Request::PreviewRunnerConfig { runner, edit } => self.preview_runner_config(&runner, &edit),
            Request::SaveRunnerConfig { runner, edit, base } => self.save_runner_config(&runner, &edit, &base, now),
            Request::Processes { runner } => self.processes_response(&runner, now),
            Request::StopProcess {
                runner,
                pid,
                started_at,
                allow_runner,
            } => self.stop_process(&runner, pid, started_at, allow_runner, now),
            Request::KeepProcess {
                runner,
                pid,
                started_at,
                extra,
            } => self.keep_process(&runner, pid, started_at, extra),
            Request::Metrics { runner, range_hours } => {
                let range = Duration::from_secs(u64::from(range_hours.clamp(1, 90 * 24)) * 3600);
                let folder = runner
                    .as_ref()
                    .and_then(|n| self.state.runners.get(n.as_str()))
                    .map(|s| s.folder_sizes.query(now, range))
                    .unwrap_or_default();
                let usage = runner
                    .as_ref()
                    .and_then(|n| self.state.runners.get(n.as_str()))
                    .map(|s| s.usage.query(now, range))
                    .unwrap_or_default();
                Response::Metrics {
                    machine: self.state.metrics.query(now, range),
                    folder,
                    usage,
                }
            }
            Request::EngineReport
            | Request::EnginePrunePreview { .. }
            | Request::EnginePrune { .. }
            | Request::CacheReport { .. }
            | Request::CachePreview { .. }
            | Request::CacheClean { .. } => {
                return self.ops_request(req, reply, now);
            }
            Request::Disk { runner } => {
                let Some(r) = self.runners.get(&runner) else {
                    return Some((reply, unknown(&runner)));
                };
                if r.scanning {
                    return Some((reply, err(ErrorCode::Busy, "the folder is being measured; try again in a moment")));
                }
                self.spawn_scan(&runner, Some(reply));
                return None;
            }
            Request::CleanDisk { runner } => {
                let Some(r) = self.runners.get_mut(&runner) else {
                    return Some((reply, unknown(&runner)));
                };
                if r.cleaning {
                    return Some((reply, err(ErrorCode::Busy, "a cleanup is running")));
                }
                if r.jobs.running(now) || !matches!(r.update.phase(), UpdatePhase::Idle) {
                    r.clean_wanted = true;
                    return Some((
                        reply,
                        err(
                            ErrorCode::Busy,
                            "the runner is running a job or an update; Pitcrew cleans its folder as soon as it is idle",
                        ),
                    ));
                }
                let cfg = r.cfg.clone();
                let bin = layout::binary(&cfg, self.env.platform.os, |p| p.exists()).path;
                let policy = cfg.cleanup_policy(&self.config.defaults);
                let keep = self.config.defaults.logs.keep;
                let free = self.probe.disk_space(&cfg.dir, (self.env.clock)()).map(|(f, _)| f);
                r.cleaning = true;
                let clock = self.env.clock.clone();
                self.worker(&runner, move || {
                    let scan = cleanup::scan(&cfg, &bin, free);
                    let plan = housekeeping::plan(&policy, &scan, CleanupTrigger::Manual, keep, clock());
                    Done::Cleaned(cleanup::execute(&cfg, &plan, clock()), Some(reply))
                });
                return None;
            }
            Request::GetSettings => Response::Settings {
                config: self.config.clone(),
                problems: self.config_problems.clone(),
                read_only: self.read_only,
            },
            Request::SaveSettings { config } => self.save_settings(config, now),
            Request::AddRunner { runner } => {
                let mut cfg = self.config.clone();
                if cfg.runners.iter().any(|r| r.name == runner.name) {
                    err(ErrorCode::BadRequest, format!("a runner is already named {}", runner.name))
                } else {
                    cfg.runners.push(runner);
                    self.save_settings(cfg, now)
                }
            }
            Request::RemoveRunner { runner } => {
                let mut cfg = self.config.clone();
                let before = cfg.runners.len();
                cfg.runners.retain(|r| r.name != runner);
                if cfg.runners.len() == before {
                    unknown(&runner)
                } else {
                    self.save_settings(cfg, now)
                }
            }
            Request::Register {
                runner,
                server_url,
                token,
                display_name,
            } => {
                let server = match check_server_url(&server_url) {
                    Ok(s) => s,
                    Err(e) => return Some((reply, err(ErrorCode::BadRequest, e))),
                };
                let display = display_name.unwrap_or_else(|| runner.to_string());
                if display.is_empty() || display.len() > 255 || display.chars().any(char::is_control) {
                    return Some((
                        reply,
                        err(ErrorCode::BadRequest, "the display name must be 1-255 printable characters"),
                    ));
                }
                let Some(r) = self.runners.get(&runner) else {
                    return Some((reply, unknown(&runner)));
                };
                let mut cfg = r.cfg.clone();
                let bin = self.binary(&cfg);
                let src = self.env.sources.clone();
                let platform = self.env.platform;
                // Remember the server (not the token) in the configuration.
                let mut config = self.config.clone();
                if let Some(c) = config.runners.iter_mut().find(|c| c.name == runner) {
                    c.server_url = Some(server.clone());
                    cfg.server_url = Some(server.clone());
                }
                if let Response::Error(e) = self.save_settings(config, now) {
                    return Some((reply, Response::Error(e)));
                }
                self.worker(&runner, move || {
                    let result = (|| {
                        std::fs::create_dir_all(&cfg.dir).map_err(|e| format!("{}: {e}", cfg.dir.display()))?;
                        if !bin.path.exists() {
                            if !bin.managed {
                                return Err(format!("the runner binary {} is missing", bin.path.display()));
                            }
                            let v = work::latest(&src, cfg.kind, pitcrew_core::update::Channel::Stable)?;
                            work::download(&src, cfg.kind, &v, platform, &bin.path)?;
                            work::verify_download(&src, cfg.kind, &v, platform, &bin.path)?;
                            work::swap(&bin.path)?;
                        }
                        work::register(&Registration {
                            dir: &cfg.dir,
                            binary: &bin.path,
                            config: layout::runner_config(&cfg),
                            server: &server,
                            token: token.expose(),
                            name: &display,
                            labels: &cfg.labels,
                        })
                    })();
                    Done::Registered(result, reply)
                });
                return None;
            }
            Request::Diagnostics => self.diagnostics(now),
        };
        Some((reply, resp))
    }

    fn control(&mut self, runner: &RunnerName, input: Input, now: Timestamp) -> Response {
        let Some(r) = self.runners.get_mut(runner) else {
            return unknown(runner);
        };
        if input == Input::Start && !r.registered {
            return err(ErrorCode::BadRequest, "the runner is not registered yet; register it first");
        }
        let resumes = matches!(input, Input::Resume | Input::Start);
        let actions = r.ctl.handle(input, now);
        self.act(runner, actions, now);
        self.user_acted(runner, resumes);
        Response::Ok
    }

    fn apply_update(&mut self, runner: &RunnerName, now: Timestamp) -> Response {
        let Some(r) = self.runners.get(runner) else {
            return unknown(runner);
        };
        if !self.binary(&r.cfg).managed {
            return err(
                ErrorCode::BadRequest,
                "this runner uses a binary you provide; Pitcrew does not update it",
            );
        }
        if !matches!(r.update.phase(), UpdatePhase::Idle) {
            return err(ErrorCode::Busy, "an update is already in progress");
        }
        let target = match self.decision(r, now) {
            Some(
                UpdateDecision::Apply { target } | UpdateDecision::Available { target, .. } | UpdateDecision::WaitForWindow { target },
            ) => target,
            _ => return err(ErrorCode::BadRequest, "no newer version is known; check for updates first"),
        };
        self.begin_update(runner, target, now);
        Response::Ok
    }

    fn change_policy(&mut self, runner: &RunnerName, now: Timestamp, f: impl FnOnce(&mut pitcrew_core::update::UpdatePolicy)) -> Response {
        let mut config = self.config.clone();
        let defaults = config.defaults.update.clone();
        let Some(r) = config.runners.iter_mut().find(|r| &r.name == runner) else {
            return unknown(runner);
        };
        let mut p = r.update.clone().unwrap_or(defaults);
        f(&mut p);
        r.update = Some(p);
        self.save_settings(config, now)
    }

    fn save_settings(&mut self, config: Config, now: Timestamp) -> Response {
        if self.read_only {
            return err(
                ErrorCode::BadRequest,
                "the configuration was written by a newer Pitcrew; update Pitcrew to change it",
            );
        }
        // Validate exactly like a file on disk.
        let text = match config::to_toml(&config) {
            Ok(t) => t,
            Err(e) => return err(ErrorCode::BadRequest, format!("the settings cannot be saved: {e}")),
        };
        let checked = config::load(Some(&text), None);
        if !checked.problems.is_empty() {
            let list: Vec<String> = checked.problems.iter().map(ToString::to_string).collect();
            return err(ErrorCode::BadRequest, list.join("; "));
        }
        if let Err(e) = store::write_atomic(&self.env.data_dir.join(crate::paths::CONFIG_FILE), &text) {
            return err(ErrorCode::Internal, format!("the settings could not be saved: {e}"));
        }
        self.apply_config(checked.config, now);
        Response::Ok
    }

    fn apply_config(&mut self, config: Config, now: Timestamp) {
        self.config = config;
        self.config_problems.clear();
        let wanted: Vec<RunnerConfig> = self.config.runners.clone();
        // Removed runners stop after their job and are then forgotten.
        let names: Vec<RunnerName> = self.runners.keys().cloned().collect();
        for name in names {
            if !wanted.iter().any(|w| w.name == name) {
                if let Some(r) = self.runners.get_mut(&name) {
                    r.removing = true;
                    let actions = r.ctl.handle(Input::Stop, now);
                    self.act(&name, actions, now);
                }
            }
        }
        for w in wanted {
            let settings = self.settings_for(&w);
            match self.runners.get_mut(&w.name) {
                Some(r) => {
                    r.ctl.reconfigure(settings);
                    r.log.set_policy(self.config.defaults.logs);
                    r.cfg = w;
                }
                None => self.add(w, now),
            }
        }
        self.dirty = true;
    }

    fn diagnostics(&self, now: Timestamp) -> Response {
        let ServiceEvent::Status { runners, service } = self.status_event(now) else {
            return err(ErrorCode::Internal, "no status");
        };
        let mut sections = vec![DiagnosticsSection {
            name: "service.txt".into(),
            text: format!(
                "Pitcrew service {SERVICE_VERSION}\nplatform: {:?}\ndata folder: {}\nstarted: {}\nconfiguration problems: {:?}\nread-only: {}\n",
                self.env.platform,
                self.env.data_dir.display(),
                pitcrew_core::time::format_rfc3339(self.started_at),
                self.config_problems,
                self.read_only
            ),
        }];
        sections.push(DiagnosticsSection {
            name: "pitcrew.toml".into(),
            text: redact(&config::to_toml(&self.config).unwrap_or_default()),
        });
        sections.push(DiagnosticsSection {
            name: "status.json".into(),
            text: redact(&serde_json::to_string_pretty(&(&runners, &service)).unwrap_or_default()),
        });
        for (name, st) in &self.state.runners {
            let events = st.history.recent(300);
            sections.push(DiagnosticsSection {
                name: format!("history-{name}.json"),
                text: redact(&serde_json::to_string_pretty(&events).unwrap_or_default()),
            });
        }
        for (name, r) in &self.runners {
            let lines = r.tail.since(None, usize::MAX);
            let skip = lines.len().saturating_sub(1500);
            let text: String = lines.iter().skip(skip).map(|l| redact(&l.text) + "\n").collect();
            sections.push(DiagnosticsSection {
                name: format!("runner-{name}.log"),
                text: cap(text, 200 * 1024),
            });
        }
        if let Ok(Some(text)) = store::read_bounded(self.service_log.path(), 256 * 1024) {
            sections.push(DiagnosticsSection {
                name: "service.log".into(),
                text: cap(redact(&text), 200 * 1024),
            });
        }
        Response::Diagnostics { sections }
    }

    // ---- status --------------------------------------------------------------------------------------------------

    fn snapshot(&self, r: &Runner, now: Timestamp) -> RunnerSnapshot {
        let updating = !matches!(r.update.phase(), UpdatePhase::Idle);
        let facts = r.ctl.facts(now, r.registered, updating);
        let status = health::status(&facts, now);
        let fallback = state::RunnerState::default();
        let st = self.state.runners.get(r.cfg.name.as_str()).unwrap_or(&fallback);
        let policy = r.cfg.update_policy(&self.config.defaults).clone();
        let cleanup_policy = r.cfg.cleanup_policy(&self.config.defaults);
        let update_target = match self.decision(r, now) {
            Some(
                UpdateDecision::Apply { target } | UpdateDecision::Available { target, .. } | UpdateDecision::WaitForWindow { target },
            ) => Some(target),
            _ => None,
        };
        let session = match (r.launched_as, r.ctl.life()) {
            (Some(t), _) if r.ctl.process_alive() => session::state_for(r.cfg.session, t),
            (_, Life::WaitingForLogon) => SessionState::WaitingForLogon,
            _ => match &r.session_problem {
                Some(reason) => SessionState::Unavailable { reason: reason.clone() },
                None => SessionState::NotRunning,
            },
        };
        let next_start_at = match r.ctl.life() {
            Life::WaitingToStart { at } => Some(at),
            Life::CrashLoop { until } => Some(until),
            _ => None,
        };
        let disk = r.scan.as_ref().map(|s| DiskUse {
            folder_bytes: s.folder_bytes,
            free_bytes: s.free_bytes,
            low: r.disk_low,
        });
        let next_cleanup_at = cleanup_policy.automatic.then(|| {
            let every = Duration::from_secs(u64::from(cleanup_policy.every_hours.max(1)) * 3600);
            st.last_cleanup.as_ref().map_or(now, |c| c.at.plus(every))
        });
        RunnerSnapshot {
            name: r.cfg.name.clone(),
            kind: r.cfg.kind,
            status,
            enabled: r.ctl.wanted(),
            pending: r.ctl.pending(),
            session,
            running_since: r.ctl.running_since(),
            next_start_at,
            restarts_24h: st.history.restarts_since(Timestamp(now.0.saturating_sub(24 * 3600 * 1000))),
            problem: r
                .ctl
                .problem()
                .map(str::to_string)
                .or_else(|| r.log.failed().then(|| "the runner's log file cannot be written".to_string())),
            current_job: r.jobs.current().cloned().filter(|_| r.jobs.running(now)),
            current_version: r.current_version.clone(),
            latest_version: st.latest.clone(),
            checked_at: st.checked_at,
            update_target,
            last_update: st.last_update.clone(),
            update_phase: r.update.phase().clone(),
            update_policy: policy,
            managed_binary: layout::binary(&r.cfg, self.env.platform.os, |p| p.exists()).managed,
            dir: r.cfg.dir.display().to_string(),
            labels: r.cfg.labels.clone(),
            server_url: r.cfg.server_url.clone(),
            session_mode: r.cfg.session,
            disk,
            next_cleanup_at,
            usage: r.usage_now,
            leftovers: r.watch.leftovers(),
            hold: self.hold_info(r),
            deadline: pitcrew_core::update::watch::deadline(self.runner_policy_deadline(r), st.behind_since, now),
            update_loop: self.update_loop_of(r.cfg.name.as_str(), now),
        }
    }

    fn status_event(&self, now: Timestamp) -> ServiceEvent {
        let runners = self.runners.values().map(|r| self.snapshot(r, now)).collect();
        let (own_cpu_permille, own_mem_bytes) = self.probe.own_usage();
        ServiceEvent::Status {
            runners,
            service: ServiceInfo {
                version: SERVICE_VERSION.into(),
                started_at: self.started_at,
                data_dir: self.env.data_dir.display().to_string(),
                config_problems: self.config_problems.clone(),
                read_only: self.read_only,
                machine: self.state.metrics.latest(),
                own_cpu_permille,
                own_mem_bytes,
            },
        }
    }

    fn publish(&mut self, now: Timestamp) {
        let event = self.status_event(now);
        // Announce new bad states.
        if let ServiceEvent::Status { runners, .. } = &event {
            for s in runners {
                if let Some(r) = self.runners.get_mut(&s.name) {
                    let prev = r.last_status.replace(s.status);
                    if let Some(n) = notify::status_change(&s.name, prev, s.status, now) {
                        r.log.supervisor(now, &n.title);
                        self.pending_notes.push(n);
                    }
                }
            }
        }
        let notes = std::mem::take(&mut self.pending_notes);
        for n in notes {
            self.notify(n);
        }
        // Push the status only when it changed (the service's own CPU figure is left out of the comparison).
        let key = match &event {
            ServiceEvent::Status { runners, service } => {
                serde_json::to_string(&(runners, &service.machine, &service.config_problems)).unwrap_or_default()
            }
            ServiceEvent::Notification { .. } => String::new(),
        };
        if key != self.last_status {
            self.last_status = key;
            self.broadcast(Response::Event { event });
        }
    }

    fn notify(&mut self, n: Notification) {
        self.broadcast(Response::Event {
            event: ServiceEvent::Notification { notification: n },
        });
    }

    fn broadcast(&mut self, r: Response) {
        self.subscribers.retain(|s| s.try_send(r.clone()).is_ok());
    }

    fn log_service(&mut self, now: Timestamp, text: &str) {
        self.service_log.supervisor(now, text);
    }

    fn save_state(&mut self, now: Timestamp) {
        self.last_save = now;
        self.dirty = false;
        for r in self.runners.values() {
            if let Some(st) = self.state.runners.get_mut(r.cfg.name.as_str()) {
                st.keeps = r.watch.keeps();
            }
        }
        match state::to_json(&self.state) {
            Ok(text) => {
                if let Err(e) = store::write_atomic(&self.env.data_dir.join(STATE_FILE), &text) {
                    self.log_service(now, &format!("saving the state failed: {e}"));
                }
            }
            Err(e) => self.log_service(now, &format!("the state could not be serialised: {e}")),
        }
    }

    fn shutdown(&mut self) {
        let now = (self.env.clock)();
        self.log_service(now, "stopping: asking the runners to stop (running jobs finish first)");
        for r in self.runners.values_mut() {
            if let Some(p) = r.process.as_mut() {
                r.log.supervisor(now, "the Pitcrew service is stopping");
                let _ = p.stop_gracefully();
            }
        }
        // Give them a little time; a runner that is still busy keeps running and is adopted at the next start.
        for _ in 0..40 {
            if self
                .runners
                .values_mut()
                .all(|r| r.process.as_mut().is_none_or(|p| p.try_wait().is_some()))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        for (name, r) in &mut self.runners {
            if r.process.as_mut().is_some_and(|p| p.try_wait().is_some()) {
                self.state.runners.entry(name.to_string()).or_default().pid = None;
            }
        }
        self.save_state(now);
    }
}

fn unknown(runner: &RunnerName) -> Response {
    err(ErrorCode::UnknownRunner, format!("no runner is named {runner}"))
}

fn cap(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut start = text.len() - max;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text = format!(
            "[… earlier lines cut …]
{}",
            &text[start..]
        );
    }
    text
}

fn plan_of(p: &UpdatePhase) -> Option<&UpdatePlan> {
    match p {
        UpdatePhase::Idle => None,
        UpdatePhase::Downloading { plan }
        | UpdatePhase::Verifying { plan }
        | UpdatePhase::Draining { plan, .. }
        | UpdatePhase::Stopping { plan, .. }
        | UpdatePhase::Swapping { plan }
        | UpdatePhase::Confirming { plan, .. }
        | UpdatePhase::RollingBack { plan, .. } => Some(plan),
    }
}

fn outcome_text(o: &pitcrew_core::update::UpdateOutcome) -> String {
    use pitcrew_core::update::UpdateOutcome as O;
    match o {
        O::Updated { plan } => format!("updated to {}", plan.to),
        O::RolledBack { plan, reason } => format!("{} rolled back ({reason:?})", plan.to),
        O::Postponed { plan, reason } => format!("{} postponed ({reason:?})", plan.to),
        O::Cancelled { plan } => format!("{} cancelled", plan.to),
        O::Failed { plan, reason, .. } => format!("{} failed: {reason}", plan.to),
    }
}

fn record_job(state: &mut ServiceState, name: &RunnerName, job: FinishedJob, now: Timestamp) {
    let _ = now;
    let at = job.finished_at;
    state
        .runners
        .entry(name.to_string())
        .or_default()
        .history
        .push(at, HistoryKind::JobFinished { job });
}

/// Loads the configuration from `dir` (never fails on damaged contents; see `pitcrew_core::config`).
pub fn load_config(dir: &Path) -> Result<config::Loaded, crate::error::Error> {
    let primary = store::read_bounded(&dir.join(crate::paths::CONFIG_FILE), config::MAX_CONFIG_BYTES)?;
    let backup = store::read_bounded(&dir.join(crate::paths::CONFIG_BACKUP), config::MAX_CONFIG_BYTES)?;
    Ok(config::load(primary.as_deref(), backup.as_deref()))
}

/// A runner the previous service instance started, watched by its process id only (its output went to the old
/// service). Pitcrew restarts it between jobs to get its output back.
struct Adopted {
    id: paddock_proc::ProcessId,
}

impl RunnerProcess for Adopted {
    fn pid(&self) -> u32 {
        self.id.pid()
    }

    fn try_wait(&mut self) -> Option<Option<i32>> {
        // Gone, or its pid belongs to another process now (ADR 0033).
        if self.id.is_alive() {
            None
        } else {
            Some(None)
        }
    }

    fn stop_gracefully(&mut self) -> Result<(), String> {
        graceful_stop(self.id, None)
    }

    fn kill(&mut self) {
        kill_tree(self.id);
    }
}
