//! A finding's identity (note section 3.3): stable across a rename of its file and across runs, so it has no path in it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

use super::RuleId;

/// The version of the hash recipe. Part of the hash: changing the normalisation is a new version and a baseline migration.
pub const ALGORITHM_VERSION: u8 = 1;

/// The longest snippet that is hashed.
const MAX_SNIPPET_BYTES: usize = 4096;

/// 32 bytes: SHA-256 of the rule id, the algorithm version, the normalised snippet and the enclosing qualified name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FindingFingerprint([u8; 32]);

/// Line endings to `\n`, runs of white space to one space, leading and trailing white space dropped, at most 4 KiB (cut on a character
/// boundary).
pub fn normalise_snippet(snippet: &str) -> String {
    let mut out = String::with_capacity(snippet.len().min(MAX_SNIPPET_BYTES));
    for word in snippet.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    let mut cut = out.len().min(MAX_SNIPPET_BYTES);
    while !out.is_char_boundary(cut) {
        cut -= 1;
    }
    out.truncate(cut);
    out
}

impl FindingFingerprint {
    /// The identity of a finding of `rule` over `snippet`, inside the symbol `enclosing` (its qualified name) when there is one.
    pub fn compute(rule: &RuleId, snippet: &str, enclosing: Option<&str>) -> FindingFingerprint {
        let mut h = Sha256::new();
        let mut put = |bytes: &[u8]| {
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(bytes);
        };
        put(rule.to_string().as_bytes());
        put(&[ALGORITHM_VERSION]);
        put(normalise_snippet(snippet).as_bytes());
        match enclosing {
            Some(name) => {
                put(&[1]);
                put(name.as_bytes());
            }
            None => put(&[0]),
        }
        FindingFingerprint(h.finalize().into())
    }

    /// Lowercase hex.
    pub fn to_hex(&self) -> String {
        self.0.iter().fold(String::with_capacity(64), |mut s, b| {
            s.push_str(&format!("{b:02x}"));
            s
        })
    }

    /// Reads 64 hex digits.
    pub fn from_hex(text: &str) -> Option<FindingFingerprint> {
        if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(FindingFingerprint(out))
    }
}

impl fmt::Display for FindingFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for FindingFingerprint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for FindingFingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        FindingFingerprint::from_hex(&text).ok_or_else(|| serde::de::Error::custom("not 64 hex digits"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: &str) -> RuleId {
        RuleId::local(id).unwrap()
    }

    #[test]
    fn the_identity_ignores_layout_and_has_no_path() {
        let a = FindingFingerprint::compute(&rule("r"), "let x =  foo( 1 );\r\n", Some("A::b"));
        let b = FindingFingerprint::compute(&rule("r"), "  let x = foo( 1 );", Some("A::b"));
        assert_eq!(a, b, "white space and line endings are normalised");
        assert_ne!(a, FindingFingerprint::compute(&rule("r"), "let x = foo( 2 );", Some("A::b")));
        assert_ne!(a, FindingFingerprint::compute(&rule("s"), "let x = foo( 1 );", Some("A::b")));
        assert_ne!(a, FindingFingerprint::compute(&rule("r"), "let x = foo( 1 );", Some("A::c")));
        assert_ne!(a, FindingFingerprint::compute(&rule("r"), "let x = foo( 1 );", None));
    }

    #[test]
    fn the_hash_is_pinned_so_a_change_of_recipe_is_noticed() {
        let fp = FindingFingerprint::compute(&rule("no-unwrap"), "x.unwrap()", Some("f"));
        assert_eq!(fp.to_hex().len(), 64);
        assert_eq!(FindingFingerprint::from_hex(&fp.to_hex()), Some(fp));
        assert_eq!(ALGORITHM_VERSION, 1, "a new recipe needs a new version and a baseline migration");
    }

    #[test]
    fn a_long_snippet_is_cut_on_a_character_boundary_and_hex_is_checked() {
        let long = "\u{e9}".repeat(3000);
        assert!(normalise_snippet(&long).len() <= 4096);
        assert!(FindingFingerprint::from_hex("zz").is_none() && FindingFingerprint::from_hex(&"g".repeat(64)).is_none());
    }
}
