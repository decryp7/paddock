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
pub mod identity;
pub mod inventory;
pub mod issues;
pub mod netwatch;
pub mod network;
pub mod ownership;
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
    /// Starts the agent over `platform`'s ports in `dirs`.
    pub fn start(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
    ) -> Result<Running, String> {
        let session = dyno_core::session::Session::normal(identity::this_uid());
        Running::start_session(dirs, platform, details, reveal, session)
    }

    /// [`Running::start`] for a given session (normal, or administrator rights for this session only).
    pub fn start_session(
        dirs: paths::Dirs,
        platform: dyno_platform::Platform,
        details: Box<dyn dyno_core::ports::ProcessSource>,
        reveal: agent::Reveal,
        session: dyno_core::session::Session,
    ) -> Result<Running, String> {
        let (shared, sensors, processes) = Shared::build_session(dirs, platform, details, reveal, session)?;
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
                .name("dyno-extras".into())
                .spawn(move || extras::run(&s))
                .map_err(|e| format!("could not start the extras: {e}"))?,
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
        #[cfg(unix)]
        if let Some((uid, gid)) = shared.session.file_owner() {
            // Administrator rights, the user's files: see `ownership`.
            let s = shared.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("dyno-owner".into())
                    .spawn(move || {
                        let root = s.dirs.root().to_path_buf();
                        while !s.stopped() {
                            ownership::hand_over(&root, uid, gid, &ownership::OsOwners);
                            let mut waited = std::time::Duration::ZERO;
                            while waited < std::time::Duration::from_secs(5) && !s.stopped() {
                                std::thread::sleep(std::time::Duration::from_millis(250));
                                waited += std::time::Duration::from_millis(250);
                            }
                        }
                    })
                    .map_err(|e| format!("could not start the file owner: {e}"))?,
            );
        }
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
        #[cfg(unix)]
        if let Some((uid, gid)) = self.shared.session.file_owner() {
            // Whatever the agent wrote while shutting down.
            ownership::hand_over(self.shared.dirs.root(), uid, gid, &ownership::OsOwners);
        }
    }
}
