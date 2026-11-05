//! [`RepoPath`]: a workspace-relative path, parsed once at the edge.
//!
//! TODO(shared-later, ADR 0002 "Shared later"): other Paddock crates parse workspace-relative paths too; this type moves
//! into a shared crate once shared code unfreezes (after the Dyno and Spotter tags).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The longest path accepted, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// The longest single path segment accepted, in bytes.
pub const MAX_SEGMENT_BYTES: usize = 255;

/// Why a string is not a [`RepoPath`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// The path is empty.
    #[error("the path is empty")]
    Empty,
    /// The path is longer than [`MAX_PATH_BYTES`] or a segment longer than [`MAX_SEGMENT_BYTES`].
    #[error("the path is too long (at most {MAX_PATH_BYTES} bytes, {MAX_SEGMENT_BYTES} per segment)")]
    TooLong,
    /// The path is absolute or starts with a Windows drive.
    #[error("the path must be relative to the workspace, not absolute: {0:?}")]
    Absolute(String),
    /// The path has a `..` or `.` segment, or an empty segment (`a//b`).
    #[error("the path has an empty, `.` or `..` segment: {0:?}")]
    BadSegment(String),
    /// The path holds a NUL, a backslash or another control character.
    #[error("the path holds a control character or a backslash (use `/`): {0:?}")]
    BadCharacter(String),
}

/// A path relative to the workspace root, `/` separated, with no `.` or `..` segment, no control character and no drive
/// prefix. It cannot name a place outside the workspace by itself; resolving it against a real folder (and refusing
/// symlinks that leave it) is the index's job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RepoPath(String);

impl RepoPath {
    /// Parses `text`.
    pub fn parse(text: &str) -> Result<RepoPath, PathError> {
        if text.is_empty() {
            return Err(PathError::Empty);
        }
        if text.len() > MAX_PATH_BYTES {
            return Err(PathError::TooLong);
        }
        if text.chars().any(|c| c.is_control() || c == '\\') {
            return Err(PathError::BadCharacter(text.escape_debug().to_string()));
        }
        let bytes = text.as_bytes();
        let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        if text.starts_with('/') || drive {
            return Err(PathError::Absolute(text.to_owned()));
        }
        for segment in text.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(PathError::BadSegment(text.to_owned()));
            }
            if segment.len() > MAX_SEGMENT_BYTES {
                return Err(PathError::TooLong);
            }
        }
        Ok(RepoPath(text.to_owned()))
    }

    /// The path as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The last segment.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// The extension of the last segment, without the dot (`None` for `Makefile` and for `.gitignore`).
    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name();
        match name.rfind('.') {
            Some(0) | None => None,
            Some(i) => Some(&name[i + 1..]),
        }
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RepoPath {
    type Err = PathError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RepoPath::parse(s)
    }
}

impl Serialize for RepoPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RepoPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        RepoPath::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_paths_parse() {
        for p in ["a", "src/lib.rs", "a/b/c.d/e", ".github/workflows/ci.yml", "dir with space/f"] {
            assert_eq!(RepoPath::parse(p).unwrap().as_str(), p);
        }
    }

    #[test]
    fn bad_paths_are_refused_with_a_reason() {
        assert_eq!(RepoPath::parse(""), Err(PathError::Empty));
        for p in ["/etc/passwd", "C:/x", "c:\\x", "d:rel"] {
            assert!(
                matches!(RepoPath::parse(p), Err(PathError::Absolute(_) | PathError::BadCharacter(_))),
                "{p}"
            );
        }
        for p in ["../x", "a/../b", "a/./b", "a//b", "a/", "./a"] {
            assert!(matches!(RepoPath::parse(p), Err(PathError::BadSegment(_))), "{p}");
        }
        for p in ["a\\b", "a\0b", "a\nb"] {
            assert!(matches!(RepoPath::parse(p), Err(PathError::BadCharacter(_))), "{p:?}");
        }
        assert_eq!(RepoPath::parse(&"a".repeat(MAX_PATH_BYTES + 1)), Err(PathError::TooLong));
        assert_eq!(RepoPath::parse(&"a".repeat(MAX_SEGMENT_BYTES + 1)), Err(PathError::TooLong));
    }

    #[test]
    fn file_name_and_extension() {
        let p = RepoPath::parse("src/a.tar.gz").unwrap();
        assert_eq!((p.file_name(), p.extension()), ("a.tar.gz", Some("gz")));
        assert_eq!(RepoPath::parse("Makefile").unwrap().extension(), None);
        assert_eq!(RepoPath::parse("d/.gitignore").unwrap().extension(), None);
    }

    #[test]
    fn serde_goes_through_the_parser() {
        let p: RepoPath = serde_json::from_str("\"a/b\"").unwrap();
        assert_eq!(serde_json::to_string(&p).unwrap(), "\"a/b\"");
        assert!(serde_json::from_str::<RepoPath>("\"../b\"").is_err());
    }

    #[test]
    fn parsing_never_panics_on_hostile_input() {
        paddock_testkit::assert_never_panics(&[b"src/lib.rs", b"../../x"], |b| {
            RepoPath::parse(&String::from_utf8_lossy(b)).is_ok()
        });
    }

    #[test]
    fn display_and_from_str() {
        let p: RepoPath = "a/b".parse().unwrap();
        assert_eq!(p.to_string(), "a/b");
        assert!("../b".parse::<RepoPath>().is_err());
    }
}
