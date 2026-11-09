//! Pitwall's server (ADR 0002, 0004): engines, the model library, serving with memory management, auth and the
//! HTTP APIs on 127.0.0.1. All decisions come from `pitwall-core`; this crate carries them out (processes, files,
//! sockets). Outbound traffic exists only in [`egress`] (ADR 0001).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod auth;
pub mod cache_db;
pub mod caches;
pub mod egress;
pub mod engine_pins;
pub mod engine_setup;
pub mod engines;
#[cfg(any(test, feature = "test-support"))]
pub mod fake_engine;
pub mod http;
pub mod leftovers;
pub mod library;
pub mod logging;
pub mod mgmt;
pub mod ops;
pub mod os_signals;
pub mod paths;
pub mod process;
pub mod service;
pub mod serving;
pub mod system;
pub mod updates;
pub mod warm;

/// Lock a mutex, recovering from poisoning.
pub(crate) use app::lock as app_lock;
