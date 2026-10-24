//! What an app links to run out-of-process plugins (ADR 0001): discovery of installed `.paddockplugin` packages, grants
//! (default deny), the pure sandbox mapping, a supervisor per plugin with verification, restart with back-off and
//! quarantine, typed proxies for the app's contracts, and the host side of the calls a plugin makes back.
//!
//! - [`installed`]: [`PluginsDir`] (inspect, install and update with the trust policy, remove, enable, grant, verify the
//!   installed program's hash before every start);
//! - [`grants`]: [`Grants`] and [`sandbox_for`]`(&Manifest, &Grants) -> `[`SandboxSpec`]. **Enforcing** the file, network and
//!   program parts at the OS level (AppContainer, landlock/seccomp, `sandbox-exec`) stays in the app for now: the spec is data
//!   the app passes to its launcher (it is also in the child's bootstrap, `Bootstrap::extra`). The host enforces the memory
//!   cap, grant-checked file reads and tool paths itself;
//! - [`supervisor`]: [`Supervisor`] (start, stop, call, status) and [`Remote`]`<C>`, the guarded proxy of a contract;
//! - [`services`], [`files`], [`output`]: the host side of `HostCall` (reads of host files, outputs, tasks, elevated
//!   operations behind the user's grant);
//! - [`host`]: [`PluginHost`], the set of plugins for the Plugins view;
//! - [`status`], [`error`], [`config`].
//!
//! # Legacy plugins (ADR 0003 phase 3)
//!
//! A plugin built for protocol 1 (Debrief's own, before the platform) is refused with a message that says to update it
//! ([`paddock_plugin_api::Refusal::Legacy`], status [`Status::Refused`]). The compatibility shim that would serve it
//! (decoding the old enum as `debrief.legacy` calls) is the app's, not the platform's.

#![deny(missing_docs)]

pub mod config;
pub mod error;
pub mod files;
pub mod grants;
pub mod host;
pub mod installed;
pub mod output;
pub mod services;
pub mod status;
pub mod supervisor;

pub use config::HostConfig;
pub use error::{FaultKind, HostError};
pub use files::{FileError, HostFiles};
pub use grants::{Grants, SandboxSpec, sandbox_for};
pub use host::{PluginHost, PluginInfo};
pub use installed::{Inspection, InstallRecord, Installed, PluginsDir};
pub use output::OutputSink;
pub use services::{ElevatedRunner, HostServices, NoTasks, RunningTask, TaskObserver};
pub use status::Status;
pub use supervisor::{Remote, Supervisor};
