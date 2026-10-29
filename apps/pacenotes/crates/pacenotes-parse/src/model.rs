//! What a parse produces.

use pacenotes_core::{Span, SymbolKind};

use crate::lang::ParseLanguage;

/// A declared symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSymbol {
    /// What it is.
    pub kind: SymbolKind,
    /// Its own name.
    pub name: String,
    /// The chain of containers inside the file plus its name, joined by the language's separator (ADR 0002): never a module
    /// path derived from the file's location.
    pub qualified_name: String,
    /// The whole declaration.
    pub span: Span,
    /// How many containers enclose it.
    pub depth: u32,
}

/// An import, include or use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// What is imported, as written (a path, a dotted name), without the keyword and the quotes.
    pub target: String,
    /// Where.
    pub span: Span,
}

/// A call whose target is only known by name (tier 2 cannot resolve it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCandidate {
    /// The last name of the callee (`parse` for `a.b.parse(x)`).
    pub callee: String,
    /// The qualified name of the enclosing function or method, if any.
    pub caller: Option<String>,
    /// Where.
    pub span: Span,
}

/// A piece of the file for search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Its lines.
    pub span: Span,
    /// The qualified name of the symbol it holds, if it holds one.
    pub symbol: Option<String>,
    /// Its place among the chunks of the symbol (or of the file): 0 for the first piece.
    pub ordinal: u32,
}

/// The result of parsing one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    /// The language parsed.
    pub language: ParseLanguage,
    /// Symbols in source order.
    pub symbols: Vec<ParsedSymbol>,
    /// Imports in source order.
    pub imports: Vec<Import>,
    /// Call candidates in source order.
    pub calls: Vec<CallCandidate>,
    /// Chunks in source order.
    pub chunks: Vec<Chunk>,
    /// True when the node limit stopped the walk: the lists are a prefix of the file.
    pub partial: bool,
    /// True when the tree contains syntax errors (the lists are still best effort).
    pub has_errors: bool,
}
