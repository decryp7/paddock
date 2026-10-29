//! The facts of one file, and the port that produces them.

use pacenotes_core::{Language, RepoPath, Span, SymbolKind};
use paddock_jobs::CancelToken;

/// A declared symbol, as the source reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolFact {
    /// What it is.
    pub kind: SymbolKind,
    /// Its own name.
    pub name: String,
    /// The container chain inside the file plus the name (ADR 0002); never a path-derived module path.
    pub qualified_name: String,
    /// The whole declaration.
    pub span: Span,
    /// How many containers enclose it.
    pub depth: u32,
}

/// An import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFact {
    /// What is imported, as written.
    pub target: String,
    /// Where.
    pub span: Span,
}

/// A call known only by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallFact {
    /// The qualified name of the enclosing callable.
    pub caller: Option<String>,
    /// The called name.
    pub callee: String,
    /// Where.
    pub span: Span,
}

/// A piece of the file for search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkFact {
    /// Its lines.
    pub span: Span,
    /// The qualified name of the symbol it holds.
    pub symbol: Option<String>,
    /// Its place among the chunks of the symbol.
    pub ordinal: u32,
}

/// Everything extracted from one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    /// The language parsed.
    pub language: Language,
    /// Symbols in source order.
    pub symbols: Vec<SymbolFact>,
    /// Imports.
    pub imports: Vec<ImportFact>,
    /// Calls.
    pub calls: Vec<CallFact>,
    /// Chunks.
    pub chunks: Vec<ChunkFact>,
    /// The extractor stopped at its node limit.
    pub partial: bool,
    /// The file has syntax errors.
    pub has_errors: bool,
}

/// Why a file's facts are missing. One bad file is recorded in the report; it never fails a run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// The extractor failed on this file (a crashed worker is attributed to the file).
    #[error("the extractor failed: {0}")]
    Failed(String),
    /// The worker died or was judged stuck on this file (ADR 0006): the file is quarantined until its content changes, and the
    /// run goes on.
    #[error("the worker crashed or hung on this file: {0}")]
    Crashed(String),
    /// Cancelled by the caller.
    #[error("cancelled")]
    Cancelled,
}

/// Produces the facts of one file. In the app this is the parser worker (A3), a separate process; the index never parses.
pub trait SymbolSource: Send + Sync {
    /// The facts of `bytes` as the file at `path`, or `None` when the file is not a language the source knows (it stays at
    /// tier 1: listed, not parsed).
    fn extract(&self, path: &RepoPath, bytes: &[u8], cancel: &CancelToken) -> Result<Option<FileFacts>, SourceError>;
}
