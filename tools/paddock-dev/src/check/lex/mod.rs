//! The tokenizers of the conformance checks, one per language (ER-22), on the real lexers and parsers instead of hand-written scanners:
//! Rust on rustc's own lexer (`ra-ap-rustc_lexer`, rust-analyzer's published copy), shell on tree-sitter-bash and YAML on tree-sitter-yaml.
//! Two readers sit on them: `mask_rust` (code with comments and literal insides blanked: C30, C39, C53, C120) and the comment view
//! (only the comments kept: the waiver reader). A hand-written scanner missed nested block comments, `cr"..."` strings, here-documents,
//! `$'...'` and plain-scalar apostrophes (WP-10 batch 2 review), each one a marker read from where it is not a comment.
//! The JavaScript host's hand lexers (`conformance/src/util/rust-lexer.mjs`, `shell-lexer.mjs`, `yaml-lexer.mjs`) are held to these by
//! the parity tests (`tests/it/check_lex.rs`): this module is the oracle.

pub mod rust;
pub mod shell;
pub mod yaml;

/// The source of the comments of a file in `kind`'s language: `text` with everything but its comments blanked (newlines kept; one
/// blank per UTF-16 unit, so lines and columns are the ones JavaScript sees).
pub fn comments_only(text: &str, kind: Kind) -> String {
    let ranges = match kind {
        Kind::Rust => rust::comment_ranges(text),
        Kind::Shell => shell::comment_ranges(text),
        Kind::Yaml => yaml::comment_ranges(text),
    };
    keep_ranges(text, &ranges)
}

/// What a file's comments are written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Rust: `//` and nested block comments.
    Rust,
    /// Shell: a `#` that starts a word, never inside a quote, a here-document or `${...}`.
    Shell,
    /// YAML: a `#` after white space (or at the start of a line) outside a scalar.
    Yaml,
}

/// `text` with the bytes outside `ranges` (sorted, byte offsets) blanked: one space per UTF-16 unit, newlines kept.
pub fn keep_ranges(text: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut r = 0;
    for (i, c) in text.char_indices() {
        while r < ranges.len() && ranges[r].1 <= i {
            r += 1;
        }
        let inside = r < ranges.len() && ranges[r].0 <= i;
        if inside || c == '\n' {
            out.push(c);
        } else {
            out.extend(std::iter::repeat_n(' ', c.len_utf16()));
        }
    }
    out
}

/// `text` with the byte ranges in `blank` (sorted, disjoint) blanked: one space per UTF-16 unit, newlines kept.
pub fn blank_ranges(text: &str, blank: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut r = 0;
    for (i, c) in text.char_indices() {
        while r < blank.len() && blank[r].1 <= i {
            r += 1;
        }
        let inside = r < blank.len() && blank[r].0 <= i;
        if inside && c != '\n' {
            out.extend(std::iter::repeat_n(' ', c.len_utf16()));
        } else {
            out.push(c);
        }
    }
    out
}
