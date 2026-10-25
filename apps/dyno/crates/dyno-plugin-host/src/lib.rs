//! Runs Dyno plugins out of process, for the agent.
//!
//! - [`discover`] finds `<plugins dir>/<id>/plugin.toml` and the executable next to it.
//! - [`PluginHost`] starts each plugin as a child process ([`dyno_ipc::child`]), on Windows in a job object with a
//!   memory cap (256 MiB by default), and calls it with a timeout per request (sensors 2 s, scan 60 s, action 10 s).
//! - A crash, hang (the plugin is killed), garbage, panic or wrong manifest is an error **attributed to the plugin**
//!   ([`HostError`]); it never panics the host. The plugin is restarted with exponential backoff and **quarantined**
//!   after 3 failures in 10 minutes until the user clears it ([`PluginHost::unquarantine`]). A plugin's own typed
//!   error (a missing folder) is not a failure.
//! - Time ([`Clock`]) and process spawning ([`Launcher`]) are injectable, so backoff and quarantine are tested purely.
//!
//! Candidates from cleanup plugins are only proposals: the agent checks every path against Dyno's cleanup safety
//! policy before showing or deleting anything. The host only drops malformed ones (relative paths, too many).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod clock;
mod config;
mod discovery;
mod error;
mod host;
mod launcher;
mod supervision;

pub use clock::{Clock, ManualClock, SystemClock};
pub use config::{HostConfig, Policy, Timeouts};
pub use discovery::{discover, read_plugin, Discovered, Invalid, MAX_PLUGINS};
pub use error::{ErrorKind, HostError};
pub use host::{FromPlugin, PluginHost, PluginState, PluginStatus};
pub use launcher::{Launcher, PluginConnection, ProcessLauncher};
