//! Regular expressions written by users (profiles, searches, filters, packs), compiled with limits: the pattern's length
//! and the compiled program's size are capped, so a pattern can neither exhaust memory nor take long to build. The
//! `regex` crate matches in linear time, so matching cannot blow up. The merge of Debrief's and Spotter's copies: Debrief's
//! caps and `compile_set`, Spotter's messages that say what to fix, typed errors for both.

use regex::{RegexBuilder, RegexSetBuilder};
use std::fmt;

/// A compiled pattern (the `regex` crate's).
pub use regex::Regex;
/// A compiled set of patterns (the `regex` crate's).
pub use regex::RegexSet;

/// The caps on a user pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegexLimits {
    /// Longest pattern, in bytes.
    pub max_pattern_len: usize,
    /// Largest compiled program and lazy-DFA cache, in bytes.
    pub size_limit: usize,
}

impl RegexLimits {
    /// Debrief's caps: 4 KiB patterns, 8 MiB compiled (log profiles with large Unicode classes).
    pub const DEFAULT: Self = Self {
        max_pattern_len: 4096,
        size_limit: 8 << 20,
    };
    /// Spotter's caps: 4 KiB patterns, 1 MiB compiled (names typed in a form).
    pub const STRICT: Self = Self {
        max_pattern_len: 4096,
        size_limit: 1 << 20,
    };
}

impl Default for RegexLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a user pattern was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegexError {
    /// The pattern is longer than the cap.
    TooLong {
        /// Its length in bytes.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// The compiled program would exceed the size cap.
    TooComplex,
    /// The pattern is not a valid regular expression; the text is the parser's explanation.
    Invalid(String),
}

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len, max } => {
                write!(f, "The pattern is too long ({len} bytes; at most {max}).")
            }
            Self::TooComplex => f.write_str("The pattern is too complex; simplify it."),
            Self::Invalid(m) => write!(f, "The pattern is not a valid regular expression: {m}"),
        }
    }
}

impl std::error::Error for RegexError {}

fn map(e: regex::Error) -> RegexError {
    match e {
        regex::Error::CompiledTooBig(_) => RegexError::TooComplex,
        other => {
            let text = other.to_string();
            let line = text
                .lines()
                .rev()
                .find(|l| l.starts_with("error:"))
                .unwrap_or(&text);
            RegexError::Invalid(line.trim().trim_start_matches("error:").trim().to_string())
        }
    }
}

/// Compiles a user pattern with [`RegexLimits::DEFAULT`]; `case_insensitive` for file and tag names.
pub fn compile(pattern: &str, case_insensitive: bool) -> Result<Regex, RegexError> {
    compile_with(pattern, case_insensitive, &RegexLimits::DEFAULT)
}

/// Compiles a user pattern under explicit caps.
pub fn compile_with(
    pattern: &str,
    case_insensitive: bool,
    limits: &RegexLimits,
) -> Result<Regex, RegexError> {
    if pattern.len() > limits.max_pattern_len {
        return Err(RegexError::TooLong {
            len: pattern.len(),
            max: limits.max_pattern_len,
        });
    }
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .size_limit(limits.size_limit)
        .dfa_size_limit(limits.size_limit)
        .build()
        .map_err(map)
}

/// Compiles several patterns into one set under the same caps; each pattern is checked for length first.
pub fn compile_set(patterns: &[String], limits: &RegexLimits) -> Result<RegexSet, RegexError> {
    if let Some(p) = patterns.iter().find(|p| p.len() > limits.max_pattern_len) {
        return Err(RegexError::TooLong {
            len: p.len(),
            max: limits.max_pattern_len,
        });
    }
    RegexSetBuilder::new(patterns)
        .size_limit(limits.size_limit)
        .dfa_size_limit(limits.size_limit)
        .build()
        .map_err(map)
}
