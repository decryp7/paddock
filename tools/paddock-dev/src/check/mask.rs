//! `mask_rust`: Rust source with its comments and the insides of its string, raw-string and char literals blanked (newlines kept),
//! the port of `maskRust` in `conformance/src/checks/C39-freed-port.mjs`. It works on UTF-16 code units, as the JavaScript does,
//! so the masked text is the same text the original gives, including for characters outside the basic plane (two blanks).
//! Faithful on purpose, quirks included (an unterminated string still gets its closing quote): the golden test compares the
//! output with the JavaScript's on every tracked `.rs` file, and C39 and C53 read positions off it.

const NL: u16 = b'\n' as u16;
const QUOTE: u16 = b'"' as u16;
const SQ: u16 = b'\'' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const STAR: u16 = b'*' as u16;
const HASH: u16 = b'#' as u16;

/// JavaScript's `slice(a, b)` for the non-negative cases used here: clamped, empty when `b <= a`.
fn slice(t: &[u16], a: usize, b: usize) -> &[u16] {
    let a = a.min(t.len());
    let b = b.min(t.len());
    if b <= a { &[] } else { &t[a..b] }
}

fn blank(out: &mut Vec<u16>, s: &[u16]) {
    out.extend(s.iter().map(|&u| if u == NL { NL } else { b' ' as u16 }));
}

