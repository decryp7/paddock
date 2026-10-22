//! Pitwall's server (ADR 0002, 0004): engines, the model library, serving with memory management, auth and the
//! HTTP APIs on 127.0.0.1. All decisions come from `pitwall-core`; this crate carries them out (processes, files,
//! sockets). Outbound traffic exists only in [`egress`] (ADR 0001).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod egress;
pub mod engines;
pub mod paths;
pub mod process;
pub mod library;
pub mod system;
