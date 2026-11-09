//! Dyno's background agent: samples the machine, keeps the history, answers the app over `paddock-ipc` and
//! is installed as a per-user service (ADR 0002, 0008, 0010). It runs without the window, so history never stops.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod alarms;
pub mod baselines;
pub mod bench;
pub mod bench_child;
pub mod bench_disk_target;
pub mod bench_scratch;
pub mod build_id;
pub mod cache;
pub mod cli;
pub mod counters;
pub mod diagnostics;
pub mod discovery;
pub mod disk;
pub mod extras;
pub mod filehash;
pub mod files;
pub mod firewall;
pub mod geo_db;
pub mod geo_fetch;
pub mod handlers;
pub mod handlers_search;
pub mod hardware;
pub mod helper_centre;
pub mod helper_first;
pub mod helper_link;
pub mod helper_os;
pub mod helper_peer;
pub mod helper_readers;
pub mod helper_wear;
pub mod home_ip;
pub mod instance;
pub mod intel_centre;
pub mod intel_fetch;
pub mod inventory;
pub mod issues;
pub mod jobs;
pub mod logging;
pub mod netwatch;
pub mod network;
pub mod paths;
pub mod popover;
pub mod power;
pub mod recorder;
pub mod region;
pub mod rollup;
pub mod sampler;
pub mod server;
pub mod service;
pub mod store;
pub mod tool_download;
pub mod tool_own_copy;
pub mod tool_pins;
pub mod unusual;
pub mod wakes;
pub mod wire;
pub mod wol;

use agent::Shared;
use std::sync::Arc;

