//! Outbound HTTP policy for the Paddock apps (plan X16): the one door every app request goes through, from Spotter's `guard.rs`
//! and `fetch.rs`, Pitwall's `egress.rs` and Pitcrew's `net.rs`.
//!
//! - [`Policy`]: how far the app may reach ([`Reach`]: public only, plus private networks, plus this computer), redirect limit,
//!   body caps, timeouts, host allow-list ([`HostPattern`]), least gap between requests to one host.
//! - [`Egress`]: `send` (answer read and capped) and `stream` (answer unread, for downloads). Before every hop, redirects
//!   included: https only (plain http to this computer when the reach allows it), no login in the URL, host on the allow-list, and
//!   every address of the name within the reach (**all** of them: a name that points at a forbidden one even once is refused);
//!   link-local (169.254.169.254 and friends), unspecified and multicast addresses are never reachable. A redirect may never go
//!   farther than where the request started, never downgrades to http, drops `Authorization` and cookies across hosts, and is
//!   followed by hand (the wire never follows one). [`Egress::set_offline`] refuses before a name is resolved or a socket opened.
//! - [`Wire`] / [`Lookup`]: the ports to the network and to name resolution. The real wire ([`UreqWire`], feature `http`) uses
//!   `paddock-download`'s TLS and proxy set-up, applies the reach to the addresses the socket connects to (DNS rebinding), and keeps
//!   one connection pool per proxy and reach. [`FakeWire`] and [`StaticLookup`] are for tests.
//! - [`Egress::download_transport`]: the same policy as a `paddock_download::Transport`.
//!
//! Requests block; run them in a job (rule 10). Conditional requests: [`Request::if_none_match`], [`Response::not_modified`].

mod egress;
mod error;
mod fake;
mod policy;
mod reach;
mod request;
#[cfg(feature = "http")]
mod ureq_wire;
mod wire;

pub use egress::{DownloadTransport, Egress, Streaming};
pub use error::NetError;
pub use fake::{FakeWire, Reply, Seen, StaticLookup};
pub use policy::{HostPattern, MAX_PATTERNS, MAX_REDIRECTS, Policy};
pub use reach::{Class, Reach, Refusal, classify, host_is_loopback, vet};
pub use request::{Method, Request, Response};
#[cfg(feature = "http")]
pub use ureq_wire::UreqWire;
pub use wire::{Lookup, SystemLookup, Wire, WireRequest, WireResponse};
