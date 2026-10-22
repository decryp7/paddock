//! Pitcrew's core: the pure logic of a runner manager for Gitea and Forgejo.
//!
//! Everything here is deterministic and free of I/O, so it is trivially unit-testable: time comes in as
//! [`time::Timestamp`] values, file contents as strings or byte slices, and decisions go out as typed values that
//! the service (`pitcrew-service`) and the app (`app/src-tauri`) carry out. See `docs/adr/0001-architecture.md`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod checksum;
pub mod config;
pub mod distribution;
pub mod health;
pub mod logline;
pub mod logs;
pub mod names;
pub mod platform;
pub mod protocol;
pub mod restart;
pub mod service_spec;
pub mod time;
pub mod update;
pub mod version;
pub mod watchdog;
