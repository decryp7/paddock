//! Encoding of message bodies (postcard, like the frames) and [`Json`], for values that are JSON by contract.
//!
//! postcard is not self-describing: wire types must not use `#[serde(untagged)]`, `flatten`, internal tags or
//! `skip_serializing_if`, and cannot hold a bare `serde_json::Value`. Values that are JSON by nature (tool arguments and
//! results, settings) are wrapped in [`Json`], which crosses as JSON text. Compatibility: adding a variant at the end of
//! an enum is a minor protocol change; anything else is a new major ([`crate::frame::ProtocolVersion`]).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A body as bytes.
pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, String> {
    postcard::to_stdvec(value).map_err(|e| format!("cannot encode the message: {e}"))
}

/// A body from bytes; the whole input must be one value.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    match postcard::take_from_bytes::<T>(bytes) {
        Ok((v, [])) => Ok(v),
        Ok((_, rest)) => Err(format!("the message has {} trailing bytes", rest.len())),
        Err(e) => Err(format!("the message is not valid ({e})")),
    }
}

/// A JSON value on the wire: JSON text in binary formats (postcard), the value itself in human-readable ones (so a type
/// holding it serialises to the same JSON as before).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Json(pub Value);

impl Json {
    /// The value.
    pub fn into_inner(self) -> Value {
        self.0
    }
}

impl From<Value> for Json {
    fn from(v: Value) -> Self {
        Json(v)
    }
}

impl std::ops::Deref for Json {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.0
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            self.0.serialize(s)
        } else {
            s.serialize_str(&self.0.to_string())
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            Value::deserialize(d).map(Json)
        } else {
            let text = String::deserialize(d)?;
            serde_json::from_str(&text)
                .map(Json)
                .map_err(serde::de::Error::custom)
        }
    }
}
