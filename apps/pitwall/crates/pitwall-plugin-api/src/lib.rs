//! The SDK of Pitwall plugins (ADR 0008). A plugin is a Rust program in its own process, sandboxed by Pitwall
//! (no network unless the user allows it), that depends on this crate alone:
//!
//! ```no_run
//! use pitwall_plugin_api::prelude::*;
//!
//! struct Shout;
//!
//! impl Filter for Shout {
//!     fn filter_request(&self, body: Value) -> Result<FilterVerdict, PluginError> {
//!         Ok(FilterVerdict::forward(body))
//!     }
//! }
//!
//! // Usually `include_str!("../pitwall-plugin.toml")`.
//! const MANIFEST: &str = r#"
//! id = "shout"
//! name = "Shout"
//! version = "0.1.0"
//! contract = "1.0"
//! kinds = ["filter"]
//! executable = "shout"
//! "#;
//!
//! fn main() -> std::process::ExitCode {
//!     serve(Plugin::new(MANIFEST).filter(Shout))
//! }
//! ```
//!
//! - [`manifest`]: `pitwall-plugin.toml` (id, name, version, contract, kinds, permissions, memory cap).
//! - [`plugin`]: one trait per kind ([`Filter`], [`Endpoints`], [`Tools`], [`Engine`]) and the [`Plugin`] builder.
//! - [`protocol`]: the requests and replies on the wire, versioned by [`CONTRACT_VERSION`] (additive within a major).
//! - [`serve()`]: runs the plugin (its `main` is one line).
//!
//! Design copied from IMT's `imt-plugin-api` (integrated_maintenance_tool, ADR 0017 / 0027), reduced to Pitwall's
//! four kinds.

// Tests may unwrap: a failed unwrap is a failed test.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod manifest;
pub mod plugin;
pub mod protocol;
pub mod serve;

pub use manifest::{Kind, Manifest, ManifestError, Permissions, MANIFEST_FILE};
pub use pitwall_ipc::{ContractVersion, Json};
pub use plugin::{Collect, Endpoints, Engine, Filter, ItemSink, Plugin, PluginContext, PluginError, TokenSink, Tools, DATA_DIR_ENV};
pub use protocol::{
    Description, EndpointInfo, EndpointRequest, EndpointResponse, FilterVerdict, GenerateItem, Reply, Request, ToolInfo, CONTRACT_VERSION,
};
pub use serve::serve;

/// Everything a plugin's `main.rs` usually needs.
pub mod prelude {
    pub use crate::{
        serve, EndpointInfo, EndpointRequest, EndpointResponse, Endpoints, Engine, Filter, FilterVerdict, Plugin, PluginContext, PluginError,
        TokenSink, ToolInfo, Tools,
    };
    pub use serde_json::{json, Value};
}
