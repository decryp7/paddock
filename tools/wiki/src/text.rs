//! Pure text parsing of wiki pages: front matter, headings and anchors, links, bullets and the word windows of the
//! copied-rule check. No I/O; none of it panics on any input (all slicing is at character boundaries found by searching).

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;

/// A copied run of this many words is rule text.
pub const COPY_WORDS: usize = 8;

/// JavaScript's `\s` (what the Node lint used), which is not `char::is_whitespace`.
pub fn is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `str::trim` with [`is_ws`].
pub fn trim_ws(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// A front matter value: `key: text` or a list (`[a, b]` or an indented `- item` block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A plain value.
    Text(String),
    /// A list value.
    List(Vec<String>),
}

impl Value {
    /// The value as a script would coerce it to a string (a list joins with commas).
    pub fn as_text(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::List(l) => l.join(","),
        }
    }
}

/// A parsed page: the front matter, the text after it, and where that text starts (byte offset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<'a> {
    /// Front matter keys (`[a-z_]+`) with their values; a later duplicate wins.
    pub meta: HashMap<String, Value>,
    /// The text after the front matter (all of it without one).
    pub body: &'a str,
    /// Byte offset of `body` in the page text.
    pub body_start: usize,
}

/// Splits a page into front matter and body. Front matter is `---\n ... \n---` at the very start.
pub fn parse_page(text: &str) -> Page<'_> {
    let none = || Page {
        meta: HashMap::new(),
        body: text,
        body_start: 0,
    };
    let Some(rest) = text.strip_prefix("---\n") else {
        return none();
    };
    let Some(pos) = rest.find("\n---") else {
        return none();
    };
    let block = &rest[..pos];
    let mut end = 4 + pos + 4;
    if text[end..].starts_with('\n') {
        end += 1;
    }
    let lines: Vec<&str> = block.split('\n').collect();
    let mut meta = HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((key, rest)) = key_value(lines[i]) {
            if rest.starts_with('[') {
                let inner = rest.strip_prefix('[').unwrap_or(rest);
                let inner = inner.strip_suffix(']').unwrap_or(inner);
                let items = inner.split(',').map(|s| trim_ws(s).to_string()).filter(|s| !s.is_empty()).collect();
                meta.insert(key.to_string(), Value::List(items));
            } else if rest.is_empty() {
                let mut items = Vec::new();
                while i + 1 < lines.len() {
                    let Some(item) = list_item(lines[i + 1]) else { break };
                    items.push(item);
                    i += 1;
                }
                meta.insert(key.to_string(), Value::List(items));
            } else {
                meta.insert(key.to_string(), Value::Text(trim_ws(rest).to_string()));
            }
        }
        i += 1;
    }
    Page {
        meta,
        body: &text[end..],
        body_start: end,
    }
}

/// `^([a-z_]+):\s*(.*)$`
fn key_value(line: &str) -> Option<(&str, &str)> {
    let key_len = line.bytes().take_while(|b| b.is_ascii_lowercase() || *b == b'_').count();
    if key_len == 0 || line.as_bytes().get(key_len) != Some(&b':') {
        return None;
    }
    let rest = line[key_len + 1..].trim_start_matches(is_ws);
    if rest.contains(['\r', '\n', '\u{2028}', '\u{2029}']) {
        return None;
    }
    Some((&line[..key_len], rest))
}

/// `^\s+-\s+` and the trimmed remainder.
fn list_item(line: &str) -> Option<String> {
    let after_ws = line.trim_start_matches(is_ws);
    if after_ws.len() == line.len() {
        return None;
    }
    let after_dash = after_ws.strip_prefix('-')?;
    let rest = after_dash.trim_start_matches(is_ws);
    if rest.len() == after_dash.len() {
        return None;
    }
    Some(trim_ws(rest).to_string())
}

/// Compiles a constant pattern once. The patterns are literals covered by the tests, so a failure is a programming error.
#[allow(clippy::expect_used, reason = "constant patterns, compiled in every test")]
fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("constant regex"))
}

/// The anchor a forge gives a heading.
pub fn slug(heading: &str) -> String {
    static STRIP: OnceLock<Regex> = OnceLock::new();
    let lower = trim_ws(heading).to_lowercase().replace(['`', '*', '_', '~'], "");
    let spaced: String = lower.chars().map(|c| if is_ws(c) { '-' } else { c }).collect();
    re(&STRIP, r"[^\p{L}\p{N}-]").replace_all(&spaced, "").into_owned()
}

