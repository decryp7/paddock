//! Dyno's one IPC, used between the app and its agent and between the agent and its plugins.
//!
//! Every byte between two Dyno processes is a [frame](frame): a 4-byte little-endian length, then a postcard body.
//! The body is a [`Frame`] envelope (handshake, calls, replies, failures, pushes) around the protocol's own message.
//!
//! The connection model is always the same:
//!
//! 1. the host creates a private endpoint and a random [`Token`] ([`Host::listen`]);
//! 2. it starts the child ([`child::launch`]), passing both on the child's stdin as one JSON line;
//! 3. the child connects and says `Hello { version, token, pid }` ([`ChildLink`], [`Client::connect`]); the host checks
//!    the token in constant time, the protocol version and, where the OS tells it, the peer's process id, and answers
//!    `Welcome` ([`Host::accept`]);
//! 4. both sides then talk through a [`Peer`]: calls with timeouts, a handler for incoming calls, and pushes.
//!
//! The child's stdin stays open as its lifeline: when the host goes away the child exits. On Windows the child also
//! runs in a job object that kills it with the host and can cap its memory.
//!
//! Everything is synchronous and thread-based; queues are bounded, and decoding never panics.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod child;
pub mod endpoint;
mod error;
pub mod frame;
mod host;
#[cfg(windows)]
mod job;
mod peer;
mod queue;
mod token;
pub mod wire;

pub use child::{ChildLink, ChildProcess, ChildSpec};
pub use error::IpcError;
pub use frame::FrameLimit;
pub use host::{Client, Connection, Host};
pub use peer::{Handler, Peer, PeerOptions};
pub use token::Token;
pub use wire::{Frame, RemoteError, PROTOCOL_VERSION};
