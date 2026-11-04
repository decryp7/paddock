//! Dyno's privileged helper (ADR 0006, 0016): a small program that answers the few read-only questions that need
//! administrator rights (per-program traffic, its own audit log) for the local accounts the OS says may ask, and
//! installs or removes itself. It changes no setting of the machine.
//!
//! - [`dispatch`] decides what a request may do and audits refusals and removal ([`audit`]). Pure over its ports, so
//!   tests use fakes.
//! - [`auth`] decides what each caller may ask (the caller is the account the OS reports; never a pid or a program).
//! - [`server`] accepts connections on the system endpoint ([`paths`]) over `paddock-ipc`, with per-account limits.
//! - [`setup`] installs and removes the helper through `dyno-helper-service` (and so `paddock-service`).
//!
//! The binary (`main.rs`) is three commands: `serve`, `install` and `uninstall`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod audit;
pub mod auth;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod elevate;
#[cfg(windows)]
pub mod eventlog;
pub mod logfile;
pub mod orphan;
pub mod paths;
pub mod policy;
pub mod protect;
pub mod refusal;
pub mod serve;
pub mod server;
pub mod setup;
pub mod startup;
pub mod sys;
pub mod tools;
#[cfg(windows)]
pub mod winsvc;

/// The helper's version: the agent only trusts a helper built from the same Dyno version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