fn index_of(t: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if from > t.len() || needle.is_empty() {
        return None;
    }
    t[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn is_word(u: u16) -> bool {
    u < 128 && ((u as u8).is_ascii_alphanumeric() || u == b'_' as u16)
}

/// `.` of a JavaScript regex: not a line terminator.
fn not_terminator(u: u16) -> bool {
    !matches!(u, 0x0a | 0x0d | 0x2028 | 0x2029)
}

/// The start of a raw string at `t[i..]`: `b?r#*"` read from the 12 units JavaScript looked at. Returns the length of the opener and its hash count.
fn raw_opener(t: &[u16], i: usize) -> Option<(usize, usize)> {
    let w = slice(t, i, i + 12);
    let mut k = 0;
    if w.first() == Some(&(b'b' as u16)) {
        k = 1;
    }
    if w.get(k) != Some(&(b'r' as u16)) {
        return None;
    }
    k += 1;
    let hashes = w[k..].iter().take_while(|&&u| u == HASH).count();
    k += hashes;
    (w.get(k) == Some(&QUOTE)).then_some((k + 1, hashes))
}

/// The length of the char literal at `t[i..]` (`'x'`, `'\n'`, `'\u{1F600}'`), looking at 12 units like the JavaScript.
fn char_literal(t: &[u16], i: usize) -> Option<usize> {
    let w = slice(t, i, i + 12);
    let at = |k: usize| w.get(k).copied();
    if at(1) == Some(BACKSLASH) {
        if at(2) == Some(b'u' as u16) && at(3) == Some(b'{' as u16) {
            let hex = w[4.min(w.len())..]
                .iter()
                .take_while(|&&u| u < 128 && (u as u8).is_ascii_hexdigit())
                .count();
            if hex > 0 && at(4 + hex) == Some(b'}' as u16) && at(5 + hex) == Some(SQ) {
                return Some(6 + hex);
            }
        }
        if at(2).is_some_and(not_terminator) && at(3) == Some(SQ) {
            return Some(4);
        }
        return None;
    }
    let c = at(1)?;
    (c != SQ && c != NL && at(2) == Some(SQ)).then_some(3)
}

/// The masked text as UTF-16 units.
pub fn mask_units(t: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        let prev = if i == 0 { b' ' as u16 } else { t[i - 1] };
        if let (Some((open, hashes)), false) = (raw_opener(t, i), is_word(prev)) {
            let mut close = vec![QUOTE];
            close.extend(std::iter::repeat_n(HASH, hashes));
            let stop = match index_of(t, &close, i + open) {
                Some(end) => end + close.len(),
                None => t.len(),
            };
            out.extend_from_slice(&t[i..i + open]);
            blank(&mut out, slice(t, i + open, stop.saturating_sub(close.len())));
            out.extend_from_slice(&close);
            i = stop;
        } else if t[i] == SLASH && t.get(i + 1) == Some(&SLASH) {
            let stop = t[i..].iter().position(|&u| u == NL).map_or(t.len(), |p| p + i);
            out.extend(std::iter::repeat_n(b' ' as u16, stop - i));
            i = stop;
        } else if t[i] == SLASH && t.get(i + 1) == Some(&STAR) {
            let stop = index_of(t, &[STAR, SLASH], i + 2).map_or(t.len(), |e| e + 2);
            blank(&mut out, slice(t, i, stop));
            i = stop;
        } else if t[i] == QUOTE {
            let mut j = i + 1;
            while j < t.len() && t[j] != QUOTE {
                j += if t[j] == BACKSLASH { 2 } else { 1 };
            }
            out.push(QUOTE);
            blank(&mut out, slice(t, i + 1, j));
            out.push(QUOTE);
            i = j + 1;
        } else if t[i] == SQ {
            if let Some(len) = char_literal(t, i) {
                out.push(SQ);
                out.extend(std::iter::repeat_n(b' ' as u16, len - 2));
                out.push(SQ);
                i += len;
            } else {
                out.push(SQ);
                i += 1;
            }
        } else {
            out.push(t[i]);
            i += 1;
        }
    }
    out
}

/// The complement of [`mask_units`]: only the comments are kept; code, strings, raw strings and char literals are blanked (newlines
/// kept, same length). The port of `rustComments`: the waiver reader looks at this text only, so a marker inside a literal is data.
pub fn comment_units(t: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(t.len());
    let mut i = 0;
    while i < t.len() {
        let prev = if i == 0 { b' ' as u16 } else { t[i - 1] };
        if let (Some((open, hashes)), false) = (raw_opener(t, i), is_word(prev)) {
            let mut close = vec![QUOTE];
            close.extend(std::iter::repeat_n(HASH, hashes));
            let stop = index_of(t, &close, i + open).map_or(t.len(), |end| end + close.len());
            blank(&mut out, slice(t, i, stop));
            i = stop;
        } else if t[i] == SLASH && t.get(i + 1) == Some(&SLASH) {
            let stop = t[i..].iter().position(|&u| u == NL).map_or(t.len(), |p| p + i);
            out.extend_from_slice(&t[i..stop]);
            i = stop;
        } else if t[i] == SLASH && t.get(i + 1) == Some(&STAR) {
            let stop = index_of(t, &[STAR, SLASH], i + 2).map_or(t.len(), |e| e + 2);
            out.extend_from_slice(&t[i..stop]);
            i = stop;
        } else if t[i] == QUOTE {
            let mut j = i + 1;
            while j < t.len() && t[j] != QUOTE {
                j += if t[j] == BACKSLASH { 2 } else { 1 };
            }
            blank(&mut out, slice(t, i, j + 1));
            i = j + 1;
        } else if t[i] == SQ {
            let len = char_literal(t, i);
            blank(&mut out, slice(t, i, i + len.unwrap_or(1)));
            i += len.unwrap_or(1);
        } else {
            out.push(if t[i] == NL { NL } else { b' ' as u16 });
            i += 1;
        }
    }
    out
}

/// `text` with comments and the insides of string, raw-string and char literals blanked (newlines kept). Unlike a plain
/// string scan it knows raw strings (`r#"..."#`) and char literals (`'"'`), which would otherwise open a string that never ends.
pub fn mask_rust(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&mask_units(&units))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_literals_and_raw_strings_do_not_open_strings_that_never_end() {
        let m = mask_rust("let q = '\"'; let r = r#\"a\"b\"#; let c = '\\n'; fn f<'a>(x: &'a str) {}\nTcpListener::bind(x)");
        assert!(m.contains("TcpListener::bind(x)"));
        assert!(m.contains("<'a>"));
        assert_eq!(m.split('\n').count(), 2);
    }

    #[test]
    fn comments_and_string_insides_are_blank_and_newlines_stay() {
        assert_eq!(mask_rust("a // x\nb"), "a     \nb");
        assert_eq!(mask_rust("a /* x\ny */ b"), "a     \n     b");
        assert_eq!(mask_rust("s = \"ab\\\"c\";"), "s = \"     \";");
        assert_eq!(mask_rust("r#\"x\"#"), "r#\" \"#");
    }

    #[test]
    fn an_emoji_is_two_blanks_as_in_javascript() {
        assert_eq!(mask_rust("\"😀\""), "\"  \"");
        assert_eq!(
            mask_rust("'😀'"),
            "'😀'",
            "a char literal holding a surrogate pair is not a literal for the JavaScript either"
        );
    }

    #[test]
    fn unterminated_forms_end_the_way_the_javascript_does() {
        assert_eq!(mask_rust("\"abc"), "\"   \"");
        assert_eq!(mask_rust("/* abc"), "      ");
        assert_eq!(mask_rust("x // abc"), "x       ");
    }
}
