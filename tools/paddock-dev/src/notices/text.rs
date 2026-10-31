//! Text rules the notices file depends on, kept equal to JavaScript's so the output is byte-identical to the generator this
//! module ports (`conformance/src/notices`): its whitespace set, its UTF-16 string order and its UTF-16 lengths.

use regex::Regex;
use std::cmp::Ordering;
use std::sync::LazyLock;

/// JavaScript's `\s` and `trim()` set (not Unicode's White_Space: it adds U+FEFF and lacks U+0085).
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\u{B}'
            | '\u{C}'
            | '\r'
            | ' '
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202F}'
            | '\u{205F}'
            | '\u{3000}'
            | '\u{FEFF}'
    ) || ('\u{2000}'..='\u{200A}').contains(&c)
}

/// `String.prototype.trim`.
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// Unifies line endings only (`\r\n` and a lone `\r` become `\n`), so a text embedded verbatim is the same on a CRLF checkout.
pub fn lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Line endings unified, trailing blanks per line removed, outer blank lines removed.
pub fn normalise(text: &str) -> String {
    let unified = lf(text);
    let joined = unified
        .split('\n')
        .map(|l| l.trim_end_matches(is_js_space))
        .collect::<Vec<_>>()
        .join("\n");
    js_trim(&joined).to_string()
}

/// The order of JavaScript's `<` on strings: by UTF-16 code unit (differs from bytes only for characters above U+FFFF).
pub fn cmp16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// JavaScript's `.length`.
pub fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

static SPDX_SPLIT: LazyLock<Regex> = LazyLock::new(|| {
    // The `\s` of JavaScript, written out: see `is_js_space`.
    let ws = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";
    Regex::new(&format!(r"(?i){ws}+(?:OR|AND){ws}+")).unwrap_or_else(|e| panic!("static pattern: {e}"))
});

/// Splits an SPDX expression into its identifiers (OR/AND operands; "WITH" stays attached).
pub fn spdx_ids(expr: &str) -> Vec<String> {
    let flat = expr.replace(['(', ')'], " ");
    SPDX_SPLIT
        .split(&flat)
        .map(|s| js_trim(s).to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_endings_and_blanks_are_unified_like_the_generator() {
        assert_eq!(lf("\r\n  a  \r\nb\rc\n\n"), "\n  a  \nb\nc\n\n");
        assert_eq!(normalise("a  \r\nb\r\n\r\n"), "a\nb");
        assert_eq!(normalise("\n\n  keep indent\t \n"), "keep indent");
        assert_eq!(normalise("x\u{FEFF}\n\u{A0}y"), "x\n\u{A0}y");
    }

    #[test]
    fn javascript_whitespace_is_used_not_unicode_whitespace() {
        assert!(is_js_space('\u{FEFF}') && is_js_space('\u{2003}'));
        assert!(!is_js_space('\u{85}') && !is_js_space('\u{200B}'));
        assert_eq!(js_trim("\u{FEFF} a \u{85}"), "a \u{85}");
    }

    #[test]
    fn string_order_is_utf16_not_utf8() {
        // U+FF5E (BMP) sorts after U+1D49C (astral, surrogate D835) in UTF-16 but before it in UTF-8.
        assert_eq!(cmp16("\u{FF5E}", "\u{1D49C}"), Ordering::Greater);
        assert_eq!("\u{FF5E}".cmp("\u{1D49C}"), Ordering::Less);
        assert_eq!(len16("a\u{1D49C}"), 3);
    }

    #[test]
    fn spdx_ids_split_on_and_or_only() {
        assert_eq!(
            spdx_ids("(MIT OR Apache-2.0) AND Unicode-3.0"),
            ["MIT", "Apache-2.0", "Unicode-3.0"]
        );
        assert_eq!(spdx_ids("Apache-2.0 WITH LLVM-exception"), ["Apache-2.0 WITH LLVM-exception"]);
        assert_eq!(spdx_ids("mit or isc"), ["mit", "isc"]);
        assert_eq!(spdx_ids(""), Vec::<String>::new());
    }
}
