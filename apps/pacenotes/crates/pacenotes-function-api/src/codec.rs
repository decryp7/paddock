//! The codec: postcard bytes, bounded on both sides.

use serde::{Serialize, de::DeserializeOwned};

/// The largest message accepted or produced: 4 MiB. Bigger results are paged by the caller.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// Why a message could not be encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    /// The message is larger than [`MAX_MESSAGE_BYTES`].
    #[error("the message is {0} bytes; at most {MAX_MESSAGE_BYTES} are allowed")]
    TooLarge(usize),
    /// The bytes are not a message of this type, or are truncated.
    #[error("the bytes are not a valid message: {0}")]
    Malformed(String),
    /// Bytes remain after a complete message.
    #[error("{0} unexpected bytes follow the message")]
    TrailingBytes(usize),
}

/// Encodes `message`.
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, CodecError> {
    let bytes = postcard::to_stdvec(message).map_err(|e| CodecError::Malformed(e.to_string()))?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge(bytes.len()));
    }
    Ok(bytes)
}

/// Decodes a message of type `T`. The size is checked before anything is read, and the whole input must be used.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge(bytes.len()));
    }
    let (value, rest) = postcard::take_from_bytes::<T>(bytes).map_err(|e| CodecError::Malformed(e.to_string()))?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(CodecError::TrailingBytes(rest.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{FunctionRequest, HostCall};
    use crate::{Budget, HostRequest};
    use pacenotes_core::RepoPath;

    #[test]
    fn a_message_round_trips_and_extra_or_missing_bytes_are_errors() {
        let m = HostRequest {
            id: 7,
            call: HostCall::FileId {
                path: RepoPath::parse("a/b.rs").unwrap(),
            },
            budget: Budget::INTERACTIVE,
        };
        let bytes = encode(&m).unwrap();
        assert_eq!(decode::<HostRequest>(&bytes).unwrap(), m);
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(decode::<HostRequest>(&extra), Err(CodecError::TrailingBytes(1)));
        assert!(matches!(
            decode::<HostRequest>(&bytes[..bytes.len() - 1]),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn size_is_bounded_both_ways() {
        let big = FunctionRequest {
            id: 1,
            tool: "t".into(),
            arguments: "x".repeat(MAX_MESSAGE_BYTES),
            budget: Budget::INTERACTIVE,
        };
        assert!(matches!(encode(&big), Err(CodecError::TooLarge(_))));
        assert!(matches!(
            decode::<FunctionRequest>(&vec![0u8; MAX_MESSAGE_BYTES + 1]),
            Err(CodecError::TooLarge(_))
        ));
    }

    #[test]
    fn decoding_never_panics_on_hostile_bytes() {
        let m = FunctionRequest {
            id: 9,
            tool: "find_symbol".into(),
            arguments: "{\"q\":\"x\"}".into(),
            budget: Budget::INTERACTIVE,
        };
        let seed = encode(&m).unwrap();
        paddock_testkit::assert_never_panics(&[seed.as_slice()], |b| decode::<FunctionRequest>(b).is_ok());
        paddock_testkit::assert_never_panics(&[seed.as_slice()], |b| decode::<crate::HostReply>(b).is_ok());
    }
}
