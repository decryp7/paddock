//! A localhost proxy that cuts a request the store behind it does not answer (user decision 2026-11-20; first user: CI's sccache
//! on RustFS, docs/research/2026-11-19-sccache.md section 8).
//!
//! **The problem.** A client library with no timeout of its own (sccache 0.19.0's S3 level has a fixed 60 s for a read and none for
//! a write) lets one hung store hold every caller for a minute each, or for good.
//!
//! **What the guard does.** The client is pointed at `127.0.0.1:<port>`; the guard forwards the bytes of every connection to the
//! store and watches the stream, never reading HTTP (a signed request passes unchanged and the guard holds no secret).
//!
//! - **Stall:** no byte in either direction for [`Limits::stall`] closes the connection, so the request fails and the caller goes on
//!   without the store. A cut with the client's request unanswered also **opens the breaker**.
//! - **Breaker:** for [`Limits::cooldown`] after that, and after a failed connect to the store, every new connection is closed at
//!   once, so a dead store costs one stall and then nothing; after the cooldown the next connection tries the store again.
//! - **Idle keep-alive connections** are closed by the same rule without opening the breaker.
//! - **Not covered:** a stall in the middle of a response body is closed after a stall but does not open the breaker (the guard
//!   cannot tell it from a keep-alive pause without reading HTTP). Plain TCP only: a TLS stream is opaque to the guard.
//! - **Stop:** a connection whose first bytes are [`STOP_MAGIC`] ends the guard ([`stop`]); it also ends after
//!   [`Limits::idle_exit`] without a connection. At most [`MAX_CONNECTIONS`] connections are served at once.
//!
//! The rules are pure (`rules.rs`: [`Limits`], [`Breaker`], [`Watch`], every time passed in) and the sockets and threads are a thin
//! adapter ([`Guard`]). Tests break a server on purpose with `paddock_testkit::net::broken_server` (accept and never answer, drip,
//! cut in the middle of an answer, refuse).

mod proxy;
mod rules;

pub use proxy::{Guard, Log, StallGuardError, stop};
pub use rules::{Breaker, Limits, MAX_CONNECTIONS, STOP_MAGIC, Verdict, Watch, is_stop};

#[cfg(test)]
mod tests;
