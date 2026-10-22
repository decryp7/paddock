//! Pitcrew's background service: the supervisor of the runners, its OS integration (Windows Service Control
//! Manager, macOS launch agent) and the answers to the app's requests. See `docs/adr/0001-architecture.md`,
//! `docs/adr/0003-service-model.md` and `docs/adr/0004-ipc.md`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod cleanup;
pub mod cli;
pub mod error;
pub mod host;
pub mod ipc;
pub mod launch;
pub mod launcher;
pub mod logfile;
pub mod login_env;
pub mod net;
pub mod paths;
pub mod platform;
pub mod probe;
pub mod process;
pub mod process_watch;
pub mod runner_files;
pub mod status;
pub mod store;
pub mod supervisor;
pub mod terminate;
pub mod work;
