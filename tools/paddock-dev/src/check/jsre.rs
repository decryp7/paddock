//! JavaScript-compatible regular expressions and string helpers, so a port of a JS check matches exactly what the original did
//! (the oracle test compares the two). The `regex` crate is Unicode-aware where JavaScript's non-`u` regexes are not:
//! `\s` here is JavaScript's `WhiteSpace` and `LineTerminator` set, `\w` and `\b` are ASCII, and `.` is anything but a line
//! terminator. [`rx`] rewrites a pattern written in JavaScript's terms.

use regex::Regex;

/// The characters JavaScript's `\s` matches, as the inside of a character class.
pub const WS: &str = "\\t\\n\\x0B\\x0C\\r \\u{a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}";

/// Compiles `pattern` with `\s`, `\S`, `\w` and `\b` meaning JavaScript's, and `\.`-free `.` written as [`DOT`] by the caller.
/// Use them outside character classes only. A pattern that does not compile is a bug of the caller: it panics on first use.
#[allow(clippy::expect_used)]
pub fn rx(pattern: &str) -> Regex {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push_str(&format!("[{WS}]")),
            Some('S') => out.push_str(&format!("[^{WS}]")),
            Some('w') => out.push_str("[0-9A-Za-z_]"),
            Some('b') => out.push_str("(?-u:\\b)"),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Regex::new(&out).unwrap_or_else(|e| panic!("the pattern {pattern:?} (as {out:?}) does not compile: {e}"))
}

/// JavaScript's `.`: any character but a line terminator (inside a pattern given to [`rx`]).
pub const DOT: &str = "[^\\n\\r\\u{2028}\\u{2029}]";

/// Whether `c` is JavaScript white space or a line terminator.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// JavaScript's `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// The number of UTF-16 code units in `s` (JavaScript's `.length`).
pub fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The 1-based line of byte offset `at` in `text`: the newlines before it plus one.
pub fn line_at(text: &str, at: usize) -> usize {
    text.as_bytes()[..at.min(text.len())].iter().filter(|b| **b == b'\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_and_w_are_javascripts() {
        assert!(rx(r"^\s+$").is_match("\u{feff}\u{a0}"));
        assert!(
            !rx(r"^\s+$").is_match("\u{85}"),
            "NEL is white space for Unicode but not for JavaScript"
        );
        assert!(!rx(r"^\w+$").is_match("é"));
        assert!(rx(r"\bab\b").is_match("x ab y"));
        assert!(
            rx(r"\bab\b").is_match("éab"),
            "an ASCII word boundary sees é as a non-word character"
        );
    }

    #[test]
    fn trim_matches_javascript() {
        assert_eq!(trim("\u{feff} a \u{2028}"), "a");
        assert_eq!(trim("\u{85}a"), "\u{85}a");
    }

    #[test]
    fn units_count_surrogates_twice() {
        assert_eq!(units("a😀"), 3);
    }
}
