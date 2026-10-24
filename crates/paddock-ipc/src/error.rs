//! Errors of the IPC: a typed [`RemoteError`] the other side answers with (it crosses as a `Fail` frame), and [`IpcError`]
//! for every failed operation. A typed error is [`IpcError::Remote`] (the call failed, the process is fine); a broken
//! connection is [`IpcError::Closed`] (a crash); a deadline the callee ignored is [`IpcError::Timeout`] (a hang); one
//! frame over the limit is [`IpcError::TooLarge`] and fails only that call.

use crate::wire::Json;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;

/// The codes the IPC itself uses in [`RemoteError::code`]. Protocols add their own (any other string).
pub mod codes {
    /// The callee does not implement this request (or answers no calls at all).
    pub const UNIMPLEMENTED: &str = "unimplemented";
    /// The request did not decode or is not valid.
    pub const INVALID: &str = "invalid";
    /// The callee failed in a way it did not classify.
    pub const INTERNAL: &str = "internal";
    /// The callee panicked while handling the call (it still runs; the panic counts as a crash).
    pub const PANIC: &str = "panic";
    /// The call was cancelled.
    pub const CANCELLED: &str = "cancelled";
    /// Too many calls are running on this connection.
    pub const BUSY: &str = "busy";
    /// A request or answer did not fit in one frame.
    pub const TOO_LARGE: &str = "tooLarge";
    /// Handshake refused: the protocol versions do not fit.
    pub const VERSION: &str = "version";
    /// Handshake refused: wrong token or wrong process.
    pub const AUTH: &str = "auth";
}

/// A typed error the other side returns for a call (not a failure of the process itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteError {
    /// A short machine-readable code: one of [`codes`] or the protocol's own.
    pub code: String,
    /// A message for people.
    pub message: String,
    /// Structured details (JSON), e.g. `{"reason": "noRuntime"}`.
    pub data: Option<Json>,
}

impl RemoteError {
    /// An error with `code` and `message` and no details.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        RemoteError {
            code: code.into(),
            message: message.into(),
            data: None,
        }
    }

    /// The same error with structured details.
    pub fn with_data(mut self, data: impl Into<Json>) -> Self {
        self.data = Some(data.into());
        self
    }

    /// Whether the code is `code`.
    pub fn is(&self, code: &str) -> bool {
        self.code == code
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for RemoteError {}

/// What went wrong with an IPC operation. `Clone`, so one connection failure is handed to every waiting caller.
#[derive(Debug, Clone, PartialEq)]
pub enum IpcError {
    /// The process could not be started (or exited before it connected).
    Spawn(String),
    /// The connection ended: the other side exited or closed it, or this side did (a crash, from the caller's view).
    Closed(String),
    /// No answer within the deadline, or the callee ignored the cancel (a hang).
    Timeout,
    /// The other side sent something that is not valid protocol here (garbage, a frame out of order, a repeated handshake).
    Protocol(String),
    /// One frame (this call's request or answer) is larger than the connection allows; only this call failed.
    TooLarge {
        /// The frame's length in bytes.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// The other side answered the call with a typed error.
    Remote(RemoteError),
    /// The two sides speak protocol versions that do not fit; the message says which side to update.
    Version(String),
    /// The handshake was refused: wrong token, or not the expected process.
    Auth(String),
    /// The work needs rights the user did not give. Not a failure of the process.
    Denied(String),
    /// An operating-system error other than the connection ending.
    Io {
        /// The kind of the underlying error.
        kind: io::ErrorKind,
        /// Its message.
        message: String,
    },
}

impl IpcError {
    /// An [`IpcError::Io`].
    pub fn io(kind: io::ErrorKind, message: impl Into<String>) -> Self {
        IpcError::Io {
            kind,
            message: message.into(),
        }
    }

    /// Whether the process itself failed (crash, hang, garbage), so a supervisor restarts it. A typed error, a refused
    /// right or one oversized frame is not a process failure.
    pub fn is_process_failure(&self) -> bool {
        !matches!(
            self,
            IpcError::Remote(_) | IpcError::Denied(_) | IpcError::TooLarge { .. }
        )
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpcError::Spawn(m) | IpcError::Denied(m) | IpcError::Version(m) => f.write_str(m),
            IpcError::Closed(m) => write!(f, "the connection ended ({m})"),
            IpcError::Timeout => f.write_str("it stopped responding"),
            IpcError::Protocol(m) => write!(f, "it sent an invalid message ({m})"),
            IpcError::TooLarge { len, max } => write!(
                f,
                "a message of {len} bytes exceeds the limit of {max} bytes"
            ),
            IpcError::Remote(r) => f.write_str(&r.message),
            IpcError::Auth(m) => write!(f, "authentication failed ({m})"),
            IpcError::Io { message, .. } => write!(f, "i/o error ({message})"),
        }
    }
}

impl std::error::Error for IpcError {}

impl From<io::Error> for IpcError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected => IpcError::Closed(e.to_string()),
            kind => IpcError::io(kind, e.to_string()),
        }
    }
}

/// The message of a panic payload (`&str` or `String`), for errors that report a contained panic.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".into()
    }
}

impl From<crate::frame::FrameError> for IpcError {
    /// A frame that cannot be written: over the limit is [`IpcError::TooLarge`] (only that call fails), anything else a
    /// protocol error.
    fn from(e: crate::frame::FrameError) -> Self {
        match e {
            crate::frame::FrameError::TooLarge { len, max } => IpcError::TooLarge { len, max },
            other => IpcError::Protocol(other.to_string()),
        }
    }
}

impl IpcError {
    /// This error as the typed failure a handler answers with: `tooLarge` for one oversized frame, else `internal`.
    pub fn to_remote(&self) -> RemoteError {
        let code = if matches!(self, IpcError::TooLarge { .. }) {
            codes::TOO_LARGE
        } else {
            codes::INTERNAL
        };
        RemoteError::new(code, self.to_string())
    }
}
