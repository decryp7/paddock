//! Dyno's background agent (work in progress): sampling, the history database, rules, cleanup and the plugin host.
//! It runs as a child of the app, connected over dyno-ipc, and exits (releasing what rules did) when the app goes.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod cleanup;
pub mod files;
pub mod store;
