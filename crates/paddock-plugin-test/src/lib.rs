//! Testing plugins (ADR 0001): what a plugin author and the host's own tests use.
//!
//! - [`fake`]: [`FakeHost`] calls a plugin in this process through a real `paddock-ipc` connection pair, answers the calls the
//!   plugin makes back from memory and records them;
//! - [`conformance`]: the checks every plugin runs ([`assert_conforms`] is one line in a plugin's tests);
//! - [`golden`]: golden files for what a plugin describes and renders;
//! - [`fixture`]: the sample plugin and its contract, and `paddock-plugin-fixture`, the same plugin as a process the host's
//!   tests start ([`fixture::locate`] finds it).
//!
//! ```no_run
//! use paddock_plugin_test::{conformance, fixture::FixturePlugin};
//! use std::sync::Arc;
//!
//! #[test]
//! fn my_plugin_conforms() {
//!     conformance::assert_conforms(Arc::new(FixturePlugin::new()));
//! }
//! ```

#![deny(missing_docs)]

pub mod conformance;
pub mod fake;
pub mod fixture;
pub mod golden;
pub mod process;

pub use fake::{CallFailure, FakeHost};