/// Why an agent did not start.
#[derive(Debug)]
pub enum StartError {
    /// Another agent already runs for this data folder ([`instance`]); this one must exit without touching anything.
    AlreadyRunning(instance::LockError),
    /// Anything else, with its message.
    Failed(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::AlreadyRunning(e) => write!(f, "{e}; this one exits."),
            StartError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for StartError {}

impl From<String> for StartError {
    fn from(m: String) -> StartError {
        StartError::Failed(m)
    }
}

impl From<StartError> for String {
    fn from(e: StartError) -> String {
        e.to_string()
    }
}

/// A running agent: the sampler and the server threads.
pub struct Running {
    /// The shared state.
    pub shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
    /// Threads that only read through the helper and may be inside a call that cannot be interrupted (the climate's
    /// `powermetrics` read takes seconds): a stop asks them to end but does not wait for them, and they write nothing once
    /// the agent has stopped.
    readers: Vec<std::thread::JoinHandle<()>>,
    /// What the app needs to connect.
    pub discovery: discovery::Discovery,
    /// The data folder's agent lock, held until the agent has stopped (`None` only when the folder cannot hold a lock
    /// file, in which case it cannot hold the history either and the agent keeps it in memory).
    instance: Option<instance::InstanceLock>,
}

/// What a stop in progress is waiting for (a thread's name, "the benchmark"); empty when none.
static STOP_WAITING_ON: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// What a stop in progress is waiting for, for the line the signal watch writes when the stop takes too long (ER-27); "nothing"
/// when no stop is waiting.
pub fn stop_waiting_on() -> String {
    let now = STOP_WAITING_ON.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if now.is_empty() {
        "nothing".into()
    } else {
        now
    }
}

fn set_stop_waiting_on(what: &str) {
    let mut now = STOP_WAITING_ON.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    now.clear();
    now.push_str(what);
}

/// Whole milliseconds of `d`, saturating.
fn millis(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl Running {
    /// Starts the agent over `platform`'s ports in `dirs`, with a disk map that sees no volumes (see [`disk::DiskPorts::none`]).
    /// The real agent uses [`Running::start_os`].
    pub fn start(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
    ) -> Result<Running, StartError> {
        Running::start_with(dirs, platform, details, reveal, disk::DiskPorts::none())
    }

    /// [`Running::start`] for the real machine: the disk map lists the real volumes and asks the OS for access rights.
    pub fn start_os(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
    ) -> Result<Running, StartError> {
        wol::prepare_password_folder(&dirs);
        Running::start_with(dirs, platform, details, reveal, disk::DiskPorts::os())
    }

    /// [`Running::start`] with the disk map's machine ports given (tests pass fixed ones). Takes the data folder's agent
    /// lock first: while another agent holds it, nothing is opened or written and the result is
    /// [`StartError::AlreadyRunning`].
    pub fn start_with(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
        disk_ports: disk::DiskPorts,
    ) -> Result<Running, StartError> {
        let lock = instance::InstanceLock::acquire(&dirs);
        Running::start_with_lock(dirs, platform, details, reveal, disk_ports, lock)
    }

    /// [`Running::start`] with the helper served from `wiring` (the real-engine rig; see [`agent::HelperWiring`]).
    pub fn start_with_helper(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
        wiring: agent::HelperWiring,
    ) -> Result<Running, StartError> {
        let lock = instance::InstanceLock::acquire(&dirs);
        Running::start_inner(dirs, platform, details, reveal, disk::DiskPorts::none(), lock, Some(wiring))
    }

    /// [`Running::start_with`] with the result of taking the data folder's lock given (tests pass a failed one). A held
    /// lock refuses the start; a folder that cannot lock at all (locking unsupported, for example on some network
    /// shares) starts the agent without the guard against a second one, and says so in the agent's log and activity.
    pub fn start_with_lock(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
        disk_ports: disk::DiskPorts,
        lock: Result<instance::InstanceLock, instance::LockError>,
    ) -> Result<Running, StartError> {
        Running::start_inner(dirs, platform, details, reveal, disk_ports, lock, None)
    }

    fn start_inner(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
        disk_ports: disk::DiskPorts,
        lock: Result<instance::InstanceLock, instance::LockError>,
        wiring: Option<agent::HelperWiring>,
    ) -> Result<Running, StartError> {
        let mut unguarded = None;
        let instance = match lock {
            Ok(lock) => {
                log::info!(ElapsedMs = logging::elapsed_ms(); "Agent lock taken");
                Some(lock)
            }
            Err(e @ instance::LockError::Held { .. }) => {
                let err = StartError::AlreadyRunning(e);
                // The holder's pid comes from its discovery file when there is one (a start in progress has not written it yet).
                let holder = discovery::Discovery::read(&dirs.discovery()).map_or(0, |d| d.pid);
                log::warn!(
                    ExitCode = cli::EXIT_ALREADY_RUNNING,
                    Reason = "the data folder is held by another agent",
                    HolderPid = holder,
                    ParentName = logging::parent_name(),
                    ElapsedMs = logging::elapsed_ms();
                    "Agent refused: another agent holds the data folder"
                );
                return Err(err);
            }
            // A folder that cannot hold a lock file cannot hold the history either (it is kept in memory); go on.
            Err(e) => {
                log::warn!(Reason = "the data folder cannot hold the agent lock", ElapsedMs = logging::elapsed_ms(); "dyno-agent: {e}; starting without the guard against a second agent");
                unguarded = Some(e.to_string());
                None
            }
        };
        let (shared, sensors, processes) = Shared::build_with_helper(dirs, platform, details, reveal, disk_ports, wiring)?;
        log::info!(ElapsedMs = logging::elapsed_ms(); "Agent store open");
        if let Some(why) = unguarded {
            agent::lock(&shared.core).log(
                agent::now(),
                dyno_core::activity::ActivityKind::System,
                "Dyno cannot guard against a second agent in this folder".into(),
                format!("{why}. If two Dyno agents run for this folder, quit one of them."),
            );
        }
        let (host, discovery) = server::bind(&shared).map_err(|e| format!("Dyno could not open its private endpoint: {e}"))?;
        // `bind` wrote `agent.json` (the discovery file the app connects by): this line is when the agent became findable.
        log::info!(ElapsedMs = logging::elapsed_ms(), DiscoveryWritten = true, AgentRun = paddock_log::fingerprint(&discovery.endpoint); "Agent endpoint bound");
        let mut threads = Vec::new();
        // The processes of every account come from the helper's sweeps when this agent reads through it (the local list stays the fallback).
        let processes: Box<dyn dyno_core::ports::ProcessSource> = if shared.helper_first {
            let weak = Arc::downgrade(&shared);
            let period: helper_readers::processes::SweepPeriod = Arc::new(move || {
                weak.upgrade().map_or(helper_readers::processes::SWEEP_PERIOD, |s| {
                    agent::lock(&s.core).settings.sampling.helper_sweep()
                })
            });
            Box::new(helper_readers::processes::HelperFirstProcesses::start(
                processes,
                Box::new(helper_first::SharedPort(Arc::downgrade(&shared))),
                Arc::new(paddock_jobs::time::SystemClock),
                shared.cores,
                period,
                shared.stop_token.clone(),
            ))
        } else {
            processes
        };
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-sampler".into())
                .spawn(move || {
                    // The live view: lowered gently where that never stalls it, normal on Windows (rule 52).
                    dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
                    sampler::Sampler::new(sensors, processes).run(&s)
                })
                .map_err(|e| format!("could not start the sampler: {e}"))?,
        );
        let mut readers = Vec::new();
        if shared.climate.active() {
            readers.push(helper_readers::counters::spawn_worker(&shared).map_err(|e| format!("could not start the sensor reader: {e}"))?);
        }
        inventory::warm(&shared);
        // The volume list is asked for once now, off every request path, so the first page or scan never waits for it.
        shared.disk.refresh_in_background();
        // The volumes' free space is read again at the Setting's interval whether or not any window is open (the popup and the
        // Disk map answer from the cache and never touch the disk themselves). OS mount hooks do not exist yet: every fifth
        // reading lists the mounts again.
        // A volume that comes or goes is listed at once (macOS DiskArbitration, Windows WM_DEVICECHANGE, Linux mountinfo); waking
        // from sleep comes through the power thread. The fifth-reading relist stays as the fallback.
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-mounts".into())
                .spawn(move || {
                    dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
                    disk::mounts::run(&s.disk, &disk::mounts::OsMountEvents, &|| s.stopped());
                })
                .map_err(|e| format!("could not start the mount watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-space".into())
                .spawn(move || {
                    // A person waits on the figures it fills: live work (rule 52).
                    dyno_platform::priority::set_current_thread(dyno_platform::priority::ThreadRole::Live);
                    while !s.stopped() {
                        s.disk.tick();
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                })
                .map_err(|e| format!("could not start the space reader: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-network".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    // The match index of the user's lists is built once at start, as a job.
                    let _ = s.jobs.start(&s, dyno_core::jobs::JobSpec::RebuildIntel);
                    // Slow and cheap: only so that a new listener is noticed (and logged) when nobody has the page open.
                    while !s.stopped() {
                        s.network.tick(&s);
                        // Monthly location databases and list subscriptions that are due (each is a job; a busy one is skipped).
                        for spec in s.intel.due() {
                            let _ = s.jobs.start(&s, spec);
                        }
                        let mut waited = std::time::Duration::ZERO;
                        while waited < network::BACKGROUND_EVERY && !s.stopped() {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            waited += std::time::Duration::from_millis(500);
                        }
                    }
                })
                .map_err(|e| format!("could not start the network watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-hardware".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    hardware::run_background(&s)
                })
                .map_err(|e| format!("could not start the hardware watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-issues".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    issues::run(&s)
                })
                .map_err(|e| format!("could not start the issue reader: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-extras".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    extras::run(&s)
                })
                .map_err(|e| format!("could not start the extras: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-unusual".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    unusual::run(&s)
                })
                .map_err(|e| format!("could not start the program watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-netwatch".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    netwatch::run(&s)
                })
                .map_err(|e| format!("could not start the network change watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-power".into())
                .spawn(move || power::run(&s))
                .map_err(|e| format!("could not start the power watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-wakes".into())
                .spawn(move || wakes::run(&s))
                .map_err(|e| format!("could not start the scheduled-wake watcher: {e}"))?,
        );
        // While this agent holds the data folder's lock, `agent.json` keeps describing it (it is written again when it was deleted).
        if instance.is_some() {
            let (s, mine) = (shared.clone(), discovery.clone());
            let path = shared.dirs.discovery();
            threads.push(
                std::thread::Builder::new()
                    .name("dyno-discovery-guard".into())
                    .spawn(move || {
                        dyno_platform::priority::lower_current_thread();
                        discovery::guard(&s, path, mine)
                    })
                    .map_err(|e| format!("could not start the discovery file guard: {e}"))?,
            );
        }
        // An agent whose program file is replaced exits (code 0) so its supervisor starts the new build.
        if let Ok(exe) = std::env::current_exe() {
            let start = build_id::FileStamp::of(&exe);
            let s = shared.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("dyno-binary-watch".into())
                    .spawn(move || {
                        dyno_platform::priority::lower_current_thread();
                        build_id::run(&s, exe, start)
                    })
                    .map_err(|e| format!("could not start the program file watcher: {e}"))?,
            );
        }
        log::info!(Version = env!("CARGO_PKG_VERSION"), Build = build_id::own_build_id().as_str(); "Agent build");
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-server".into())
                // Not lowered: it answers the app's requests (rule 52; every other agent thread is).
                .spawn(move || server::serve(&s, host))
                .map_err(|e| format!("could not start the server: {e}"))?,
        );
        Ok(Running {
            shared,
            threads,
            readers,
            discovery,
            instance,
        })
    }

    /// Whether the agent has been asked to stop (by `Shutdown` or a signal).
    pub fn stopped(&self) -> bool {
        self.shared.stopped()
    }

    /// Stops and waits for the threads (the sampler releases everything it applied first), then frees the data folder. A
    /// reader inside a helper call is not waited for (it writes nothing once stopped): a stop after TERM has a bounded grace.
    pub fn stop(self) {
        self.shared.request_stop();
        let stopping = std::time::Instant::now();
        let mut slowest = (String::new(), 0u64);
        for t in self.threads {
            let began = std::time::Instant::now();
            let name = t.thread().name().unwrap_or("unnamed").to_string();
            set_stop_waiting_on(&name);
            let _ = t.join();
            let ms = millis(began.elapsed());
            if ms > slowest.1 {
                slowest = (name, ms);
            }
        }
        let mut left = 0usize;
        for r in self.readers {
            if r.is_finished() {
                let _ = r.join();
            } else {
                left += 1;
                // Rule 56 (ER-27): a stop that leaves a read behind says which one, so a slow or late stop can be explained.
                log::info!(Thread = r.thread().name().unwrap_or("unnamed"); "Agent left a helper read in progress to end by itself");
            }
        }
        // A benchmark in progress is cancelled and its child (and Linux unit) ended before the agent goes.
        set_stop_waiting_on("the benchmark");
        self.shared.bench.shutdown(std::time::Duration::from_secs(5));
        drop(self.instance);
        set_stop_waiting_on("");
        log::info!(
            ElapsedMs = millis(stopping.elapsed()),
            SlowestThread = slowest.0.as_str(),
            SlowestMs = slowest.1,
            ReadsLeft = left;
            "Agent threads stopped"
        );
    }
}
