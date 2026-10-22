//! Spotter's plugin SDK (ADR 0003).
//!
//! A plugin is a separate Rust program that offers one or more *source kinds* (places releases come from). Release
//! Monitor starts it with its standard input and output connected to private pipes and talks to it in
//! length-prefixed postcard frames ([`frame`]). A plugin depends on this crate alone and runs [`serve`]:
//!
//! ```no_run
//! use spotter_plugin_api::{serve, Config, Manifest, Plugin, PluginError, SourceKindSpec, WireRelease};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn manifest(&self) -> Manifest {
//!         Manifest::new("hello", "Hello", env!("CARGO_PKG_VERSION"))
//!             .with_kind(SourceKindSpec::new("hello", "Hello releases", "Always one release."))
//!     }
//!
//!     fn list(&self, _kind: &str, _config: &Config, _limit: usize) -> Result<Vec<WireRelease>, PluginError> {
//!         Ok(vec![WireRelease::new("1.0.0", "hello-1.0.0", "https://example.invalid/hello-1.0.0")])
//!     }
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     serve(Hello)
//! }
//! ```
//!
//! The same [`schema`] types describe the built-in sources, so the app renders one kind of form for both.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod frame;
pub mod protocol;
pub mod schema;
mod serve;

pub use protocol::{ErrorKind, Manifest, PluginError, ProtocolVersion, Request, Response, SourceKindSpec, WireRelease, PROTOCOL};
pub use schema::{ChoiceOption, Config, FieldKind, FieldSpec};
pub use serve::{serve, serve_on, Plugin};
