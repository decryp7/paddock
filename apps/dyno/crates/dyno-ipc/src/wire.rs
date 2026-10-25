//! The frame envelope. Its body is the protocol's own message type `B` (the plugin protocol, the agent protocol).
//!
//! Compatibility: adding a variant at the end of an enum (here or in a body type) is a minor change; anything else
//! is a new [`PROTOCOL_VERSION`], refused in the handshake with a message. postcard is not self-describing, so wire
//! types never use `untagged`, `flatten`, internal tags or `skip_serializing_if`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The version of the envelope, sent in `Hello` and `Welcome`.
pub const PROTOCOL_VERSION: u32 = 1;

/// One frame on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Frame<B> {
    /// The first frame of the connecting side.
    Hello {
        /// Its [`PROTOCOL_VERSION`].
        version: u32,
        /// The token it was given on stdin.
        token: [u8; 32],
        /// Its process id (checked against the OS where possible).
        pid: u32,
    },
    /// The listening side accepted the `Hello`.
    Welcome {
        /// Its [`PROTOCOL_VERSION`].
        version: u32,
    },
    /// A request; ids are chosen by the caller, per direction.
    Call {
        /// The call id, echoed in the `Reply` or `Fail`.
        id: u64,
        /// The protocol's request.
        body: B,
    },
    /// The answer to the call `id`.
    Reply {
        /// The call id.
        id: u64,
        /// The protocol's reply.
        body: B,
    },
    /// The call `id` failed with a typed error (also sent with id 0 to refuse a handshake).
    Fail {
        /// The call id.
        id: u64,
        /// What went wrong.
        error: RemoteError,
    },
    /// An unsolicited event (live updates, alerts).
    Push {
        /// The protocol's event.
        body: B,
    },
}

/// A typed failure sent by the other side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteError {
    /// A short machine-readable code; see [`codes`].
    pub code: String,
    /// A message for people.
    pub message: String,
}

impl RemoteError {
    /// An error with `code` and `message`.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        RemoteError {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RemoteError {}

/// The codes the IPC itself uses. Protocols add their own.
pub mod codes {
    /// The handler panicked; the connection stays up but the callee is suspect.
    pub const PANIC: &str = "panic";
    /// This side has no handler for calls.
    pub const UNSUPPORTED: &str = "unsupported";
    /// The reply did not fit in a frame.
    pub const TOO_LARGE: &str = "tooLarge";
    /// Handshake refused: the protocol versions differ.
    pub const VERSION: &str = "version";
    /// Handshake refused: wrong token or wrong process.
    pub const AUTH: &str = "auth";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_errors_read_well() {
        let e = RemoteError::new("x", "it broke");
        assert_eq!(e.to_string(), "it broke (x)");
    }

    #[test]
    fn frames_roundtrip_and_the_handshake_decodes_without_a_body_type() {
        let f: Frame<String> = Frame::Call { id: 7, body: "hi".into() };
        let bytes = postcard::to_stdvec(&f).unwrap();
        assert_eq!(postcard::from_bytes::<Frame<String>>(&bytes).unwrap(), f);
        let hello: Frame<String> = Frame::Hello {
            version: 1,
            token: [3; 32],
            pid: 9,
        };
        let bytes = postcard::to_stdvec(&hello).unwrap();
        // The handshake is decoded as `Frame<()>`: the same bytes for every body type.
        assert_eq!(
            postcard::from_bytes::<Frame<()>>(&bytes).unwrap(),
            Frame::Hello {
                version: 1,
                token: [3; 32],
                pid: 9
            }
        );
    }
}
