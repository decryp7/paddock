//! `FieldPath`: which list of a reply a cut is about (Debrief ADR 0065 section 1).

use crate::bounded::BoundedError;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cmp::Ordering;

/// The longest [`FieldPath`].
pub const MAX_FIELD_PATH: usize = 64;

/// A dotted path of field names in the reply's camelCase (`durationMs`, `callees.selfMs`), where a part may carry one
/// index into a list (`groups[3].ids`). Parsed once, at most 64 bytes; never shown to a person (the words come from the
/// message), never starting with `result`. The empty path means the value itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FieldPath(Cow<'static, str>);

const fn valid_path(b: &[u8]) -> bool {
    if b.is_empty() {
        return true;
    }
    if b.len() > MAX_FIELD_PATH {
        return false;
    }
    let mut i = 0;
    let n = b.len();
    // `result` as the first part would restate the wrapper.
    if n >= 6
        && b[0] == b'r'
        && b[1] == b'e'
        && b[2] == b's'
        && b[3] == b'u'
        && b[4] == b'l'
        && b[5] == b't'
        && (n == 6 || b[6] == b'.' || b[6] == b'[')
    {
        return false;
    }
    loop {
        if i >= n || !b[i].is_ascii_lowercase() {
            return false;
        }
        i += 1;
        while i < n && (b[i].is_ascii_alphanumeric()) {
            i += 1;
        }
        if i < n && b[i] == b'[' {
            i += 1;
            let start = i;
            let mut value: u64 = 0;
            // No leading zero: `[03]` would order like `[3]` and not be equal to it.
            if i + 1 < n && b[i] == b'0' && b[i + 1].is_ascii_digit() {
                return false;
            }
            while i < n && b[i].is_ascii_digit() {
                value = value * 10 + (b[i] - b'0') as u64;
                if value > u32::MAX as u64 {
                    return false;
                }
                i += 1;
            }
            if i == start || i >= n || b[i] != b']' {
                return false;
            }
            i += 1;
        }
        if i == n {
            return true;
        }
        if b[i] != b'.' {
            return false;
        }
        i += 1;
    }
}

impl FieldPath {
    /// A path from a literal; in a `const` a bad one fails the build.
    ///
    /// # Panics
    /// If `path` is not a field path. Paths are literals chosen by the programmer, never input.
    #[must_use]
    pub const fn lit(path: &'static str) -> Self {
        assert!(valid_path(path.as_bytes()), "not a field path");
        Self(Cow::Borrowed(path))
    }

    /// The value itself.
    #[must_use]
    pub const fn root() -> Self {
        Self(Cow::Borrowed(""))
    }

    /// A path from text outside the program.
    ///
    /// # Errors
    /// [`BoundedError::Path`].
    pub fn parse(path: &str) -> Result<Self, BoundedError> {
        if valid_path(path.as_bytes()) {
            Ok(Self(Cow::Owned(path.to_owned())))
        } else {
            Err(BoundedError::Path)
        }
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for FieldPath {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for FieldPath {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <Cow<'de, str>>::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "ts")]
impl ts_rs::TS for FieldPath {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;
    fn name() -> String {
        "FieldPath".into()
    }
    fn inline() -> String {
        "string".into()
    }
    fn inline_flattened() -> String {
        "string".into()
    }
    fn decl() -> String {
        "type FieldPath = string;".into()
    }
    fn decl_concrete() -> String {
        "type FieldPath = string;".into()
    }
}

impl FieldPath {
    /// The parts in order: a field name and the index after it, if any (`groups[3].ids` is `("groups", Some(3))`, `("ids", None)`).
    fn parts(&self) -> impl Iterator<Item = (&str, Option<u32>)> {
        self.as_str()
            .split('.')
            .filter(|p| !p.is_empty())
            .map(|part| match part.split_once('[') {
                Some((name, rest)) => (name, rest.trim_end_matches(']').parse().ok()),
                None => (part, None),
            })
    }
}

impl Ord for FieldPath {
    /// The structural order: a path before the paths inside it, siblings by name, the entries of one list by index.
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts().cmp(other.parts())
    }
}

impl PartialOrd for FieldPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
