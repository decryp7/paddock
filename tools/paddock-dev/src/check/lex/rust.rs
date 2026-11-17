//! Rust on rustc's own lexer (`ra-ap-rustc_lexer`): comments (nested block comments included), strings, byte and C strings, raw
//! strings with any prefix (`r`, `br`, `cr`), char and byte literals told from lifetimes, raw identifiers.

use super::{blank_ranges, keep_ranges};
use ra_ap_rustc_lexer::{FrontmatterAllowed, LiteralKind, TokenKind, strip_shebang, tokenize};

/// One token of a Rust source, by byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tok {
    /// First byte.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
    /// What it is.
    pub kind: TokKind,
}

/// The kinds a check cares about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    /// A line or block comment.
    Comment,
    /// A string, byte string, C string, raw string or char literal: the bytes between `open` and `close` (relative to `start`; `close`
    /// is the token's end when the literal is not terminated) are its inside.
    Literal {
        /// Where the inside starts.
        open: usize,
        /// Where the inside ends (where the closing delimiter starts).
        close: usize,
    },
    /// Anything else (code, identifiers, an invalid or unterminated char literal, a shebang).
    Other,
}

/// The tokens of `text` (frontmatter off, a shebang line one `Other` token).
pub fn tokens(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut pos = 0;
    if let Some(n) = strip_shebang(text) {
        out.push(Tok {
            start: 0,
            end: n,
            kind: TokKind::Other,
        });
        pos = n;
    }
    for t in tokenize(&text[pos..], FrontmatterAllowed::No) {
        let (start, end) = (pos, pos + t.len as usize);
        let src = &text[start..end];
        let kind = match t.kind {
            TokenKind::LineComment { .. } | TokenKind::BlockComment { .. } => TokKind::Comment,
            TokenKind::Literal { kind, suffix_start } => literal(src, kind, suffix_start as usize),
            _ => TokKind::Other,
        };
        out.push(Tok { start, end, kind });
        pos = end;
    }
    out
}

/// The inside of a literal token: after its opener (prefix, hashes and the quote), before its closer (the quote and hashes). `suffix_start`
/// is where the lexer says the literal ends and its suffix (`u8`) begins.
fn literal(src: &str, kind: LiteralKind, suffix_start: usize) -> TokKind {
    let quote = |c: char| src.find(c);
    match kind {
        LiteralKind::Int { .. } | LiteralKind::Float { .. } => TokKind::Other,
        LiteralKind::Str { terminated } | LiteralKind::ByteStr { terminated } | LiteralKind::CStr { terminated } => {
            let Some(q) = quote('"') else { return TokKind::Other };
            TokKind::Literal {
                open: q + 1,
                close: if terminated { suffix_start - 1 } else { src.len() },
            }
        }
        LiteralKind::Char { terminated } | LiteralKind::Byte { terminated } => {
            if !terminated {
                return TokKind::Other;
            }
            let Some(q) = quote('\'') else { return TokKind::Other };
            TokKind::Literal {
                open: q + 1,
                close: suffix_start - 1,
            }
        }
        LiteralKind::RawStr { n_hashes } | LiteralKind::RawByteStr { n_hashes } | LiteralKind::RawCStr { n_hashes } => {
            let after = src.trim_start_matches(['b', 'c', 'r']);
            let hashes = after.bytes().take_while(|&b| b == b'#').count();
            let open = src.len() - after.len() + hashes + 1;
            match n_hashes {
                Some(n) => TokKind::Literal {
                    open,
                    close: suffix_start - 1 - usize::from(n),
                },
                // An invalid starter (`r#x`) is no literal; a missing terminator is a literal that runs to the end.
                None if after[hashes..].starts_with('"') => TokKind::Literal { open, close: src.len() },
                None => TokKind::Other,
            }
        }
    }
}

/// The byte ranges of the comments.
pub fn comment_ranges(text: &str) -> Vec<(usize, usize)> {
    tokens(text)
        .iter()
        .filter(|t| t.kind == TokKind::Comment)
        .map(|t| (t.start, t.end))
        .collect()
}

/// `text` with only its comments kept.
pub fn comments(text: &str) -> String {
    keep_ranges(text, &comment_ranges(text))
}

/// `text` with its comments and the insides of its literals blanked (newlines kept; one blank per UTF-16 unit).
pub fn mask(text: &str) -> String {
    let mut blank: Vec<(usize, usize)> = Vec::new();
    for t in tokens(text) {
        match t.kind {
            TokKind::Comment => blank.push((t.start, t.end)),
            TokKind::Literal { open, close } if close > open => blank.push((t.start + open, t.start + close)),
            _ => {}
        }
    }
    blank_ranges(text, &blank)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, bool)> {
        tokens(text)
            .iter()
            .filter(|t| t.kind != TokKind::Other)
            .map(|t| (&text[t.start..t.end], t.kind == TokKind::Comment))
            .collect()
    }

    #[test]
    fn nested_block_comments_and_c_raw_strings_are_one_token() {
        assert_eq!(kinds("/* a /* b */ c */ x"), [("/* a /* b */ c */", true)]);
        assert_eq!(kinds("let s = cr\"C:\\\"; // c"), [("cr\"C:\\\"", false), ("// c", true)]);
        assert_eq!(kinds("let s = br##\"a\"#b\"##;"), [("br##\"a\"#b\"##", false)]);
    }

    #[test]
    fn lifetimes_are_not_char_literals_and_a_char_may_hold_a_quote() {
        assert_eq!(
            kinds("fn f<'a>(x: &'a str) -> char { '\"' } // c"),
            [("'\"'", false), ("// c", true)]
        );
        assert_eq!(kinds("let c = '😀'; /* x */"), [("'😀'", false), ("/* x */", true)]);
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end_and_the_mask_keeps_its_opener() {
        assert_eq!(mask("a \"bc\nd"), "a \"  \n ");
        assert_eq!(mask("x // abc"), "x       ");
        assert_eq!(mask("r#\"x\"#"), "r#\" \"#");
        assert_eq!(mask("s = \"ab\\\"c\";"), "s = \"     \";");
        assert_eq!(mask("\"😀\""), "\"  \"");
    }
}
