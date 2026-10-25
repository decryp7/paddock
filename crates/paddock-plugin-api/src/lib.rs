//! The platform half of the family's plugin contract (ADR 0001, extends Debrief ADR 0017): what a plugin links.
//! Rust only, out of process only, one IPC (`paddock-ipc`), no web code from plugins. A plugin depends on this crate
//! alone; its `main` is one line:
//!
//! ```no_run
//! use paddock_plugin_api::prelude::*;
//! use std::sync::Arc;
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn id(&self) -> &str { "net.example.hello" }
//!     fn name(&self) -> &str { "Hello" }
//!     fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
//!     fn register(&self, _c: &mut Contributions) -> Result<(), RegisterError> { Ok(()) }
//! }
//!
//! fn main() -> Result<(), paddock_plugin_api::IpcError> {
//!     paddock_plugin_api::serve(Arc::new(Hello))
//! }
//! ```
//!
//! - [`protocol`]: `PLATFORM_PROTOCOL` 2.0, [`PlatformRequest`] (what the app asks) and [`HostCall`] (what the plugin asks
//!   back), [`Described`];
//! - [`plugin`]: [`Plugin`] and [`Contributions`] (tools, settings, views, MCP tools, app contracts);
//! - [`extension`]: [`Contract`], the app's own request and reply enums, routed by namespace and never decoded here;
//! - [`tools`], [`host_file`], [`output`]: tools with checked schemas, host files read by ranges, output written by the app;
//! - [`compat`]: the version rules and the refusal texts that say who must update;
//! - [`serve()`]: a plugin process' `main`.
//!
//! The manifest model (`Manifest`, `Permission`, `Namespace`, ...) lives in `paddock-plugin-package` and is re-exported
//! here; views are `paddock-view`'s `ViewDoc`.

#![deny(missing_docs)]

pub mod compat;
pub mod entry;
pub mod extension;
pub mod host_file;
pub mod output;
pub mod plugin;
pub mod protocol;
pub mod serve;
pub mod tools;

#[cfg(test)]
mod testing;

pub use compat::{HostInfo, NamespaceSupport, Refusal};
pub use entry::serve;
pub use extension::{Contract, ExtCtx, ExtensionHandler};
pub use paddock_ipc::{IpcError, Json, RemoteError};
pub use plugin::{Contributions, McpTool, Plugin, RegisterError, View, ViewCtx};
pub use protocol::{
    Described, Fail, HostCall, PLATFORM_PROTOCOL, PlatformRequest, ProtocolVersion,
};
pub use tools::{Host, Tool, ToolCtx, ToolError};

/// The names most plugins need: `use paddock_plugin_api::prelude::*;`.
pub mod prelude {
    pub use crate::entry::serve;
    pub use crate::extension::{Contract, ExtCtx};
    pub use crate::plugin::{Contributions, McpTool, Plugin, RegisterError, View, ViewCtx};
    pub use crate::protocol::{McpBudget, McpToolInfo, ViewInfo};
    pub use crate::tools::{OpSpec, Schema, Tool, ToolCtx, ToolError, ToolInfo};
    pub use paddock_ipc::RemoteError;
}
