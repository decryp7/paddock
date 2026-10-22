//! Pitwall's one IPC (ADR 0008) between the server and its plugin processes. Copied and adapted from IMT's
//! `imt-ipc` (integrated_maintenance_tool, ADR 0017 "one IPC"): the same frames, handshake, peer and launcher,
//! trimmed to what plugins need and with the sandbox enforced by the launcher on every platform.
//!
//! - [`frame`]: the frame codec (u32 LE length + postcard; bounded, never panicking); [`wire`]: message bodies.
//! - [`peer`]: a connection after the handshake: calls both ways, streams with credits, cancel, typed errors,
//!   flood limits.
//! - [`link`]: the host listens (private endpoint, per-launch token), the child connects; tokens are compared in
//!   constant time and the peer's pid is checked against the process the host started.
//! - [`endpoint`]: a private address (Unix socket in a 0700 directory; owner-only named pipe that is the first
//!   instance and rejects remote clients); never TCP.
//! - [`launch`] / [`child`]: the launcher and the child loop; [`process`]: starting a plugin (empty environment
//!   plus an allow-list, no console, below-normal priority, stdin lifeline, job object on Windows).
//! - [`sandbox`]: what a plugin may do, per OS (no network by default); [`memory`]: the memory cap on Unix.

// Tests may unwrap: a failed unwrap is a failed test.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod child;
#[cfg(windows)]
pub mod contain;
pub mod endpoint;
pub mod error;
pub mod frame;
#[cfg(windows)]
pub mod job;
pub mod launch;
pub mod link;
pub mod memory;
pub mod peer;
pub mod process;
pub mod runtime;
pub mod sandbox;
pub mod wire;

pub use error::{IpcError, RemoteError};
pub use frame::ContractVersion;
pub use launch::{launch, Bootstrap, Child, ChildSpec};
pub use peer::{CallCtx, CallStream, CancelProbe, Handler, Peer, PeerOptions, StreamEvent};
pub use sandbox::SandboxSpec;
pub use wire::Json;

/// The message of a panic payload (`&str` or `String`), for errors that report a contained panic.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}
