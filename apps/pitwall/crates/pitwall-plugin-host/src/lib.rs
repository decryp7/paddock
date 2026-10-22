//! The host side of Pitwall's plugins (ADR 0008): [`PluginHost`] discovers installed plugins
//! (`<plugins_dir>/<id>/pitwall-plugin.toml` + executable), starts each in its own sandboxed process on first use
//! (no network unless the manifest asks for it and the user granted it), and wraps every call in a guard: a deadline,
//! a memory cap, a crash or protocol violation turned into a [`PluginError`] attributed to the plugin, a restart with
//! backoff, and a quarantine after repeated failures ([`policy`]). A plugin never takes the server down.
//!
//! The API is async (tokio): the blocking IPC runs on blocking threads.
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use pitwall_plugin_host::{HostConfig, PluginHost, PluginSettings};
//! let host = PluginHost::new(HostConfig::new("/plugins", "/plugin-data"), PluginSettings::default());
//! let problems = host.rescan();
//! let body = serde_json::json!({"messages": [{"role": "user", "content": "mail me at a@b.example"}]});
//! match host.filter_request(body).await {
//!     Ok(body) => { /* send it to the model */ }
//!     Err(rejection) => { /* answer rejection.status with rejection.message */ }
//! }
//! # Ok(()) }
//! ```
//!
//! The guard follows IMT's `imt-ipc::supervisor` (integrated_maintenance_tool, ADR 0017).

// Tests may unwrap: a failed unwrap is a failed test.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod config;
pub mod discover;
pub mod error;
pub mod events;
pub mod host;
mod instance;
pub mod policy;

pub use config::{FilterFailure, HostConfig, PluginSettings, Timeouts};
pub use discover::{Discovered, Problem};
pub use error::{ErrorKind, PluginError, Rejection};
pub use events::{EventKind, HostEvent};
pub use host::{GenerateStream, PluginHost};
pub use instance::{EngineModel, GenerateEvent, PluginStatus, State};
pub use pitwall_plugin_api::{Description, EndpointInfo, EndpointRequest, EndpointResponse, Kind, ToolInfo};
