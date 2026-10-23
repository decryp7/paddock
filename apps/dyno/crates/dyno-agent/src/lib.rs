//! Dyno's background agent: samples the machine, keeps the history, runs the rules, answers the app over dyno-ipc and
//! is installed as a per-user service (ADR 0002, 0008, 0010). It runs without the window, so history never stops.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod alarms;
pub mod baselines;
pub mod bench;
pub mod cleanup;
pub mod cli;
pub mod counters;
pub mod diagnostics;
pub mod discovery;
pub mod filehash;
pub mod files;
pub mod handlers;
pub mod handlers_cleanup;
pub mod handlers_search;
pub mod handlers_watch;
pub mod hardware;
pub mod helper_centre;
pub mod helper_link;
pub mod inventory;
pub mod issues;
pub mod netwatch;
pub mod network;
pub mod paths;
pub mod recorder;
pub mod rollup;
pub mod sampler;
pub mod server;
pub mod service;
pub mod store;
pub mod tuning;
pub mod unusual;
pub mod watch;

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
    /// Starts the agent over `platform`'s ports in `dirs`.
    pub fn start(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
    ) -> Result<Running, String> {
        let (shared, sensors, processes) = Shared::build(dirs, platform, details, reveal)?;
        let (host, discovery) = server::bind(&shared).map_err(|e| format!("Dyno could not open its private endpoint: {e}"))?;
        let mut threads = Vec::new();
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-sampler".into())
                .spawn(move || sampler::Sampler::new(sensors, processes).run(&s))
                .map_err(|e| format!("could not start the sampler: {e}"))?,
        );
        inventory::warm(&shared);
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-network".into())
                .spawn(move || {
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
                .spawn(move || hardware::run_background(&s))
                .map_err(|e| format!("could not start the hardware watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-issues".into())
                .spawn(move || issues::run(&s))
                .map_err(|e| format!("could not start the issue reader: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-unusual".into())
                .spawn(move || unusual::run(&s))
                .map_err(|e| format!("could not start the program watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-netwatch".into())
                .spawn(move || netwatch::run(&s))
                .map_err(|e| format!("could not start the network change watcher: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-watched".into())
                .spawn(move || handlers_watch::run(&s))
                .map_err(|e| format!("could not start the watched folders sweeper: {e}"))?,
        );
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("dyno-server".into())
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
