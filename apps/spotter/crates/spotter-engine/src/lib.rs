//! Spotter's engine: the adapters and orchestration around the pure core.
//!
//! - [`db`]: the SQLite state and history adapter; [`config_store`]: the atomic TOML configuration.
//! - [`fetch`] / [`guard`]: polite HTTP (per-host gaps, validators, caps, redirects) and the loopback/link-local guard.
//! - [`avatar`]: watch pictures (favicons, owner avatars) checked and cached; [`mail`] / [`channels`]: e-mail delivery over SMTP.
//! - [`pipeline`] / [`check`]: reading a source and turning the result into state, events and notification decisions.
//! - [`scheduler`]: what is due and when, with backoff and a concurrency bound.
//! - [`engine`]: the [`Engine`] facade the Tauri shell calls (mirrors the UI's `SpotterBackend`).
//! - [`notifier`] / [`secrets`] / [`clock`]: the traits the shell and tests supply.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod avatar;
pub mod channels;
pub mod check;
pub mod clock;
pub mod config_store;
pub mod db;
pub mod engine;
pub mod error;
pub mod fetch;
pub mod guard;
pub mod mail;
pub mod notifier;
pub mod pipeline;
pub mod preview;
pub mod scheduler;
pub mod secrets;
pub mod view;

pub use check::CheckReport;
pub use engine::{Change, Engine, EngineConfig, Services};
pub use error::{EngineError, Result};
pub use scheduler::Scheduler;
