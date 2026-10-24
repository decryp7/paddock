//! JSON and postcard encoding of the wire types, with the size limit and a nesting guard on decode.
//!
//! Both decoders refuse input larger than [`MAX_ENCODED`](crate::MAX_ENCODED) before they look at it and stop nesting at
//! [`DECODE_DEPTH`]: postcard has no recursion limit of its own, and a few hundred kilobytes of nested stacks would
//! otherwise overflow the decoder's stack.

use crate::{MAX_ENCODED, Node, NodeKind, ViewError};
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use std::cell::Cell;

/// How deep nodes may nest while decoding (the validator's limit is lower; this one only protects the stack).
pub const DECODE_DEPTH: u32 = 64;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Counts one nesting level for as long as it lives.
struct Level;

impl Level {
    fn enter<E: serde::de::Error>() -> Result<Level, E> {
        DEPTH.with(|d| {
            if d.get() >= DECODE_DEPTH {
                Err(E::custom("nodes are nested too deeply"))
            } else {
                d.set(d.get() + 1);
                Ok(Level)
            }
        })
    }
}

impl Drop for Level {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

impl<'de> Deserialize<'de> for Node {
    /// Decodes a node inside a nesting level, so a document of nested stacks fails instead of overflowing the stack.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Repr {
            id: String,
            kind: NodeKind,
        }
        let _level = Level::enter::<D::Error>()?;
        let Repr { id, kind } = Repr::deserialize(d)?;
        Ok(Node { id, kind })
    }
}

/// The value as JSON text. Fails when it is larger than the encoded limit.
pub fn to_json<T: Serialize>(value: &T) -> Result<String, ViewError> {
    let text = serde_json::to_string(value).map_err(|e| ViewError::Decode(e.to_string()))?;
    if text.len() > MAX_ENCODED {
        return Err(ViewError::TooLarge { bytes: text.len() });
    }
    Ok(text)
}

/// A value from JSON text. Does not validate it (see [`crate::validate`]).
pub fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, ViewError> {
    if text.len() > MAX_ENCODED {
        return Err(ViewError::TooLarge { bytes: text.len() });
    }
    serde_json::from_str(text).map_err(|e| ViewError::Decode(e.to_string()))
}

/// The value as postcard bytes (the IPC encoding). Fails when it is larger than the encoded limit.
pub fn to_postcard<T: Serialize>(value: &T) -> Result<Vec<u8>, ViewError> {
    let bytes = postcard::to_allocvec(value).map_err(|e| ViewError::Decode(e.to_string()))?;
    if bytes.len() > MAX_ENCODED {
        return Err(ViewError::TooLarge { bytes: bytes.len() });
    }
    Ok(bytes)
}

/// A value from postcard bytes; trailing bytes are refused. Does not validate it (see [`crate::validate`]).
pub fn from_postcard<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ViewError> {
    if bytes.len() > MAX_ENCODED {
        return Err(ViewError::TooLarge { bytes: bytes.len() });
    }
    let (value, rest) = postcard::take_from_bytes::<T>(bytes).map_err(|e| ViewError::Decode(e.to_string()))?;
    if rest.is_empty() {
        Ok(value)
    } else {
        Err(ViewError::Decode(format!("{} bytes after the value", rest.len())))
    }
}
