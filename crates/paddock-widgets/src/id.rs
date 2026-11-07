//! Placement and preset ids: parsed once, valid ever after.

use serde::{Deserialize, Serialize};

/// Longest placement id, preset id and preset name, in characters.
pub const MAX_ID: usize = 64;

ts! {
/// A placement's id: 1 to [`MAX_ID`] characters of `[a-z0-9-]`, unique within its layout. Parsed once at the edge.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(any(test, feature = "ts"), ts(type = "string"))]
pub struct PlacementId(pub(crate) std::sync::Arc<str>);

/// A user preset's id, same rules as [`PlacementId`]; never the id of a built-in preset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(any(test, feature = "ts"), ts(type = "string"))]
pub struct PresetId(pub(crate) std::sync::Arc<str>);
}

/// Why a string is not an id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("An id is 1 to {MAX_ID} characters of a-z, 0-9 and -.")]
pub struct BadId;

fn check_id(s: &str) -> Result<(), BadId> {
    let ok = !s.is_empty()
        && s.len() <= MAX_ID
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok { Ok(()) } else { Err(BadId) }
}

macro_rules! id_conversions {
    ($t:ident) => {
        impl TryFrom<String> for $t {
            type Error = BadId;
            fn try_from(s: String) -> Result<Self, BadId> {
                check_id(&s).map(|()| $t(s.into()))
            }
        }
        impl From<$t> for String {
            fn from(id: $t) -> String {
                id.0.to_string()
            }
        }
        impl Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                $t::try_from(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
        impl std::fmt::Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl $t {
            /// Parses an id from text.
            pub fn new(text: &str) -> Result<Self, BadId> {
                check_id(text).map(|()| $t(text.into()))
            }

            /// The id as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}
id_conversions!(PlacementId);
id_conversions!(PresetId);

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
pub(crate) fn unique_id(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut candidate = base.to_string();
    let mut n = 2u32;
    while taken(&candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    // `base` is a slug of at most 56 characters and the suffix has at most 8: always a valid id.
    candidate
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
        assert_eq!(id, "disk-3");
        assert_eq!(unique_id("disk", |_| false), "disk");
    }
}
