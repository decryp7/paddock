//! Placement and preset ids: parsed once, valid ever after.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::error::WidgetError;

/// The longest id, in characters.
pub const MAX_ID: usize = 64;

/// An id of 1 to 64 characters of `a-z`, `0-9` and `-`. Placement ids are unique within a layout and preset ids within
/// a surface. Parsed at the edge (`Deserialize` goes through [`PlacementId::new`]), so a held id is always valid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, TS)]
#[ts(as = "String")]
pub struct PlacementId(String);

impl PlacementId {
    /// Parses an id; [`WidgetError::BadId`] when it is empty, too long or has other characters.
    pub fn new(text: &str) -> Result<PlacementId, WidgetError> {
        let ok = !text.is_empty()
            && text.len() <= MAX_ID
            && text
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ok {
            Ok(PlacementId(text.to_string()))
        } else {
            Err(WidgetError::BadId)
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PlacementId {
    type Error = WidgetError;
    fn try_from(text: String) -> Result<PlacementId, WidgetError> {
        PlacementId::new(&text)
    }
}

impl Serialize for PlacementId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PlacementId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        PlacementId::new(&text).map_err(D::Error::custom)
    }
}

impl From<PlacementId> for String {
    fn from(id: PlacementId) -> String {
        id.0
    }
}

impl fmt::Display for PlacementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lower-case `text` into `a-z0-9-`: other characters become one hyphen, camelCase words are split, the ends are
/// trimmed, and `fallback` stands in for an empty result. At most 56 characters, so a numeric suffix always fits.
pub(crate) fn slug(text: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    let mut prev_lower = false;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if (c.is_ascii_uppercase() && prev_lower) || (pending_dash && !out.is_empty()) {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
            pending_dash = false;
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else {
            pending_dash = true;
            prev_lower = false;
        }
        if out.len() >= 56 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

/// `base`, or `base-2`, `base-3`... the first that `taken` does not report.
pub(crate) fn unique_id(base: &str, taken: impl Fn(&str) -> bool) -> PlacementId {
    let mut candidate = base.to_string();
    let mut n = 2u32;
    while taken(&candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    // `base` is a slug of at most 56 characters and the suffix has at most 8: always valid.
    PlacementId(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_splits_camel_case_and_trims() {
        assert_eq!(slug("diskSpace", "x"), "disk-space");
        assert_eq!(slug("  My  layout! ", "x"), "my-layout");
        assert_eq!(slug("CPU", "x"), "cpu");
        assert_eq!(slug("v2 Beta", "x"), "v2-beta");
    }

    #[test]
    fn slug_falls_back_and_is_bounded() {
        assert_eq!(slug("", "widget"), "widget");
        assert_eq!(slug("---", "layout"), "layout");
        assert_eq!(slug("é", "layout"), "layout");
        assert!(slug(&"a".repeat(200), "x").len() <= 56);
        assert!(PlacementId::new(&slug(&"ab ".repeat(100), "x")).is_ok());
    }

    #[test]
    fn unique_id_adds_a_suffix_until_free() {
        let id = unique_id("disk", |c| c == "disk" || c == "disk-2");
        assert_eq!(id.as_str(), "disk-3");
        assert_eq!(unique_id("disk", |_| false).as_str(), "disk");
    }
}
