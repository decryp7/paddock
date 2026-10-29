#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! Pacenotes' pure core (ADR 0002): the vocabulary every other crate shares, the ports the core owns and the validated
//! schemas of language-server packs and review rules.
//!
//! Nothing here does I/O, starts a process or reads a clock. Untrusted input is parsed once, into the types below
//! ([`RepoPath`], [`ServerPack`], [`RuleSpec`], ...), and everything after works with the types.

pub mod finding;
pub mod ids;
pub mod language;
pub mod location;
pub mod lsp;
pub mod pack;
pub mod path;
pub mod ports;
pub mod rev;
pub mod rule;
pub mod tier;

#[cfg(any(test, feature = "testing"))]
pub mod contract;

pub use finding::{FindingState, TransitionError};
pub use ids::{ChunkId, FileId, FileIdAllocator, Fingerprint, IdError, SymbolId, SymbolKind};
pub use language::{KnownLanguage, Language, LanguageError, LanguageId};
pub use location::{Location, Position, Span, SpanError};
pub use pack::{LspFeature, PackError, ServerPack};
pub use path::{PathError, RepoPath};
pub use rev::{CommitHash, Generation, Rev, RevError};
pub use rule::{Check, RuleError, RuleId, RuleSpec};
pub use tier::{Confidence, Severity, SeverityError, Tier};

#[cfg(test)]
mod wire_tests;
