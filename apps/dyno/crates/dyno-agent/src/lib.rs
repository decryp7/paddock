//! Dyno's background agent: samples the machine, keeps the history, answers the app over dyno-ipc and
//! is installed as a per-user service (ADR 0002, 0008, 0010). It runs without the window, so history never stops.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod alarms;
pub mod baselines;
pub mod bench;
pub mod cli;
pub mod counters;
pub mod diagnostics;
pub mod discovery;
pub mod disk;
pub mod extras;
pub mod filehash;
pub mod files;
pub mod handlers;
pub mod handlers_search;
pub mod hardware;
pub mod instance;
pub mod inventory;
pub mod issues;
pub mod jobs;
pub mod logging;
pub mod netwatch;
pub mod network;
pub mod paths;
pub mod recorder;
pub mod rollup;
pub mod sampler;
pub mod server;
pub mod service;
pub mod store;
pub mod unusual;

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
    /// What the app needs to connect.
    pub discovery: discovery::Discovery,
    /// The data folder's agent lock, held until the agent has stopped (`None` only when the folder cannot hold a lock
    /// file, in which case it cannot hold the history either and the agent keeps it in memory).
    instance: Option<instance::InstanceLock>,
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
        let mut unguarded = None;
        let instance = match lock {
            Ok(lock) => {
                log::info!(ElapsedMs = logging::elapsed_ms(); "Agent lock taken");
                Some(lock)
            }
            Err(e @ instance::LockError::Held { .. }) => {
                let err = StartError::AlreadyRunning(e);
                log::warn!(ExitCode = cli::EXIT_ALREADY_RUNNING, ElapsedMs = logging::elapsed_ms(); "Agent refused: another agent holds the data folder");
                instance::note(&dirs, &format!("dyno-agent: {err}"));
                return Err(err);
            }
            // A folder that cannot hold a lock file cannot hold the history either (it is kept in memory); go on.
            Err(e) => {
                log::warn!("dyno-agent: {e}");
                instance::note(
                    &dirs,
                    &format!("dyno-agent: {e}; starting without the guard against a second agent"),
                );
                unguarded = Some(e.to_string());
                None
            }
        };
        let (shared, sensors, processes) = Shared::build_with(dirs, platform, details, reveal, disk_ports)?;
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
        log::info!(ElapsedMs = logging::elapsed_ms(), DiscoveryWritten = true; "Agent endpoint bound");
        let mut threads = Vec::new();
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
        inventory::warm(&shared);
        // The volume list is asked for once now, off every request path, so the first page or scan never waits for it.
        shared.disk.refresh_in_background();
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-network".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
                    // Slow and cheap: only so that a new listener is noticed (and logged) when nobody has the page open.
                    while !s.stopped() {
                        s.network.tick(&s);
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
                .name("dyno-server".into())
                // Not lowered: it answers the app's requests (rule 52; every other agent thread is).
                .spawn(move || server::serve(&s, host))
                .map_err(|e| format!("could not start the server: {e}"))?,
        );
        Ok(Running {
            shared,
            threads,
            discovery,
            instance,
        })
    }

    /// Whether the agent has been asked to stop (by `Shutdown` or a signal).
    pub fn stopped(&self) -> bool {
        self.shared.stopped()
    }

    /// Stops and waits for the threads (the sampler releases everything it applied first), then frees the data folder.
    pub fn stop(self) {
        self.shared.request_stop();
        for t in self.threads {
            let _ = t.join();
        }
        drop(self.instance);
    }
}
