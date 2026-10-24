//! The link between an app and its own companion process (agent, service, worker) that it did not necessarily start
//! (plan X13): the companion runs a [`LinkServer`] and writes a [`Discovery`] file; the app runs a [`LinkClient`] that
//! finds it, connects, replaces a companion of another build, ignores a stale file, and reconnects with back-off.

pub mod client;
pub mod discovery;
pub mod server;

pub use client::{ClientConfig, Launcher, LinkClient, LinkState, Plan, Replace, plan};
pub use discovery::{Discovery, Found, MAX_DISCOVERY};
pub use server::{LinkServer, ServerConfig};
