//! The `pitwall` command-line tool. The engine stays in `pitwall-core`/`pitwall-server`; this crate parses a command
//! line, calls the local management and standard APIs through `pitwall-loopback` (the client that can only reach
//! 127.0.0.1) and prints the result. It adds no server surface and sends nothing off this machine.
//!
//! Pure parts: [`args`], [`exit`], [`format`], [`sse`], [`discover`]. Thin edges: [`api`] and [`run`].

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod args;
pub mod discover;
pub mod exit;
pub mod format;
pub mod logging;
pub mod run;
pub mod sse;
