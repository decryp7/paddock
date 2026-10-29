#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' tier-2 parser library (ADR 0003): tree-sitter turned into symbols, an outline, imports, call candidates and
//! chunks.
//!
//! **Not crash-isolated.** Several grammars have an external scanner in C; a crash there cannot be caught. Until the parser
//! worker of A3 runs this in its own process, nothing in the app calls it in process on a repository's files. The walk over
//! a tree iterates a `TreeCursor` with an explicit stack and never recurses, and is bounded by [`Limits`]; arbitrary bytes
//! never panic.

pub mod chunk;
pub mod extract;
pub mod lang;
pub mod model;
pub mod spec;

pub use lang::{Grammar, ParseLanguage, abi_range};
pub use model::{CallCandidate, Chunk, Import, Outline, ParsedSymbol};

use paddock_jobs::CancelToken;

/// The most bytes of a file that are parsed by default (2 MiB).
pub const DEFAULT_MAX_BYTES: usize = 2 * 1024 * 1024;
/// The most syntax nodes the walk visits by default.
pub const DEFAULT_MAX_NODES: usize = 500_000;

/// Per-file limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Files larger than this are not parsed ([`ParseError::TooLarge`]); the file stays at tier 1.
    pub max_bytes: usize,
    /// The walk stops after this many nodes and the outline is marked partial.
    pub max_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bytes: DEFAULT_MAX_BYTES,
            max_nodes: DEFAULT_MAX_NODES,
        }
    }
}

/// Why a file was not parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The file is larger than the byte limit.
    #[error("the file is {len} bytes; at most {max} are parsed")]
    TooLarge {
        /// Its size.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
    /// The grammar's ABI is outside the runtime's range, or the runtime refused it.
    #[error("the grammar for {language} cannot be loaded: {why}")]
    Grammar {
        /// The language.
        language: &'static str,
        /// Why.
        why: String,
    },
    /// The parser produced no tree.
    #[error("the parser produced no syntax tree")]
    NoTree,
}

/// Parses `bytes` as `language` and extracts the outline. Any bytes are accepted; invalid UTF-8 is read lossily for names.
pub fn parse(language: ParseLanguage, bytes: &[u8], limits: &Limits, cancel: &CancelToken) -> Result<Outline, ParseError> {
    extract::parse(language, bytes, limits, cancel)
}
