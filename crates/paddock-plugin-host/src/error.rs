//! Errors of calling a plugin.

use paddock_plugin_api::protocol::Fail;
use paddock_ipc::RemoteError;
use std::fmt;

/// What a fault is: the plugin misbehaved, the process is (or was) in trouble. Each one is counted towards quarantine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// It panicked inside a contribution, or its process ended.
    Crash,
    /// A call overran its budget and the plugin ignored the cancel.
    Hang,
    /// A frame it sent was oversized or garbled, or a reply or document did not decode or validate.
    Garbled,
}

/// Why a call to a plugin did not give its result.
#[derive(Debug, Clone, PartialEq)]
pub enum HostError {
    /// The plugin cannot be called now (not enabled, refused, quarantined, restarting); the text says why and what to do.
    Unavailable(String),
    /// The plugin misbehaved: counted towards quarantine and attributed to it.
    Fault {
        /// What kind.
        kind: FaultKind,
        /// What happened.
        message: String,
    },
    /// The plugin answered a typed failure: the call failed, the plugin is fine, nothing is counted.
    Failed(Fail),
    /// The plugin refused the request with a typed error (invalid request, unknown namespace, a newer minor): not counted.
    Remote(RemoteError),
    /// The request was wrong before it was sent (arguments that do not match the schema, a path outside the grants, an
    /// unknown tool). Nothing was sent.
    Invalid(String),
    /// The call was cancelled.
    Cancelled,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Unavailable(m) | HostError::Invalid(m) => f.write_str(m),
            HostError::Fault { message, .. } => f.write_str(message),
            HostError::Failed(x) => f.write_str(&x.message),
            HostError::Remote(r) => f.write_str(&r.message),
            HostError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for HostError {}
