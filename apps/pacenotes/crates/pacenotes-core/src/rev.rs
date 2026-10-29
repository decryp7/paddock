//! [`Rev`]: which state of the repository a fact was read from.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why a string is not a [`CommitHash`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RevError {
    /// A commit hash is 40 (SHA-1) or 64 (SHA-256) hexadecimal digits.
    #[error("a commit hash is 40 or 64 hexadecimal digits, got {0} characters")]
    BadLength(usize),
    /// A character is not a hexadecimal digit.
    #[error("a commit hash holds only 0-9 and a-f")]
    NotHex,
}

/// A full commit hash, lowercase hexadecimal (40 digits for SHA-1 repositories, 64 for SHA-256).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommitHash(String);

impl CommitHash {
    /// Parses a full hash; upper case is accepted and lowered.
    pub fn parse(text: &str) -> Result<CommitHash, RevError> {
        if text.len() != 40 && text.len() != 64 {
            return Err(RevError::BadLength(text.chars().count()));
        }
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(RevError::NotHex);
        }
        Ok(CommitHash(text.to_ascii_lowercase()))
    }

    /// The hash as lowercase hex.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for CommitHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CommitHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        CommitHash::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// The index generation counter: it grows by one each time the index is rewritten, so a cache keyed by it never serves
/// an older index.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Generation(pub u64);

impl Generation {
    /// The next generation.
    #[must_use]
    pub fn next(self) -> Generation {
        Generation(self.0.saturating_add(1))
    }
}

/// A state of the repository: a commit, or the working tree as the index saw it in one generation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Rev {
    /// A commit.
    Commit(CommitHash),
    /// The working tree at an index generation.
    WorkingTree(Generation),
}

impl Rev {
    /// True when this revision can change under the same name (the working tree does; a commit never does).
    pub fn is_mutable(&self) -> bool {
        matches!(self, Rev::WorkingTree(_))
    }
}

impl fmt::Display for Rev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rev::Commit(h) => write!(f, "{h}"),
            Rev::WorkingTree(g) => write!(f, "worktree@{}", g.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn commit_hashes_are_checked() {
        assert_eq!(CommitHash::parse(SHA1).unwrap().as_str(), SHA1);
        assert_eq!(CommitHash::parse(&SHA1.to_uppercase()).unwrap().as_str(), SHA1);
        assert!(CommitHash::parse(&SHA1.repeat(2)[..64]).is_ok());
        assert_eq!(CommitHash::parse("abc"), Err(RevError::BadLength(3)));
        assert_eq!(CommitHash::parse(&"g".repeat(40)), Err(RevError::NotHex));
    }

    #[test]
    fn generations_grow_and_saturate() {
        assert_eq!(Generation(1).next(), Generation(2));
        assert_eq!(Generation(u64::MAX).next(), Generation(u64::MAX));
    }

    #[test]
    fn revs_round_trip_and_know_if_they_move() {
        let c = Rev::Commit(CommitHash::parse(SHA1).unwrap());
        let w = Rev::WorkingTree(Generation(7));
        for r in [&c, &w] {
            let json = serde_json::to_string(r).unwrap();
            assert_eq!(&serde_json::from_str::<Rev>(&json).unwrap(), r);
        }
        assert!(!c.is_mutable() && w.is_mutable());
        assert_eq!(w.to_string(), "worktree@7");
        assert!(serde_json::from_str::<Rev>(r#"{"kind":"commit","value":"zz"}"#).is_err());
    }
}
