//! The contract Dyno plugins are written against.
//!
//! A plugin is a small Rust executable that depends on this crate only. Dyno's agent starts it as a child process,
//! talks to it over [`dyno_ipc`] and treats every failure (a panic, a hang, a crash, garbage) as that plugin's error.
//! A plugin contributes **sensors** ([`SensorProvider`]): extra readings shown next to Dyno's own. Dyno is
//! monitoring-only, so a plugin never changes the machine through Dyno.
//!
//! ```no_run
//! use dyno_plugin_api::{Manifest, Plugin, SensorProvider, SensorReading, Contribution};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn manifest(&self) -> Manifest {
//!         Manifest::new("hello", "Hello", env!("CARGO_PKG_VERSION")).contributing(Contribution::Sensors)
//!     }
//!     fn as_sensors(&self) -> Option<&dyn SensorProvider> {
//!         Some(self)
//!     }
//! }
//!
//! impl SensorProvider for Hello {
//!     fn sensors(&self) -> Vec<SensorReading> {
//!         vec![SensorReading::new("answer", "The answer", 42.0, "")]
//!     }
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     dyno_plugin_api::serve(Hello)
//! }
//! ```
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod manifest;
mod model;
mod plugin;
pub mod protocol;
mod serve;

pub use manifest::{valid_id, Contribution, Manifest, API_VERSION};
pub use model::SensorReading;
pub use plugin::{Plugin, SensorProvider};
pub use protocol::{PluginMessage, PluginReply, PluginRequest};
pub use serve::{handle, serve};
