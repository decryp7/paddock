//! Spotter's plugin host (ADR 0003) and child-process helper.
//!
//! - [`host`]: the registry of installed plugins; every plugin call goes through it, guarded.
//! - [`session`]: one conversation with a plugin process (handshake, deadlines, memory limit).
//! - [`process`]: the one way Spotter starts child processes (no console window, cleared environment, bounded).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod host;
pub mod process;
pub mod session;

pub use host::{Installed, ListOutcome, PluginHost, PluginStatus};
