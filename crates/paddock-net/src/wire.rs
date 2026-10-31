//! The ports to the network and to name resolution.

use crate::error::NetError;
use crate::reach::{LanAccess, Reach};
use crate::request::Method;
use paddock_download::Header;
use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

/// One request to send, already checked by the policy.
#[derive(Debug, Clone, Copy)]
pub struct WireRequest<'a> {
    /// The method.
    pub method: Method,
    /// The address.
    pub url: &'a str,
    /// The farthest this hop may reach (the policy's reach, lowered by where the request started); the wire applies it to the
    /// addresses it connects to.
    pub reach: Reach,
    /// How private-network addresses are treated; the wire applies it to the addresses it connects to, like `reach`.
    pub lan: LanAccess,
    /// The headers to send (the wire adds `Host`, framing and `Accept-Encoding: identity`).
    pub headers: &'a [Header],
    /// The body.
    pub body: &'a [u8],
    /// The `User-Agent`.
    pub user_agent: &'a str,
    /// Longest to wait for a connection.
    pub connect_timeout: Duration,
    /// Longest for the whole request including the body; `None` for a streaming answer.
    pub timeout: Option<Duration>,
    /// Longest to wait for the status line and headers.
    pub response_timeout: Duration,
    /// Longest to wait for the body to continue (streaming answers).
    pub body_timeout: Option<Duration>,
}

/// A status, the headers (names lower-cased) and the body.
pub struct WireResponse {
    /// The status.
    pub status: u16,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The body; dropping it closes the connection.
    pub body: Box<dyn Read + Send>,
}

/// The network. Contract: **never follow a redirect** (return the 3xx), never decompress, return any status as a response (only a
/// failure without a status line is an error), connect only to addresses the policy's reach allows (the check applies to the
/// addresses the socket connects to, so a name that answers differently the second time is still refused), send no header
/// the request does not list. Called from several threads at once.
pub trait Wire: Send + Sync {
    /// Sends the request.
    fn send(&self, request: &WireRequest<'_>) -> Result<WireResponse, NetError>;
}

/// Name resolution for the check before each hop; injected so tests answer without a network.
pub trait Lookup: Send + Sync {
    /// Every address of `host`.
    fn lookup(&self, host: &str, port: u16) -> std::io::Result<Vec<IpAddr>>;
}

/// The operating system's resolver.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemLookup;

impl Lookup for SystemLookup {
    fn lookup(&self, host: &str, port: u16) -> std::io::Result<Vec<IpAddr>> {
        Ok((host, port).to_socket_addrs()?.map(|a| a.ip()).collect())
    }
}
