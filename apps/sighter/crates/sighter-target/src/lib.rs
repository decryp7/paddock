//! Sighter's targets. SW3c: its own RFB/VNC client, ported from the proof of concept. It is the VM-console target of the
//! design review (libvnc is GPL, so the client is our own): it implements the core's [`sighter_core::ports::Target`].
//!
//! Layers, innermost first, so each can be tested without a socket:
//! * [`proto`]: the wire format over `Read`/`Write`. Every parser is bounded and returns a typed [`RfbError`]; none panics
//!   on any input (property-tested).
//! * [`canvas`]: the mutable framebuffer the decoders paint into; it publishes validated core `Frame` snapshots.
//! * [`auth`]: VNC password authentication (DES challenge).
//! * [`keys`]: key names and chords to X11 keysyms.
//! * [`input`]: pointer and key events with the stop flag, held-key tracking and release on stop.
//! * [`dial`]: where a connection may go (`paddock-net`'s checks: Offline, address classes, every resolved address).
//! * [`client`]: the TCP adapter with connect, handshake, write and first-frame timeouts and a reader thread.
//!
//! The stream is not encrypted (VNC password auth is weak by design): keep it on loopback or a tunnel.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod auth;
pub mod canvas;
pub mod client;
pub mod dial;
pub mod error;
pub mod input;
pub mod keys;
pub mod proto;

pub use client::{ConnectOptions, VncAddr, VncTarget};
pub use dial::{Dial, Scope};
pub use error::RfbError;
