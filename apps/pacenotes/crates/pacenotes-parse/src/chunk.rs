//! Chunks for search: pure, from the symbols of a file.

use pacenotes_core::{Position, Span, SymbolKind};

use crate::model::{Chunk, ParsedSymbol};

/// The most lines in one chunk; a longer symbol is cut into pieces of this many lines.
pub const MAX_CHUNK_LINES: u32 = 120;
/// The window used for a file without symbols (a script, a config-like source).
pub const WINDOW_LINES: u32 = 60;

fn callable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor)
}

fn type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::Trait
    )
}

fn lines(first: u32, last: u32) -> Option<Span> {
    let start = Position::new(first, 1)?;
    let end = Position::new(last.saturating_add(1), 1)?;
    Span::new(start, end).ok()
}

/// The last line a span covers (an end at column 1 is the line before).
fn last_line(span: &Span) -> u32 {
    let end = span.end();
    if end.column() == 1 && end.line() > span.start().line() {
        end.line() - 1
    } else {
        end.line()
    }
}

fn pieces(out: &mut Vec<Chunk>, first: u32, last: u32, symbol: Option<&str>) {
    let mut ordinal = 0;
    let mut start = first;
    while start <= last {
        let end = last.min(start.saturating_add(MAX_CHUNK_LINES - 1));
        if let Some(span) = lines(start, end) {
            out.push(Chunk {
                span,
                symbol: symbol.map(str::to_owned),
                ordinal,
            });
        }
        ordinal += 1;
        start = end.saturating_add(1);
        if end == u32::MAX {
            break;
        }
    }
}

/// Chunks of a file with `total_lines` lines: every function, method and constructor, and every type that holds none of them
/// (so a class with methods is chunked by its methods, not twice). A symbol longer than [`MAX_CHUNK_LINES`] is cut into
/// pieces. A file with no such symbol is cut into windows of [`WINDOW_LINES`].
pub fn build(symbols: &[ParsedSymbol], total_lines: u32) -> Vec<Chunk> {
    let mut out = Vec::new();
    for (i, s) in symbols.iter().enumerate() {
        let wanted =
            callable(s.kind) || (type_like(s.kind) && !symbols[i + 1..].iter().take_while(|n| n.depth > s.depth).any(|n| callable(n.kind)));
        if wanted {
            pieces(&mut out, s.span.start().line(), last_line(&s.span), Some(&s.qualified_name));
        }
    }
    if out.is_empty() && total_lines > 0 {
        let mut start = 1;
        let mut ordinal = 0;
        while start <= total_lines {
            let end = total_lines.min(start + WINDOW_LINES - 1);
            if let Some(span) = lines(start, end) {
                out.push(Chunk {
                    span,
                    symbol: None,
                    ordinal,
                });
            }
            ordinal += 1;
            start = end + 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(kind: SymbolKind, name: &str, first: u32, last: u32, depth: u32) -> ParsedSymbol {
        ParsedSymbol {
            kind,
            name: name.into(),
            qualified_name: name.into(),
            span: lines(first, last).unwrap(),
            depth,
        }
    }

    #[test]
    fn functions_and_types_without_functions_are_chunked_once() {
        let symbols = [
            sym(SymbolKind::Class, "A", 1, 20, 0),
            sym(SymbolKind::Method, "A.f", 2, 9, 1),
            sym(SymbolKind::Method, "A.g", 10, 19, 1),
            sym(SymbolKind::Struct, "Plain", 22, 25, 0),
            sym(SymbolKind::Constant, "K", 27, 27, 0),
        ];
        let chunks = build(&symbols, 30);
        let names: Vec<_> = chunks.iter().map(|c| c.symbol.as_deref().unwrap()).collect();
        assert_eq!(names, ["A.f", "A.g", "Plain"]);
        assert_eq!((chunks[0].span.start().line(), last_line(&chunks[0].span)), (2, 9));
    }

    #[test]
    fn a_long_symbol_is_cut_into_pieces_and_a_file_without_symbols_into_windows() {
        let long = [sym(SymbolKind::Function, "big", 1, 300, 0)];
        let chunks = build(&long, 300);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks.iter().map(|c| c.ordinal).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(last_line(&chunks[2].span), 300);
        let windows = build(&[], 130);
        assert_eq!(windows.len(), 3);
        assert!(windows.iter().all(|c| c.symbol.is_none()));
        assert_eq!(last_line(&windows[2].span), 130);
        assert!(build(&[], 0).is_empty());
    }

    #[test]
    fn a_one_line_symbol_has_one_line() {
        let chunks = build(&[sym(SymbolKind::Function, "f", 4, 4, 0)], 10);
        assert_eq!((chunks[0].span.start().line(), last_line(&chunks[0].span)), (4, 4));
    }
}
