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
pub mod inventory;
pub mod issues;
pub mod jobs;
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

/// A running agent: the sampler and the server threads.
pub struct Running {
    /// The shared state.
    pub shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
    /// What the app needs to connect.
    pub discovery: discovery::Discovery,
}

impl Running {
    /// Starts the agent over `platform`'s ports in `dirs`, with a disk map that sees no volumes (see [`disk::DiskPorts::none`]).
    /// The real agent uses [`Running::start_os`].
    pub fn start(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
    ) -> Result<Running, String> {
        Running::start_with(dirs, platform, details, reveal, disk::DiskPorts::none())
    }

    /// [`Running::start`] for the real machine: the disk map lists the real volumes and asks the OS for access rights.
    pub fn start_os(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
    ) -> Result<Running, String> {
        Running::start_with(dirs, platform, details, reveal, disk::DiskPorts::os())
    }

    /// [`Running::start`] with the disk map's machine ports given (tests pass fixed ones).
    pub fn start_with(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
        disk_ports: disk::DiskPorts,
    ) -> Result<Running, String> {
        let (shared, sensors, processes) = Shared::build_with(dirs, platform, details, reveal, disk_ports)?;
        let (host, discovery) = server::bind(&shared).map_err(|e| format!("Dyno could not open its private endpoint: {e}"))?;
        let mut threads = Vec::new();
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-sampler".into())
                .spawn(move || {
                    dyno_platform::priority::lower_current_thread();
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
        })
    }

    /// Whether the agent has been asked to stop (by `Shutdown` or a signal).
    pub fn stopped(&self) -> bool {
        self.shared.stopped()
    }

    /// Stops and waits for the threads (the sampler releases everything it applied first).
    pub fn stop(self) {
        self.shared.request_stop();
        for t in self.threads {
            let _ = t.join();
        }
    }
}
