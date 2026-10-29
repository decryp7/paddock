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
    /// The message could not be serialised (a type that postcard cannot write).
    #[error("the message could not be encoded: {0}")]
    Encode(String),
    /// The bytes are not a message of this type, or are truncated.
    #[error("the bytes are not a valid message: {0}")]
    Malformed(String),
    /// Bytes remain after a complete message.
    #[error("{0} unexpected bytes follow the message")]
    TrailingBytes(usize),
}

/// A postcard output that refuses to grow past [`MAX_MESSAGE_BYTES`], so an oversized message stops while it is being
/// written and is never held whole.
struct Bounded(Vec<u8>);

impl postcard::ser_flavors::Flavor for Bounded {
    type Output = Vec<u8>;

    fn try_push(&mut self, data: u8) -> postcard::Result<()> {
        if self.0.len() >= MAX_MESSAGE_BYTES {
            return Err(postcard::Error::SerializeBufferFull);
        }
        self.0.push(data);
        Ok(())
    }

    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(self.0)
    }
}

/// Encodes `message`; stops as soon as it would exceed [`MAX_MESSAGE_BYTES`].
pub fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, CodecError> {
    postcard::serialize_with_flavor(message, Bounded(Vec::new())).map_err(|e| match e {
        postcard::Error::SerializeBufferFull => CodecError::TooLarge(MAX_MESSAGE_BYTES + 1),
        other => CodecError::Encode(other.to_string()),
    })
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

    fn budget() -> Budget {
        Budget::new(30, 10, 1000).unwrap()
    }

    #[test]
    fn a_message_round_trips_and_extra_or_missing_bytes_are_errors() {
        let m = HostRequest::new(
            7,
            HostCall::FileId {
                path: RepoPath::parse("a/b.rs").unwrap(),
            },
            budget(),
        )
        .unwrap();
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
    fn size_is_bounded_both_ways_and_encoding_stops_while_writing() {
        let big = FunctionRequest {
            id: 1,
            tool: "t".into(),
            arguments: "x".repeat(MAX_MESSAGE_BYTES),
            budget: budget(),
        };
        assert!(matches!(encode(&big), Err(CodecError::TooLarge(_))));
        let just_fits = FunctionRequest {
            id: 1,
            tool: "t".into(),
            arguments: "x".repeat(MAX_MESSAGE_BYTES - 64),
            budget: budget(),
        };
        assert!(encode(&just_fits).is_ok());
        assert!(matches!(
            decode::<FunctionRequest>(&vec![0u8; MAX_MESSAGE_BYTES + 1]),
            Err(CodecError::TooLarge(_))
        ));
    }

    #[test]
    fn a_type_postcard_cannot_write_is_an_encode_error_not_a_malformed_message() {
        // A map with a non-string-convertible key length is fine, but a value that fails to serialise is not Malformed.
        struct Fails;
        impl Serialize for Fails {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("no"))
            }
        }
        assert!(matches!(encode(&Fails), Err(CodecError::Encode(_))));
    }

    #[test]
    fn decoding_never_panics_on_hostile_bytes() {
        let m = FunctionRequest {
            id: 9,
            tool: "find_symbol".into(),
            arguments: "{\"q\":\"x\"}".into(),
            budget: budget(),
        };
        let seed = encode(&m).unwrap();
        paddock_testkit::assert_never_panics(&[seed.as_slice()], |b| decode::<FunctionRequest>(b).is_ok());
    }
}
