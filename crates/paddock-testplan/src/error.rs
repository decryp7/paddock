//! The one error type of the crate. Every variant carries what the user has to fix.

use std::fmt;

/// Why a plan or a result was refused. Used for both formats: a result is refused for the same kinds of reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The bytes are not valid TOML for the schema (`line` and `col` start at 1).
    Syntax {
        /// Line of the problem.
        line: usize,
        /// Column of the problem.
        col: usize,
        /// What is wrong.
        msg: String,
    },
    /// A key the schema does not know, at the same major version.
    UnknownKey {
        /// The key.
        path: String,
        /// Line of the key.
        line: usize,
    },
    /// The file's major version is outside what this reader accepts.
    Version {
        /// Version in the file.
        found: u32,
        /// Oldest major this reader accepts.
        min: u32,
        /// Newest major this reader accepts.
        max: u32,
    },
    /// `format` is missing or names the other kind of file.
    WrongFormat {
        /// What the reader wanted.
        expected: &'static str,
        /// What the file says (empty when missing).
        found: String,
    },
    /// A size or count limit.
    Limit {
        /// What was too big.
        what: String,
        /// The limit.
        max: usize,
    },
    /// An id that breaks the charset or length rule.
    BadId {
        /// Which kind of id.
        kind: &'static str,
        /// The offending text (cut to 80 characters).
        value: String,
    },
    /// A secret reference or a secret-looking value that is wrong.
    BadSecret {
        /// The offending text (cut to 80 characters).
        value: String,
        /// What is wrong with it.
        msg: String,
    },
    /// A `{{...}}` placeholder that is not `{{data.COLUMN}}` or `{{secret:NAME}}`, or a malformed value.
    BadValue {
        /// The offending text (cut to 80 characters).
        value: String,
    },
    /// A step that lacks a field its action needs, or has one it must not have.
    BadStep {
        /// The case id.
        case: String,
        /// Zero-based step index.
        index: usize,
        /// What is wrong.
        msg: String,
    },
    /// A field with a value outside its rules (timestamps, paths, hashes, enums).
    BadField {
        /// The field.
        field: String,
        /// What is wrong.
        msg: String,
    },
    /// A reference to something the file does not declare.
    DanglingRef {
        /// What kind of thing.
        kind: &'static str,
        /// The id that has no target.
        id: String,
    },
    /// Two entries with the same id.
    Duplicate {
        /// What kind of thing.
        kind: &'static str,
        /// The repeated id.
        id: String,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { line, col, msg } => write!(
                f,
                "line {line}, column {col}: {msg}. Fix the TOML at that position."
            ),
            Self::UnknownKey { path, line } => {
                write!(
                    f,
                    "line {line}: unknown key `{path}`. Remove it or fix its spelling; a file never carries keys the schema lacks."
                )
            }
            Self::Version { found, min, max } if found > max => write!(
                f,
                "this file has format version {found}; this reader accepts {min} to {max}. This file needs Sighter/Pacenotes {found} or newer: update the app."
            ),
            Self::Version { found, min, max } => {
                write!(
                    f,
                    "this file has format version {found}, older than the {min} to {max} this reader accepts. Re-export it with a current Sighter/Pacenotes."
                )
            }
            Self::WrongFormat { expected, found } if found.is_empty() => {
                write!(
                    f,
                    "`format` is missing. Add `format = \"{expected}\"` at the top of the file."
                )
            }
            Self::WrongFormat { expected, found } => write!(
                f,
                "`format` is `{found}` but a `{expected}` file is expected here. Open the right kind of file."
            ),
            Self::Limit { what, max } => write!(
                f,
                "{what} exceeds the limit of {max}. Split the file or shorten the text."
            ),
            Self::BadId { kind, value } => {
                write!(
                    f,
                    "`{value}` is not a valid {kind}: use 1 to 64 characters from A-Z a-z 0-9 . _ : / # -."
                )
            }
            Self::BadSecret { value, msg } => write!(
                f,
                "secret `{value}`: {msg}. Secrets are written as {{{{secret:NAME}}}} with NAME in A-Z 0-9 _ (1 to 64)."
            ),
            Self::BadValue { value } => write!(
                f,
                "`{value}` is not a valid value: use plain text, {{{{data.COLUMN}}}} or {{{{secret:NAME}}}}; plain text may not contain `{{{{`."
            ),
            Self::BadStep { case, index, msg } => {
                write!(f, "case `{case}`, step {}: {msg}.", index + 1)
            }
            Self::BadField { field, msg } => write!(f, "`{field}`: {msg}."),
            Self::DanglingRef { kind, id } => write!(
                f,
                "`{id}` refers to a {kind} that the file does not declare. Declare it or fix the reference."
            ),
            Self::Duplicate { kind, id } => write!(
                f,
                "the {kind} `{id}` is declared twice. Rename or remove one."
            ),
        }
    }
}

impl std::error::Error for PlanError {}
