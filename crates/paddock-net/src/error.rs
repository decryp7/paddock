//! Why a request did not happen or did not finish.

use crate::reach::Refusal;
use std::fmt;

/// Why a request failed. Messages say what happened and what to do, and never contain a query string, a login or a header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The app is offline: nothing was resolved and no socket was opened.
    Offline,
    /// The address is not usable (not a URL, a login in the URL, no host).
    Invalid(String),
    /// A scheme other than https (or http to this computer).
    Scheme(String),
    /// The host is not on the allow-list.
    NotAllowed(String),
    /// The host resolves to an address the app may not reach.
    Refused {
        /// The host.
        host: String,
        /// Why.
        why: Refusal,
    },
    /// More redirects than allowed.
    /// More redirects than the policy allowed (the number it allowed).
    TooManyRedirects(u32),
    /// A redirect to a forbidden place (plain http from https, a farther network, a host off the allow-list).
    BadRedirect(String),
    /// The body is larger than the limit (bytes).
    TooLarge(u64),
    /// The host could not be reached.
    Connect(String),
    /// No answer in time.
    Timeout(String),
    /// The secure connection could not be verified.
    Tls(String),
    /// The proxy failed.
    Proxy(String),
    /// Stopped on request.
    Cancelled,
}

impl NetError {
    /// Whether another attempt can help (the network, not the policy).
    pub fn retryable(&self) -> bool {
        matches!(self, NetError::Connect(_) | NetError::Timeout(_))
    }
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Offline => {
                f.write_str("The app is offline; go online in Settings to reach the network.")
            }
            NetError::Invalid(why) => f.write_str(why),
            NetError::Scheme(host) => write!(
                f,
                "{host} must be reached over https:// (plain http is only for this computer)."
            ),
            NetError::NotAllowed(host) => {
                write!(f, "{host} is not one of the hosts this app may contact.")
            }
            NetError::Refused { host, why } => write!(f, "{host} was not contacted: {why}."),
            NetError::TooManyRedirects(_) => f.write_str("The server redirected too many times."),
            NetError::BadRedirect(why) => write!(f, "A redirect was refused: {why}."),
            NetError::TooLarge(limit) => {
                write!(f, "The answer is larger than the {limit} byte limit.")
            }
            NetError::Connect(host) => write!(
                f,
                "{host} could not be reached. Check the address and that this computer is online."
            ),
            NetError::Timeout(host) => write!(f, "{host} did not answer in time."),
            NetError::Tls(host) => write!(
                f,
                "A secure connection to {host} could not be verified. Check the computer's clock and the proxy's certificate."
            ),
            NetError::Proxy(why) => {
                write!(f, "The proxy failed ({why}). Check the proxy settings.")
            }
            NetError::Cancelled => f.write_str("Stopped."),
        }
    }
}

impl std::error::Error for NetError {}
