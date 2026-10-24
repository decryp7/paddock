use std::fmt;

/// Why a pack file was refused. Every message says what to fix and carries none of the file's content beyond the
/// parser's own location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// The file is larger than [`crate::Limits::max_file_bytes`].
    TooLarge {
        /// Bytes in the file (or "more than the limit" read).
        len: u64,
        /// The limit.
        max: u64,
    },
    /// The file is not UTF-8 text.
    NotText,
    /// The text is not valid TOML (the parser's message names the line and column).
    Syntax(String),
    /// The `format` key is missing, not a whole number, or not the one this build reads.
    Format(String),
    /// A limit on the content was exceeded (entries, string length, nesting, nodes); the text says which and where.
    Limit(String),
    /// The content does not have the shape the app declared (wrong type, missing key, unknown enum value).
    Shape(String),
    /// The app's own rules refused the pack ([`crate::PackKind::convert`]).
    Rejected(String),
    /// The file could not be read.
    Io(String),
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len, max } => write!(
                f,
                "the pack file is too large ({len} bytes; the limit is {max})"
            ),
            Self::NotText => f.write_str("the pack file is not UTF-8 text"),
            Self::Syntax(m) => write!(f, "not valid TOML: {m}"),
            Self::Format(m) | Self::Limit(m) | Self::Shape(m) | Self::Rejected(m) | Self::Io(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for PackError {}
