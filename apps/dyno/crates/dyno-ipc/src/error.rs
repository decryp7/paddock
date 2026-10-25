//! The one error type of the IPC.

use crate::wire::RemoteError;
use std::io;
use std::time::Duration;

/// Why an IPC operation failed. `Clone`, so one connection failure can be handed to every waiting caller.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum IpcError {
    /// An operating-system error other than the connection closing.
    #[error("i/o error: {message}")]
    Io {
        /// The kind of the underlying error.
        kind: io::ErrorKind,
        /// Its message.
        message: String,
    },
    /// The other side closed the connection (or this side did).
    #[error("the connection is closed")]
    Closed,
    /// The other side sent something that is not a valid frame here (garbage, an oversized frame, a frame out of order).
    #[error("protocol error: {0}")]
    Protocol(String),
    /// No answer within the deadline.
    #[error("no answer within {0:?}")]
    Timeout(Duration),
    /// The other side answered the call with a typed failure.
    #[error("{0}")]
    Remote(RemoteError),
    /// The two sides speak different protocol versions.
    #[error("protocol version mismatch: this side speaks {ours}, the other side {theirs}; update the older one")]
    Version {
        /// This side's version.
        ours: u32,
        /// The other side's version.
        theirs: u32,
    },
    /// The handshake was refused: wrong token, or not the expected process.
    #[error("authentication failed: {0}")]
    Auth(String),
}

impl IpcError {
    /// An [`IpcError::Io`] with `kind` and `message`.
    pub fn io(kind: io::ErrorKind, message: impl Into<String>) -> Self {
        IpcError::Io {
            kind,
            message: message.into(),
        }
    }
}

impl From<io::Error> for IpcError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected => IpcError::Closed,
            kind => IpcError::io(kind, e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_stream_is_closed_and_other_errors_keep_their_kind() {
        assert_eq!(IpcError::from(io::Error::from(io::ErrorKind::BrokenPipe)), IpcError::Closed);
        assert_eq!(IpcError::from(io::Error::from(io::ErrorKind::UnexpectedEof)), IpcError::Closed);
        let e = IpcError::from(io::Error::new(io::ErrorKind::PermissionDenied, "no"));
        assert!(matches!(
            e,
            IpcError::Io {
                kind: io::ErrorKind::PermissionDenied,
                ..
            }
        ));
        assert!(e.to_string().contains("no"));
        assert!(IpcError::Version { ours: 1, theirs: 2 }.to_string().contains("update"));
    }
}