/// The anchors of every Markdown heading of `text` (a `#`-line; like the script, the whitespace after the hashes may run
/// over line ends).
pub fn headings_of(text: &str) -> Vec<String> {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    let ws = r"[\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]";
    let pattern = format!(r"(?m)^#{{1,6}}{ws}+([^\n\r\x{{2028}}\x{{2029}}]+?){ws}*#*$");
    re(&HEADING, &pattern)
        .captures_iter(text)
        .filter_map(|c| c.get(1))
        .map(|m| slug(m.as_str()))
        .collect()
}

/// A relative link of a Markdown text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The path part.
    pub target: String,
    /// The text after the first `#` (up to a second `#`), empty if none.
    pub anchor: String,
    /// 1-based line.
    pub line: usize,
}

/// Is `s` fenced-code opening/closing: `^\s*```.
fn is_fence(s: &str) -> bool {
    s.trim_start_matches(is_ws).starts_with("```")
}

/// Removes `` `...` `` spans (a backtick pair with no backtick inside), left to right.
fn without_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        match after.find('`') {
            Some(close) => {
                out.push_str(&rest[..open]);
                rest = &after[close + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Does `href` start with a scheme (`https:`) or `#`, which are not relative file links?
fn is_absolute_or_anchor(href: &str) -> bool {
    if href.starts_with('#') {
        return true;
    }
    let mut chars = href.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')) {
            return false;
        }
    }
    false
}

/// Relative link targets of a Markdown text, skipping fenced and inline code, absolute URLs and pure anchors.
pub fn links_of(text: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut fenced = false;
    for (i, raw) in text.split('\n').enumerate() {
        if is_fence(raw) {
            fenced = !fenced;
        }
        if fenced {
            continue;
        }
        let line = without_code_spans(raw);
        for href in hrefs(&line) {
            if is_absolute_or_anchor(href) {
                continue;
            }
            let mut parts = href.split('#');
            let target = parts.next().unwrap_or("").to_string();
            let anchor = parts.next().unwrap_or("").to_string();
            out.push(Link {
                target,
                anchor,
                line: i + 1,
            });
        }
    }
    out
}

/// The `(...)` parts of every `[text](href)` in `line` (`\[[^\]]*\]\(([^)\s]+)\)`, left to right, without overlap).
///
/// Linear time: every `[` before the same `]` fails the same way, so a failure resumes after that `]`; and a failed href scan
/// ended at a terminator that every later start inside the same run meets too, so it is remembered (`fail_end`).
fn hrefs(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    let mut fail_end = 0;
    while let Some(off) = line[from..].find('[') {
        let open = from + off;
        let Some(close_off) = line[open + 1..].find(']') else { break };
        let close = open + 1 + close_off;
        let start = close + 2;
        let Some(inner) = line[close + 1..].strip_prefix('(') else {
            from = close + 1;
            continue;
        };
        if start <= fail_end {
            from = close + 1;
            continue;
        }
        // No ')' or whitespace to the end of the line: no later start finds one either.
        let Some(len) = inner.find(|c: char| c == ')' || is_ws(c)) else {
            break;
        };
        if len > 0 && inner[len..].starts_with(')') {
            out.push(&inner[..len]);
            from = start + len + 1;
        } else {
            fail_end = start + len;
            from = close + 1;
        }
    }
    out
}

/// A bullet in progress: its line and its text with the continuation lines.
struct Bullet {
    line: usize,
    text: String,
}

/// The line numbers (1-based within `body`) of bullets that name no source: no Markdown link and no `src:`.
pub fn bullets_without_source(body: &str) -> Vec<usize> {
    let mut bad = Vec::new();
    let mut fenced = false;
    let mut bullet: Option<Bullet> = None;
    let mut close = |bullet: &mut Option<Bullet>| {
        if let Some(b) = bullet.take()
            && !names_source(&b.text)
        {
            bad.push(b.line);
        }
    };
    for (i, raw) in body.split('\n').enumerate() {
        if is_fence(raw) {
            fenced = !fenced;
        }
        if fenced {
            continue;
        }
        if is_bullet_start(raw) {
            close(&mut bullet);
            bullet = Some(Bullet {
                line: i + 1,
                text: raw.to_string(),
            });
        } else if let (Some(b), true) = (bullet.as_mut(), is_continuation(raw)) {
            b.text.push(' ');
            b.text.push_str(raw);
        } else {
            close(&mut bullet);
        }
    }
    close(&mut bullet);
    bad
}

/// `^\s*[-*]\s+\S`
fn is_bullet_start(raw: &str) -> bool {
    let t = raw.trim_start_matches(is_ws);
    let Some(after) = t.strip_prefix(['-', '*']) else { return false };
    let rest = after.trim_start_matches(is_ws);
    rest.len() < after.len() && !rest.is_empty()
}

/// `^\s+\S`
fn is_continuation(raw: &str) -> bool {
    let t = raw.trim_start_matches(is_ws);
    t.len() < raw.len() && !t.is_empty()
}

/// `\]\([^)]+\)|\bsrc:`
fn names_source(text: &str) -> bool {
    let mut from = 0;
    while let Some(off) = text[from..].find("](") {
        let after = &text[from + off + 2..];
        // No ')' anywhere after: no later "](" has one either, so stop looking (keeps the scan linear).
        let Some(len) = after.find(')') else { break };
        if len > 0 {
            return true;
        }
        from += off + 2;
    }
    let mut from = 0;
    while let Some(off) = text[from..].find("src:") {
        let at = from + off;
        if !text[..at].chars().next_back().is_some_and(is_word) {
            return true;
        }
        from = at + 4;
    }
    false
}

/// Does the line contain `\b\d{1,2}:\d{2}\b` (a clock time like 14:33)?
pub fn has_clock_time(line: &str) -> bool {
    let b = line.as_bytes();
    for (k, _) in line.match_indices(':') {
        let run = b[..k].iter().rev().take_while(|c| c.is_ascii_digit()).count();
        if !(1..=2).contains(&run) {
            continue;
        }
        let before_ok = line[..k - run].chars().next_back().is_none_or(|c| !is_word(c));
        let two = b.get(k + 1..k + 3).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
        if before_ok && two && line[k + 3..].chars().next().is_none_or(|c| !is_word(c)) {
            return true;
        }
    }
    false
}

/// The words of a text for the copy check: lower case, link syntax reduced to the link text, markup characters dropped,
/// split on anything but letters, digits, `'`, `.` and `-`, with `.`, `'` and `-` trimmed from each word's ends.
pub fn words(text: &str) -> Vec<String> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let lower = text.to_lowercase();
    let plain = replace_links(&lower);
    let cleaned: String = plain
        .chars()
        .map(|c| if matches!(c, '`' | '*' | '_' | '>' | '#' | '|') { ' ' } else { c })
        .collect();
    re(&SPLIT, r"[^\p{L}\p{N}'.\-]+")
        .split(&cleaned)
        .map(|w| w.trim_matches(['.', '\'', '-']))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// `[text](target)` becomes `text` (`\[([^\]]*)\]\([^)]*\)`). Linear time: a failure resumes after the `]` it met.
fn replace_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut from = 0;
    let mut copied = 0;
    let mut no_more_paren = false;
    while let Some(off) = s[from..].find('[') {
        let open = from + off;
        let Some(close_off) = s[open + 1..].find(']') else { break };
        let close = open + 1 + close_off;
        let hit = if no_more_paren {
            None
        } else if let Some(inner) = s[close + 1..].strip_prefix('(') {
            let end = inner.find(')');
            no_more_paren = end.is_none();
            end.map(|len| close + 2 + len + 1)
        } else {
            None
        };
        match hit {
            Some(end) => {
                out.push_str(&s[copied..open]);
                out.push_str(&s[open + 1..close]);
                copied = end;
                from = end;
            }
            None => from = close + 1,
        }
    }
    out.push_str(&s[copied..]);
    out
}

/// Every `n`-word window of `text`, in order of first appearance, each joined by a space.
pub fn windows(text: &str, n: usize) -> Vec<String> {
    let w = words(text);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    for i in 0..w.len().saturating_sub(n - 1) {
        let window = w[i..i + n].join(" ");
        if seen.insert(window.clone()) {
            out.push(window);
        }
    }
    out
}
