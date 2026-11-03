//! Dyno's privileged helper (ADR 0006, 0016): a small program that answers the few read-only questions that need
//! administrator rights (per-program traffic, its own audit log) for one authorised user, and installs or removes
//! itself. It changes no setting of the machine.
//!
//! - [`dispatch`] decides what a request may do and audits refusals and removal ([`audit`]). Pure over its ports, so
//!   tests use fakes.
//! - [`auth`] decides who may call (the authorised user, the recorded agent program).
//! - [`server`] accepts connections on the fixed endpoint ([`paths`]) over `dyno-ipc`.
//! - [`install`] is the typed install and uninstall plan per OS (launchd, systemd, the Windows service) and the
//!   machine that carries it out.
//!
//! The binary (`main.rs`) is three commands: `serve`, `install` and `uninstall`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod audit;
pub mod auth;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod elevate;
pub mod install;
pub mod paths;
pub mod serve;
pub mod server;
pub mod sys;
#[cfg(windows)]
pub mod winsvc;

/// The helper's version: the agent only trusts a helper built from the same Dyno version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
