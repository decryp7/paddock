//! Pitcrew's background service: the supervisor of the runners, its OS integration (Windows Service Control
//! Manager, macOS launch agent) and the answers to the app's requests. See `docs/adr/0001-architecture.md` and
//! `docs/adr/0003-service-model.md`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod cli;
pub mod dispatch;
pub mod error;
pub mod paths;
pub mod platform;
pub mod process;
pub mod supervisor;
