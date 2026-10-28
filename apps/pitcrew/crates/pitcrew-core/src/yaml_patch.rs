//! A minimal, comment-preserving patcher for the runner's `config.yaml` (ADR 0011).
//!
//! Pitcrew edits a handful of keys of a file the user owns, documents with comments and arranges in their own way.
//! Re-serialising the whole file (parse, change, dump) would lose every comment and reorder or reformat keys, so the
//! patcher changes **only the lines of the keys it is asked to change**: a changed value keeps its key text, its
//! spacing and its trailing comment; a new key is added at the end of its section (after a commented-out
//! `#key: default` line of the same section when there is one, which is how `config.reference.yaml` documents
//! options); a removed key takes its own lines (and its list items or sub-keys) with it. Everything else, byte for
//! byte, stays: other keys, blank lines, comments, indentation, line endings and the final newline.
//!
//! It handles the block style that gitea-runner's configuration uses (mappings of mappings, scalars, lists of
//! scalars, mappings of scalars, and `[a, b]` / `{}` on one line). Anything else it meets on the way (a block
//! scalar, an anchor, a list of mappings, a tab used for indentation) is refused with a message that says so rather
//! than guessed at: the Advanced editor is for those files. The whole module is pure.

use std::collections::HashSet;
use std::fmt;

/// Largest document the patcher accepts.
pub const MAX_PATCH_BYTES: usize = 250_000;
/// Deepest key path.
pub const MAX_PATH_DEPTH: usize = 6;
/// Indentation of new nested lines.
const INDENT: usize = 2;

/// A scalar value to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    /// Text (quoted when plain text would mean something else).
    Str(String),
    /// A whole number.
    Int(i64),
    /// `true` or `false`.
    Bool(bool),
}

impl Scalar {
    /// The text a reader gets back from the value (what YAML would parse the written value as).
    fn text(&self) -> String {
        match self {
            Scalar::Str(s) => s.clone(),
            Scalar::Int(n) => n.to_string(),
            Scalar::Bool(b) => b.to_string(),
        }
    }

    /// The value as YAML source.
    fn render(&self) -> String {
        match self {
            Scalar::Str(s) => render_string(s),
            Scalar::Int(n) => n.to_string(),
            Scalar::Bool(b) => b.to_string(),
        }
    }
}

/// One change to the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Sets `path` to a scalar (adding the key and its sections when missing).
    Set {
        /// Keys from the root, e.g. `["runner", "capacity"]`.
        path: Vec<String>,
        /// The value.
        value: Scalar,
    },
    /// Sets `path` to a list of strings (an empty list removes the items).
    SetList {
        /// Keys from the root.
        path: Vec<String>,
        /// The items, in order.
        items: Vec<String>,
    },
    /// Sets `path` to a mapping of strings to strings (keys not listed are removed).
    SetMap {
        /// Keys from the root.
        path: Vec<String>,
        /// The entries, in order.
        entries: Vec<(String, String)>,
    },
    /// Removes `path` (the key and everything under it); nothing happens when it is absent.
    Remove {
        /// Keys from the root.
        path: Vec<String>,
    },
}

impl Edit {
    /// The key path the edit is about.
    pub fn path(&self) -> &[String] {
        match self {
            Edit::Set { path, .. } | Edit::SetList { path, .. } | Edit::SetMap { path, .. } | Edit::Remove { path } => path,
        }
    }
}

/// Why a patch could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchError {
    /// The key path, dotted.
    pub path: String,
    /// What is in the way, for the user.
    pub reason: String,
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "the file cannot be edited by the form: {}; use the Advanced editor", self.reason)
        } else {
            write!(
                f,
                "`{}` cannot be edited by the form: {}; use the Advanced editor",
                self.path, self.reason
            )
        }
    }
}

impl std::error::Error for PatchError {}

fn refuse<T>(path: &[String], reason: impl Into<String>) -> Result<T, PatchError> {
    Err(PatchError {
        path: path.join("."),
        reason: reason.into(),
    })
}

/// Applies `edits` in order and returns the new text. The input is returned unchanged when nothing needs to change.
pub fn apply(text: &str, edits: &[Edit]) -> Result<String, PatchError> {
    if text.len() > MAX_PATCH_BYTES {
        return refuse(
            &[],
            format!(
                "it is larger than {}",
                paddock_units::format_whole(MAX_PATCH_BYTES as u64, paddock_units::Kind::Storage)
            ),
        );
    }
    let mut doc = Doc::parse(text)?;
    for e in edits {
        let path = e.path();
        if path.is_empty() || path.len() > MAX_PATH_DEPTH || path.iter().any(|s| s.is_empty() || s.len() > 128) {
            return refuse(path, "the key path is not valid");
        }
        match e {
            Edit::Set { path, value } => doc.set_scalar(path, value)?,
            Edit::SetList { path, items } => doc.set_list(path, items)?,
            Edit::SetMap { path, entries } => doc.set_map(path, entries)?,
            Edit::Remove { path } => doc.remove(path)?,
        }
    }
    Ok(doc.render(text))
}

/// The line (0-based) of the key at `path`, when the document has it.
pub fn line_of(text: &str, path: &[String]) -> Option<usize> {
    let doc = Doc::parse(text).ok()?;
    match doc.locate(path) {
        Ok(Ok(i)) => Some(i),
        _ => None,
    }
}

// ---- scalars -------------------------------------------------------------------------------------------------------

/// A string as YAML source: plain when that reads back as the same string, else double-quoted.
fn render_string(s: &str) -> String {
    if plain_ok(s) {
        s.to_string()
    } else {
        quote(s)
    }
}

/// `s` double-quoted, with the escapes YAML needs.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether `s` can be written without quotes and still be read as the same string.
fn plain_ok(s: &str) -> bool {
    let Some(first) = s.chars().next() else { return false };
    if !(first.is_ascii_alphanumeric() || first == '/' || first == '.') {
        return false;
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || "_./-+@%".contains(c)) {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "y" | "n") {
        return false;
    }
    // Numbers, including `1e3`, `.5`, `0x1f`, `1_000` and `.inf`.
    let numeric = lower.trim_start_matches(['+', '-']);
    if numeric.parse::<f64>().is_ok() || numeric.starts_with("0x") || numeric.starts_with("0o") || numeric.starts_with("0b") {
        return false;
    }
    if numeric.chars().all(|c| c.is_ascii_digit() || c == '_' || c == '.') && numeric.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    !matches!(lower.as_str(), ".inf" | ".nan" | "." | "..")
}

/// The text a scalar written in YAML source stands for: quotes removed and escapes resolved.
fn unquote(v: &str) -> String {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('0') => out.push('\0'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{fffd}'));
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        out
    } else if let Some(inner) = v.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.replace("''", "'")
    } else {
        v.to_string()
    }
}

/// Splits what follows a key's colon (or a list item's dash) into `(value, comment)`, the comment starting at its
/// leading white space (`" # note"`) and the value having none around it. Quotes are respected.
fn split_comment(rest: &str) -> (&str, &str) {
    let b = rest.as_bytes();
    let mut i = 0;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(b'"') => {
                if c == b'\\' {
                    i += 1;
                } else if c == b'"' {
                    quote = None;
                }
            }
            Some(_) => {
                if c == b'\'' {
                    quote = None;
                }
            }
            None => {
                if (c == b'"' || c == b'\'') && value_start(b, i) {
                    quote = Some(c);
                } else if c == b'#' && (i == 0 || b[i - 1] == b' ' || b[i - 1] == b'\t') {
                    let mut start = i;
                    while start > 0 && (b[start - 1] == b' ' || b[start - 1] == b'\t') {
                        start -= 1;
                    }
                    return (rest[..start].trim_start(), &rest[start..]);
                }
            }
        }
        i += 1;
    }
    (rest.trim(), "")
}

/// Whether a quote at `i` opens a quoted scalar (rather than being an apostrophe inside a plain one).
fn value_start(b: &[u8], i: usize) -> bool {
    b[..i]
        .iter()
        .all(|c| *c == b' ' || *c == b'\t' || *c == b'[' || *c == b',' || *c == b'{')
        || (i > 0 && matches!(b[i - 1], b'[' | b',' | b' ') && b[..i].iter().rev().find(|c| **c != b' ') == Some(&b','))
}

// ---- lines ---------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Blank,
    Comment,
    Content(usize),
}

fn kind(line: &str) -> Kind {
    let t = line.trim_start_matches(' ');
    if t.trim().is_empty() {
        Kind::Blank
    } else if t.starts_with('#') || (line.len() == t.len() && (t.starts_with("---") || t.starts_with("..."))) {
        Kind::Comment
    } else {
        Kind::Content(line.len() - t.len())
    }
}

/// A `key: value` line.
struct KeyLine {
    key: String,
    /// Byte offset just after the colon.
    after_colon: usize,
}

fn parse_key(line: &str) -> Option<KeyLine> {
    let Kind::Content(indent) = kind(line) else { return None };
    let t = &line[indent..];
    if t == "-" || t.starts_with("- ") {
        return None;
    }
    let first = t.chars().next()?;
    let (key, end) = if first == '"' || first == '\'' {
        let b = t.as_bytes();
        let mut i = 1;
        while i < b.len() {
            if first == '"' && b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == first as u8 {
                if first == '\'' && b.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                break;
            }
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        let after = t[i + 1..].trim_start_matches(' ');
        if !after.starts_with(':') || !(after.len() == 1 || after[1..].starts_with([' ', '\t'])) {
            return None;
        }
        (unquote(&t[..=i]), t.len() - after.len() + 1)
    } else {
        if "[{?&*!|>%@`#,".contains(first) {
            return None;
        }
        let b = t.as_bytes();
        let mut end = None;
        for i in 0..b.len() {
            if b[i] == b':' && (i + 1 == b.len() || b[i + 1] == b' ' || b[i + 1] == b'\t') {
                end = Some(i);
                break;
            }
            if b[i] == b'#' && i > 0 && b[i - 1] == b' ' {
                return None;
            }
        }
        let end = end?;
        (t[..end].trim_end().to_string(), end + 1)
    };
    Some(KeyLine {
        key,
        after_colon: indent + end,
    })
}

/// The document as lines, with what is needed to put it back together exactly.
struct Doc {
    lines: Vec<String>,
    eol: &'static str,
    final_eol: bool,
    changed: bool,
}

/// Where a key was found.
struct Block {
    /// The first line after the parent.
    first: usize,
    /// The last content line inside the block, when there is one.
    last: Option<usize>,
    /// The indentation of the block's first content line.
    indent: Option<usize>,
    /// The first line that is no longer part of the section (comments after its last content line included).
    extent: usize,
}

impl Doc {
    fn parse(text: &str) -> Result<Doc, PatchError> {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let final_eol = text.ends_with('\n');
        let body = text.strip_suffix('\n').unwrap_or(text);
        let body = body.strip_suffix('\r').unwrap_or(body);
        let lines: Vec<String> = if text.is_empty() {
            vec![]
        } else {
            body.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect()
        };
        for l in &lines {
            let lead = &l[..l.len() - l.trim_start_matches([' ', '\t']).len()];
            if lead.contains('\t') && !l.trim().is_empty() {
                return refuse(&[], "it is indented with tabs, which YAML does not allow");
            }
        }
        Ok(Doc {
            lines,
            eol,
            final_eol,
            changed: false,
        })
    }

    fn render(&self, original: &str) -> String {
        if !self.changed {
            return original.to_string();
        }
        let mut out = self.lines.join(self.eol);
        if self.final_eol || original.is_empty() {
            out.push_str(self.eol);
        }
        out
    }

    fn indent_of(&self, i: usize) -> Option<usize> {
        match kind(&self.lines[i]) {
            Kind::Content(n) => Some(n),
            _ => None,
        }
    }

    fn is_item(&self, i: usize) -> bool {
        let t = self.lines[i].trim_start_matches(' ');
        matches!(kind(&self.lines[i]), Kind::Content(_)) && (t == "-" || t.starts_with("- "))
    }

    /// The text after the colon of key line `i`, split into `(value, comment)`.
    fn value_of(&self, i: usize) -> (String, String) {
        parse_key(&self.lines[i]).map_or((String::new(), String::new()), |k| {
            let (v, c) = split_comment(&self.lines[i][k.after_colon..]);
            (v.to_string(), c.to_string())
        })
    }

    /// The text of key line `i` up to and including its colon.
    fn head_of(&self, i: usize) -> String {
        let end = parse_key(&self.lines[i]).map_or(self.lines[i].len(), |k| k.after_colon);
        self.lines[i][..end].to_string()
    }

    /// The lines under `parent` (`None`: the whole document).
    fn block(&self, parent: Option<usize>) -> Block {
        let (first, parent_indent, seq_at_key) = match parent {
            Some(p) => {
                let indent = self.indent_of(p).unwrap_or(0) as i64;
                (p + 1, indent, self.value_of(p).0.is_empty())
            }
            None => (0, -1, false),
        };
        let mut last = None;
        let mut indent = None;
        let mut i = first;
        while i < self.lines.len() {
            if let Kind::Content(n) = kind(&self.lines[i]) {
                let n64 = n as i64;
                let inside = n64 > parent_indent || (n64 == parent_indent && seq_at_key && self.is_item(i));
                if !inside {
                    break;
                }
                last = Some(i);
                if indent.is_none() {
                    indent = Some(n);
                }
            }
            i += 1;
        }
        Block {
            first,
            last,
            indent,
            extent: i,
        }
    }

    /// The line of key `key` directly inside `parent`.
    fn child(&self, parent: Option<usize>, key: &str) -> Result<Option<usize>, ()> {
        if let Some(p) = parent {
            let (v, _) = self.value_of(p);
            if !(v.is_empty() || v == "{}" || v == "~" || v == "null") {
                return Err(());
            }
        }
        let b = self.block(parent);
        let Some(want) = b.indent else { return Ok(None) };
        let end = b.last.map_or(b.first, |l| l + 1);
        for i in b.first..end {
            if self.indent_of(i) == Some(want) {
                if let Some(k) = parse_key(&self.lines[i]) {
                    if k.key == key {
                        return Ok(Some(i));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Walks `path`: the line of its last key, or the deepest existing parent and the keys still missing.
    fn locate(&self, path: &[String]) -> Result<Result<usize, (Option<usize>, usize)>, PatchError> {
        let mut parent = None;
        for (n, seg) in path.iter().enumerate() {
            match self.child(parent, seg) {
                Err(()) => return refuse(&path[..n], "it holds a value, not a section"),
                Ok(Some(i)) => parent = Some(i),
                Ok(None) => return Ok(Err((parent, n))),
            }
        }
        Ok(parent.ok_or((None, 0)))
    }

    fn insert(&mut self, at: usize, new: Vec<String>) {
        if new.is_empty() {
            return;
        }
        let tail = self.lines.split_off(at);
        self.lines.extend(new);
        self.lines.extend(tail);
        self.changed = true;
    }

    fn remove_lines(&mut self, from: usize, to_exclusive: usize) {
        if from < to_exclusive {
            self.lines.drain(from..to_exclusive);
            self.changed = true;
        }
    }

    fn replace_line(&mut self, i: usize, new: String) {
        if self.lines[i] != new {
            self.lines[i] = new;
            self.changed = true;
        }
    }

    /// Turns `key: {}` / `key: ~` / `key: null` into `key:` so children can follow; keeps the comment.
    fn open_section(&mut self, i: usize) {
        let (v, c) = self.value_of(i);
        if !v.is_empty() {
            let head = self.head_of(i);
            self.replace_line(i, format!("{head}{c}"));
        }
    }

    /// Where a new key `key` goes in the block of `parent`, and the indentation of its siblings.
    fn slot(&self, parent: Option<usize>, key: &str) -> (usize, usize) {
        let b = self.block(parent);
        let parent_indent = parent.and_then(|p| self.indent_of(p));
        let indent = b.indent.unwrap_or_else(|| parent_indent.map_or(0, |n| n + INDENT));
        let end = b.last.map_or(b.first, |l| l + 1);
        // After the commented-out default of the same key (`#key: value` and its `#  sub: value` lines).
        let placeholder = format!("#{key}:");
        let min_indent = parent_indent.map_or(0, |n| n + 1);
        for i in b.first..b.extent {
            let line = &self.lines[i];
            let t = line.trim_start_matches(' ');
            if line.len() - t.len() >= min_indent && (t == placeholder || t.starts_with(&format!("{placeholder} "))) {
                let mut at = i + 1;
                while at < b.extent && self.lines[at].trim_start_matches(' ').starts_with("#  ") {
                    at += 1;
                }
                return (at, indent);
            }
        }
        (end, indent)
    }

    /// Creates the missing keys of `path` (from index `missing`) under `parent`, with `leaf` as the last one's body.
    fn create(&mut self, parent: Option<usize>, path: &[String], missing: usize, leaf: &Leaf) {
        let segs = &path[missing..];
        let Some(first) = segs.first() else { return };
        if let Some(p) = parent {
            self.open_section(p);
        }
        let (at, mut indent) = self.slot(parent, first);
        let mut new = vec![];
        for (n, seg) in segs.iter().enumerate() {
            let key = render_key(seg);
            if n + 1 == segs.len() {
                new.extend(leaf.lines(indent, &key));
            } else {
                new.push(format!("{}{key}:", " ".repeat(indent)));
                indent += INDENT;
            }
        }
        self.insert(at, new);
    }

    // ---- the edits -------------------------------------------------------------------------------------------------

    fn set_scalar(&mut self, path: &[String], value: &Scalar) -> Result<(), PatchError> {
        match self.locate(path)? {
            Err((parent, missing)) => {
                self.create(parent, path, missing, &Leaf::Scalar(value.render()));
                Ok(())
            }
            Ok(i) => {
                let (current, comment) = self.value_of(i);
                if self.block(Some(i)).last.is_some() {
                    return refuse(path, "it is a section or a list, not a single value");
                }
                if current.starts_with(['|', '>', '&', '*', '!', '[', '{']) && current != "{}" && current != "[]" {
                    return refuse(path, "its value is written in a style the form does not edit");
                }
                if !current.is_empty() && unquote(&current) == value.text() && current != "{}" && current != "[]" {
                    return Ok(());
                }
                let head = self.head_of(i);
                self.replace_line(i, format!("{head} {}{comment}", value.render()));
                Ok(())
            }
        }
    }

    fn set_list(&mut self, path: &[String], items: &[String]) -> Result<(), PatchError> {
        match self.locate(path)? {
            Err((parent, missing)) => {
                if !items.is_empty() {
                    self.create(parent, path, missing, &Leaf::List(items.to_vec()));
                }
                Ok(())
            }
            Ok(i) => self.rewrite_list(i, path, items),
        }
    }

    fn rewrite_list(&mut self, i: usize, path: &[String], items: &[String]) -> Result<(), PatchError> {
        let (value, comment) = self.value_of(i);
        let head = self.head_of(i);
        let b = self.block(Some(i));
        // A list on one line: `[a, "b"]` or `[]`.
        if let Some(inner) = value.strip_prefix('[') {
            if b.last.is_some() {
                return refuse(path, "it mixes a one-line list with more lines");
            }
            let Some(inner) = inner.strip_suffix(']') else {
                return refuse(path, "its list spans several lines in a style the form does not edit");
            };
            let Some(current) = parse_flow_items(inner) else {
                return refuse(path, "its list is written in a style the form does not edit");
            };
            if current == items {
                return Ok(());
            }
            if items.is_empty() {
                self.replace_line(i, format!("{head} []{comment}"));
            } else {
                let flow: Vec<String> = items.iter().map(|s| render_string(s)).collect();
                self.replace_line(i, format!("{head} [{}]{comment}", flow.join(", ")));
            }
            return Ok(());
        }
        if !value.is_empty() && value != "~" && value != "null" {
            return refuse(path, "it is a single value, not a list");
        }
        // A block list below the key (or nothing yet).
        let mut old: Vec<(usize, String)> = vec![];
        if let Some(last) = b.last {
            for n in b.first..=last {
                if matches!(kind(&self.lines[n]), Kind::Content(_)) {
                    if !self.is_item(n) {
                        return refuse(path, "it is a section, not a list");
                    }
                    let t = self.lines[n].trim_start_matches(' ');
                    let (v, _) = split_comment(t.strip_prefix('-').unwrap_or("").trim_start());
                    if parse_key(&format!("x: {v}")).is_none() || v.starts_with(['[', '{', '&', '*', '!', '|', '>']) || has_map_key(v) {
                        return refuse(path, "its items are not plain text");
                    }
                    let item_indent = self.indent_of(n).unwrap_or(0);
                    if b.indent != Some(item_indent) {
                        return refuse(path, "its items are not aligned");
                    }
                    old.push((n, unquote(v)));
                }
            }
        }
        let old_values: Vec<&String> = old.iter().map(|(_, v)| v).collect();
        if old_values.iter().copied().eq(items.iter()) {
            return Ok(());
        }
        let item_indent = b.indent.unwrap_or_else(|| self.indent_of(i).unwrap_or(0) + INDENT);
        let plain = |s: &String| format!("{}- {}", " ".repeat(item_indent), render_string(s));
        let unique = items.iter().collect::<HashSet<_>>().len() == items.len()
            && old_values.iter().collect::<HashSet<_>>().len() == old_values.len();
        let kept: Vec<&String> = old_values.iter().copied().filter(|v| items.contains(v)).collect();
        let kept_in_order: Vec<&String> = items.iter().filter(|v| old_values.contains(v)).collect();
        // Items that stay keep their own line and the comments that follow it; new items are written plain.
        let preserve = unique && kept == kept_in_order;
        let mut region: Vec<String> = vec![];
        for item in items {
            match old_values.iter().position(|v| *v == item).filter(|_| preserve) {
                Some(k) => {
                    let from = old[k].0;
                    let to = old.get(k + 1).map_or(b.last.map_or(from + 1, |l| l + 1), |n| n.0);
                    region.extend(self.lines[from..to].iter().cloned());
                }
                None => region.push(plain(item)),
            }
        }
        match (old.first().map(|o| o.0), b.last) {
            (Some(first), Some(last)) => {
                self.remove_lines(first, last + 1);
                self.insert(first, region);
            }
            _ => self.insert(i + 1, region),
        }
        if items.is_empty() {
            let (_, c) = self.value_of(i);
            self.replace_line(i, format!("{head} []{c}"));
        } else if value == "~" || value == "null" {
            self.replace_line(i, format!("{head}{comment}"));
        }
        Ok(())
    }

    fn set_map(&mut self, path: &[String], entries: &[(String, String)]) -> Result<(), PatchError> {
        match self.locate(path)? {
            Err((parent, missing)) => {
                if !entries.is_empty() {
                    self.create(parent, path, missing, &Leaf::Map(entries.to_vec()));
                }
                Ok(())
            }
            Ok(i) => {
                let (value, comment) = self.value_of(i);
                let head = self.head_of(i);
                if !value.is_empty() && value != "{}" && value != "~" && value != "null" {
                    return refuse(path, "it is not a block of `name: value` lines");
                }
                // Existing entries must be plain `name: value` lines.
                let b = self.block(Some(i));
                let mut existing: Vec<String> = vec![];
                if let (Some(last), Some(indent)) = (b.last, b.indent) {
                    for n in b.first..=last {
                        if self.indent_of(n) == Some(indent) {
                            match parse_key(&self.lines[n]) {
                                Some(kl) => existing.push(kl.key),
                                None => return refuse(path, "it holds something other than `name: value` lines"),
                            }
                            let (v, _) = self.value_of(n);
                            if v.is_empty() || v.starts_with(['|', '>', '&', '*', '!', '[', '{']) {
                                return refuse(path, "its values are not plain text");
                            }
                        } else if matches!(kind(&self.lines[n]), Kind::Content(_)) {
                            return refuse(path, "its values span several lines");
                        }
                    }
                }
                let names: Vec<&String> = entries.iter().map(|(k, _)| k).collect();
                for key in existing.iter().rev() {
                    if !names.contains(&key) {
                        let mut p = path.to_vec();
                        p.push(key.clone());
                        self.remove(&p)?;
                    }
                }
                for (k, v) in entries {
                    let mut p = path.to_vec();
                    p.push(k.clone());
                    self.set_scalar(&p, &Scalar::Str(v.clone()))?;
                }
                if entries.is_empty() && value != "{}" {
                    self.replace_line(i, format!("{head} {{}}{comment}"));
                } else if !entries.is_empty() {
                    self.open_section(i);
                }
                Ok(())
            }
        }
    }

    fn remove(&mut self, path: &[String]) -> Result<(), PatchError> {
        match self.locate(path)? {
            Err(_) => Ok(()),
            Ok(i) => {
                let end = self.block(Some(i)).last.map_or(i + 1, |l| l + 1);
                self.remove_lines(i, end);
                Ok(())
            }
        }
    }
}

/// What a new key holds.
enum Leaf {
    Scalar(String),
    List(Vec<String>),
    Map(Vec<(String, String)>),
}

impl Leaf {
    fn lines(&self, indent: usize, key: &str) -> Vec<String> {
        let pad = " ".repeat(indent);
        match self {
            Leaf::Scalar(v) => vec![format!("{pad}{key}: {v}")],
            Leaf::List(items) => std::iter::once(format!("{pad}{key}:"))
                .chain(items.iter().map(|s| format!("{pad}{}- {}", " ".repeat(INDENT), render_string(s))))
                .collect(),
            Leaf::Map(entries) => std::iter::once(format!("{pad}{key}:"))
                .chain(
                    entries
                        .iter()
                        .map(|(k, v)| format!("{pad}{}{}: {}", " ".repeat(INDENT), render_key(k), render_string(v))),
                )
                .collect(),
        }
    }
}

/// A key as YAML source.
fn render_key(k: &str) -> String {
    if k.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)) && !k.is_empty() && !k.starts_with('-') {
        k.to_string()
    } else {
        quote(k)
    }
}

/// Whether the text of a list item reads as `key: value` (a mapping inside the list).
fn has_map_key(v: &str) -> bool {
    if v.starts_with('"') || v.starts_with('\'') {
        return false;
    }
    v.contains(": ") || v.ends_with(':')
}

/// The items of a one-line list's inside (`a, "b, c"`); `None` when it holds anything but plain items.
fn parse_flow_items(inner: &str) -> Option<Vec<String>> {
    let inner = inner.trim();
    if inner.is_empty() {
        return Some(vec![]);
    }
    let mut items = vec![];
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == '\\' && q == '"' {
                    cur.push(chars.next()?);
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '[' | '{' | ']' | '}' => return None,
                ',' => items.push(std::mem::take(&mut cur)),
                _ => cur.push(c),
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    items.push(cur);
    Some(items.iter().map(|s| unquote(s.trim())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        s.split('.').map(str::to_string).collect()
    }
    fn set(path: &str, v: Scalar) -> Edit {
        Edit::Set { path: p(path), value: v }
    }
    fn int(n: i64) -> Scalar {
        Scalar::Int(n)
    }
    fn st(s: &str) -> Scalar {
        Scalar::Str(s.into())
    }

    const REF: &str = "\
# Every option, commented out.
log:
  # Severity.
  #level: info
runner:
  # Execute how many tasks.
  #capacity: 1
  #envs:
  #  A: b
  # The timeout.
  timeout: 3h # three hours
  labels:
    - \"a:host\"   # first
    - b
  # Last note
cache:
  enabled: true
";

    #[test]
    fn a_value_keeps_its_comment_and_neighbours() {
        let out = apply(REF, &[set("runner.timeout", st("2h"))]).unwrap();
        assert_eq!(out, REF.replace("timeout: 3h # three hours", "timeout: 2h # three hours"));
    }

    #[test]
    fn the_same_value_changes_nothing_even_when_quoted_differently() {
        assert_eq!(apply(REF, &[set("runner.timeout", st("3h"))]).unwrap(), REF);
        let q = "a:\n  b: \"3h\"\n";
        assert_eq!(apply(q, &[set("a.b", st("3h"))]).unwrap(), q);
        let n = "a:\n  b: 4 # n\n";
        assert_eq!(apply(n, &[set("a.b", int(4))]).unwrap(), n);
        assert_eq!(apply(n, &[set("a.b", int(5))]).unwrap(), "a:\n  b: 5 # n\n");
        let single = "a:\n  b: 'it''s'\n";
        assert_eq!(apply(single, &[set("a.b", st("it's"))]).unwrap(), single);
        let esc = "a:\n  b: \"x\\ty\\u0041\"\n";
        assert_eq!(apply(esc, &[set("a.b", st("x\tyA"))]).unwrap(), esc);
    }

    #[test]
    fn a_new_key_goes_after_its_commented_default() {
        let out = apply(REF, &[set("runner.capacity", int(2))]).unwrap();
        assert!(out.contains("  #capacity: 1\n  capacity: 2\n  #envs:"), "{out}");
        let out = apply(REF, &[set("log.level", st("debug"))]).unwrap();
        assert!(
            out.contains("  #level: info\n  level: debug\ncache:") || out.contains("  #level: info\n  level: debug\nrunner:"),
            "{out}"
        );
        // The comment lines of the commented default's sub-keys stay with it.
        let out = apply(
            REF,
            &[Edit::SetMap {
                path: p("runner.envs"),
                entries: vec![("X".into(), "1".into())],
            }],
        )
        .unwrap();
        assert!(out.contains("  #envs:\n  #  A: b\n  envs:\n    X: \"1\"\n"), "{out}");
    }

    #[test]
    fn missing_sections_are_created_where_they_belong() {
        let out = apply("log:\n  level: info\n", &[set("container.network", st("host"))]).unwrap();
        assert_eq!(out, "log:\n  level: info\ncontainer:\n  network: host\n");
        let out = apply("", &[set("a.b.c", int(1))]).unwrap();
        assert_eq!(out, "a:\n  b:\n    c: 1\n");
        let out = apply("a: {}\n", &[set("a.b", Scalar::Bool(true))]).unwrap();
        assert_eq!(out, "a:\n  b: true\n");
        let out = apply("a: # note\n", &[set("a.b", int(1))]).unwrap();
        assert_eq!(out, "a: # note\n  b: 1\n");
        let out = apply("x: 1", &[set("y", int(2))]).unwrap();
        assert_eq!(out, "x: 1\ny: 2", "no final newline stays so");
    }

    #[test]
    fn a_sibling_section_is_not_confused_with_a_nested_key() {
        let t = "a:\n  size: 1\nb:\n  size: 2\n";
        assert_eq!(apply(t, &[set("b.size", int(9))]).unwrap(), "a:\n  size: 1\nb:\n  size: 9\n");
        assert_eq!(apply(t, &[set("c.size", int(9))]).unwrap(), format!("{t}c:\n  size: 9\n"));
    }

    #[test]
    fn lists_are_changed_item_by_item() {
        // Removing and adding keeps the other items and their comments.
        let out = apply(
            REF,
            &[Edit::SetList {
                path: p("runner.labels"),
                items: vec!["a:host".into(), "c".into()],
            }],
        )
        .unwrap();
        assert!(out.contains("    - \"a:host\"   # first\n    - c\n  # Last note"), "{out}");
        assert!(!out.contains("    - b\n"));
        // Same items: untouched.
        let same = apply(
            REF,
            &[Edit::SetList {
                path: p("runner.labels"),
                items: vec!["a:host".into(), "b".into()],
            }],
        )
        .unwrap();
        assert_eq!(same, REF);
        // Reordered: rewritten in place.
        let re = apply(
            REF,
            &[Edit::SetList {
                path: p("runner.labels"),
                items: vec!["b".into(), "a:host".into()],
            }],
        )
        .unwrap();
        assert!(re.contains("    - b\n    - \"a:host\"\n  # Last note"), "{re}");
        // Emptied.
        let none = apply(
            REF,
            &[Edit::SetList {
                path: p("runner.labels"),
                items: vec![],
            }],
        )
        .unwrap();
        assert!(none.contains("  labels: []\n  # Last note"), "{none}");
        // Created, at the key's indentation style.
        let new = apply(
            "runner:\n  capacity: 1\n",
            &[Edit::SetList {
                path: p("runner.labels"),
                items: vec!["x:host".into()],
            }],
        )
        .unwrap();
        assert_eq!(new, "runner:\n  capacity: 1\n  labels:\n    - \"x:host\"\n");
        // Nothing to create for an empty list.
        assert_eq!(
            apply(
                "a: 1\n",
                &[Edit::SetList {
                    path: p("b"),
                    items: vec![]
                }]
            )
            .unwrap(),
            "a: 1\n"
        );
        // Items at the key's own indentation are understood.
        let flat = "l:\n- a\n- b\nz: 1\n";
        assert_eq!(
            apply(
                flat,
                &[Edit::SetList {
                    path: p("l"),
                    items: vec!["a".into(), "b".into(), "c".into()]
                }]
            )
            .unwrap(),
            "l:\n- a\n- b\n- c\nz: 1\n"
        );
        // Duplicates are written as given.
        let dup = apply(
            "l:\n  - a\n",
            &[Edit::SetList {
                path: p("l"),
                items: vec!["a".into(), "a".into()],
            }],
        )
        .unwrap();
        assert_eq!(dup, "l:\n  - a\n  - a\n");
        // A null value becomes a list.
        assert_eq!(
            apply(
                "l: # c\nz: 1\n",
                &[Edit::SetList {
                    path: p("l"),
                    items: vec!["a".into()]
                }]
            )
            .unwrap(),
            "l: # c\n  - a\nz: 1\n"
        );
    }

    #[test]
    fn one_line_lists_stay_on_one_line() {
        let t = "l: [a, \"b, c\"] # note\n";
        assert_eq!(
            apply(
                t,
                &[Edit::SetList {
                    path: p("l"),
                    items: vec!["a".into(), "b, c".into()]
                }]
            )
            .unwrap(),
            t
        );
        assert_eq!(
            apply(
                t,
                &[Edit::SetList {
                    path: p("l"),
                    items: vec!["a".into()]
                }]
            )
            .unwrap(),
            "l: [a] # note\n"
        );
        assert_eq!(
            apply(
                t,
                &[Edit::SetList {
                    path: p("l"),
                    items: vec![]
                }]
            )
            .unwrap(),
            "l: [] # note\n"
        );
        assert_eq!(
            apply(
                "l: []\n",
                &[Edit::SetList {
                    path: p("l"),
                    items: vec![]
                }]
            )
            .unwrap(),
            "l: []\n"
        );
        assert!(apply(
            "l: [a, [b]]\n",
            &[Edit::SetList {
                path: p("l"),
                items: vec![]
            }]
        )
        .is_err());
        assert!(apply(
            "l: [a, 'b\n",
            &[Edit::SetList {
                path: p("l"),
                items: vec![]
            }]
        )
        .is_err());
        assert!(apply(
            "l: [a,\n  b]\n",
            &[Edit::SetList {
                path: p("l"),
                items: vec![]
            }]
        )
        .is_err());
    }

    #[test]
    fn maps_are_changed_key_by_key() {
        let t = "runner:\n  envs:\n    A: \"1\"   # keep\n    B: 2\n  other: x\n";
        let out = apply(
            t,
            &[Edit::SetMap {
                path: p("runner.envs"),
                entries: vec![("A".into(), "1".into()), ("C".into(), "3".into())],
            }],
        )
        .unwrap();
        assert_eq!(out, "runner:\n  envs:\n    A: \"1\"   # keep\n    C: \"3\"\n  other: x\n");
        let cleared = apply(
            t,
            &[Edit::SetMap {
                path: p("runner.envs"),
                entries: vec![],
            }],
        )
        .unwrap();
        assert_eq!(cleared, "runner:\n  envs: {}\n  other: x\n");
        assert_eq!(
            apply(
                "a: {}\n",
                &[Edit::SetMap {
                    path: p("a"),
                    entries: vec![]
                }]
            )
            .unwrap(),
            "a: {}\n"
        );
        let created = apply(
            "a: 1\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![("K".into(), "v w".into())],
            }],
        )
        .unwrap();
        assert_eq!(created, "a: 1\ne:\n  K: \"v w\"\n");
        assert_eq!(
            apply(
                "a: 1\n",
                &[Edit::SetMap {
                    path: p("e"),
                    entries: vec![]
                }]
            )
            .unwrap(),
            "a: 1\n"
        );
        let from_empty = apply(
            "e: {}\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![("K".into(), "v".into())],
            }],
        )
        .unwrap();
        assert_eq!(from_empty, "e:\n  K: v\n");
        assert!(apply(
            "e: 5\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![]
            }]
        )
        .is_err());
        assert!(apply(
            "e:\n  k:\n    n: 1\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![]
            }]
        )
        .is_err());
        assert!(apply(
            "e:\n  k: |\n    text\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![]
            }]
        )
        .is_err());
        assert!(apply(
            "e:\n  - a\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![]
            }]
        )
        .is_err());
    }

    #[test]
    fn removing_takes_the_key_and_what_is_under_it() {
        let out = apply(REF, &[Edit::Remove { path: p("runner.labels") }]).unwrap();
        assert!(out.contains("  timeout: 3h # three hours\n  # Last note\n"), "{out}");
        assert!(!out.contains("first"));
        let out = apply(REF, &[Edit::Remove { path: p("runner.timeout") }]).unwrap();
        assert!(!out.contains("timeout: 3h") && out.contains("  # The timeout.\n  labels:"), "{out}");
        assert_eq!(apply(REF, &[Edit::Remove { path: p("nothing.here") }]).unwrap(), REF);
    }

    #[test]
    fn crlf_and_missing_final_newline_are_kept() {
        let t = "a:\r\n  b: 1\r\n";
        assert_eq!(apply(t, &[set("a.b", int(2))]).unwrap(), "a:\r\n  b: 2\r\n");
        assert_eq!(apply(t, &[set("a.c", int(3))]).unwrap(), "a:\r\n  b: 1\r\n  c: 3\r\n");
        let t = "a:\n  b: 1";
        assert_eq!(apply(t, &[set("a.b", int(2))]).unwrap(), "a:\n  b: 2");
    }

    #[test]
    fn strings_are_quoted_when_plain_would_change_the_meaning() {
        for (s, want) in [
            ("3h", "3h"),
            ("true", "\"true\""),
            ("Yes", "\"Yes\""),
            ("null", "\"null\""),
            ("12", "\"12\""),
            ("1.5", "\"1.5\""),
            ("1_000", "\"1_000\""),
            ("0x1f", "\"0x1f\""),
            ("", "\"\""),
            ("a b", "\"a b\""),
            ("a: b", "\"a: b\""),
            ("x # y", "\"x # y\""),
            ("-", "\"-\""),
            ("~/x", "\"~/x\""),
            ("say \"hi\"\n", "\"say \\\"hi\\\"\\n\""),
            ("tab\there", "\"tab\\there\""),
            ("bell\u{7}", "\"bell\\u0007\""),
            ("c:\\dir", "\"c:\\\\dir\""),
            ("/usr/bin", "/usr/bin"),
            (".inf", "\".inf\""),
            ("é", "\"é\""),
        ] {
            assert_eq!(render_string(s), want, "{s:?}");
        }
        assert_eq!(render_key("a-b"), "a-b");
        assert_eq!(render_key("a b"), "\"a b\"");
        assert_eq!(render_key("-x"), "\"-x\"");
    }

    #[test]
    fn what_cannot_be_edited_is_refused_with_the_path() {
        let cases: Vec<(&str, Edit)> = vec![
            ("a: |\n  text\nb: 1\n", set("a", st("x"))),
            ("a: &anchor 1\n", set("a", st("x"))),
            ("a: [1]\n", set("a", st("x"))),
            ("a:\n  - x\n", set("a", st("x"))),
            ("a: 1\n", set("a.b", st("x"))),
            ("a:\n\tb: 1\n", set("a.b", st("x"))),
            (
                "l:\n  - k: v\n",
                Edit::SetList {
                    path: p("l"),
                    items: vec![],
                },
            ),
            (
                "l:\n  - [x]\n",
                Edit::SetList {
                    path: p("l"),
                    items: vec![],
                },
            ),
            (
                "l:\n  k: v\n",
                Edit::SetList {
                    path: p("l"),
                    items: vec![],
                },
            ),
            (
                "l: 5\n",
                Edit::SetList {
                    path: p("l"),
                    items: vec![],
                },
            ),
            (
                "l:\n  - a\n    - b\n",
                Edit::SetList {
                    path: p("l"),
                    items: vec![],
                },
            ),
            (
                "l: [a]\n  - b\n",
                Edit::SetList {
                    path: p("l"),
                    items: vec![],
                },
            ),
        ];
        for (text, edit) in cases {
            let err = apply(text, std::slice::from_ref(&edit)).unwrap_err();
            assert!(err.to_string().contains("Advanced editor"), "{text:?}: {err}");
        }
        let err = apply("a: 1\n", &[set("a.b", int(1))]).unwrap_err();
        assert_eq!(err.path, "a");
        assert!(apply(
            "a: 1\n",
            &[Edit::Set {
                path: vec![],
                value: int(1)
            }]
        )
        .is_err());
        assert!(apply(
            "a: 1\n",
            &[Edit::Set {
                path: vec!["".into()],
                value: int(1)
            }]
        )
        .is_err());
        assert!(apply("a: 1\n", &[set("a.b.c.d.e.f.g", int(1))]).is_err());
        let big = "#".repeat(MAX_PATCH_BYTES + 1);
        let err = apply(&big, &[set("a", int(1))]).unwrap_err();
        assert!(err.to_string().starts_with("the file cannot be edited by the form"), "{err}");
    }

    #[test]
    fn quoted_keys_comments_and_document_markers_are_understood() {
        let t = "---\n\"a b\":\n  'c d': 1 # x\n  # note\n  e: \"has # inside\" # real\n";
        let out = apply(t, &[set("a b.c d", int(2)), set("a b.e", st("nn"))]).unwrap();
        assert_eq!(out, "---\n\"a b\":\n  'c d': 2 # x\n  # note\n  e: nn # real\n");
        // A quote inside a plain value is not a string start.
        let t = "a: it's # c\n";
        assert_eq!(apply(t, &[set("a", st("z"))]).unwrap(), "a: z # c\n");
        // A key whose line only looks like a comment or a list item is not a key.
        assert_eq!(apply("# a: 1\nb: 2\n", &[set("a", int(3))]).unwrap(), "# a: 1\nb: 2\na: 3\n");
        assert_eq!(apply("- a\n", &[set("x", int(1))]).unwrap(), "- a\nx: 1\n");
        // Windows paths and URLs as plain values.
        let t = "dir: C:\\work # c\nurl: http://x:80/y\n";
        assert_eq!(
            apply(t, &[set("url", st("http://z"))]).unwrap(),
            "dir: C:\\work # c\nurl: \"http://z\"\n"
        );
        // Unterminated quotes are not keys.
        assert_eq!(apply("\"a: 1\nb: 2\n", &[set("b", int(3))]).unwrap(), "\"a: 1\nb: 3\n");
        assert_eq!(apply("'a': 1\n", &[set("a", int(2))]).unwrap(), "'a': 2\n");
        assert_eq!(apply("\"a\" x: 1\n", &[set("a", int(2))]).unwrap(), "\"a\" x: 1\na: 2\n");
    }

    #[test]
    fn splitting_values_from_comments() {
        assert_eq!(split_comment(" 3h # c"), ("3h", " # c"));
        assert_eq!(split_comment("\"a # b\""), ("\"a # b\"", ""));
        assert_eq!(split_comment("'a # b' # c"), ("'a # b'", " # c"));
        assert_eq!(split_comment("a#b"), ("a#b", ""));
        assert_eq!(split_comment("# only"), ("", "# only"));
        assert_eq!(split_comment("\"a\\\" # b\" # c"), ("\"a\\\" # b\"", " # c"));
        assert_eq!(split_comment("[\"a # b\", c] # z"), ("[\"a # b\", c]", " # z"));
        assert_eq!(
            parse_flow_items("a, 'b', \"c\\\"d\""),
            Some(vec!["a".into(), "b".into(), "c\"d".into()])
        );
        assert_eq!(parse_flow_items("{a}"), None);
        assert_eq!(parse_flow_items("\"a\\"), None);
    }

    #[test]
    fn escapes_in_quoted_text_round_trip() {
        assert_eq!(render_string("a\rb"), "\"a\\rb\"");
        assert_eq!(unquote("\"a\\nb\\tc\\rd\\0e\\\\f\""), "a\nb\tc\rd\0e\\f");
        assert_eq!(unquote("\"trailing\\\""), "trailing");
        assert_eq!(unquote("\"\\u00e9\\uZZZZ\""), "é\u{fffd}");
        assert_eq!(unquote("'it''s'"), "it's");
        let t = "\"a\\\"b\": 1\n'c''d': 2\n";
        assert_eq!(
            apply(t, &[set("a\"b", int(5)), set("c'd", int(6))]).unwrap(),
            "\"a\\\"b\": 5\n'c''d': 6\n"
        );
    }

    #[test]
    fn nulls_and_odd_shapes_are_handled() {
        assert_eq!(
            apply(
                "l: ~\nz: 1\n",
                &[Edit::SetList {
                    path: p("l"),
                    items: vec!["a".into()]
                }]
            )
            .unwrap(),
            "l:\n  - a\nz: 1\n"
        );
        assert!(apply(
            "e:\n  k: v\n    more\n",
            &[Edit::SetMap {
                path: p("e"),
                entries: vec![]
            }]
        )
        .is_err());
        assert_eq!(line_of("a:\n  b: 1\n", &p("a.b")), Some(1));
        assert_eq!(line_of("a: 1\n", &p("x")), None);
        assert_eq!(line_of("a:\n\tb: 1\n", &p("a")), None);
        assert!(apply("a: 1\n", &[Edit::Remove { path: p("a.b") }]).is_err());
        assert!(parse_flow_items("'open").is_none());
        let doc = Doc::parse("a: 1\n").unwrap();
        assert!(matches!(doc.locate(&[]), Ok(Err((None, 0)))));
    }

    #[test]
    fn several_edits_apply_in_order() {
        let out = apply(
            "a:\n  b: 1\n",
            &[
                set("a.b", int(2)),
                set("a.c", int(3)),
                Edit::Remove { path: p("a.b") },
                set("z", Scalar::Bool(false)),
            ],
        )
        .unwrap();
        assert_eq!(out, "a:\n  c: 3\nz: false\n");
        assert_eq!(apply("", &[]).unwrap(), "");
        assert_eq!(apply("", &[set("a", int(1))]).unwrap(), "a: 1\n");
    }
}

#[cfg(test)]
mod properties {
    use super::*;
    use proptest::prelude::*;
    use serde_norway::Value;

    /// A generated document: sections of `key: value` lines, some with comments around them.
    type Entry = (String, String, Option<String>, Option<String>);

    #[derive(Debug, Clone)]
    struct Gen {
        sections: Vec<(String, Vec<Entry>)>,
    }

    fn word() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,6}"
    }

    fn gen() -> impl Strategy<Value = Gen> {
        let entry = (
            word(),
            "[a-z0-9]{1,6}",
            proptest::option::of("[a-z ]{1,10}"),
            proptest::option::of("[a-z ]{1,10}"),
        );
        let section = (word(), proptest::collection::vec(entry, 1..5));
        proptest::collection::vec(section, 1..4).prop_map(|mut sections| {
            // Unique section names and unique keys inside each.
            let mut seen = std::collections::HashSet::new();
            sections.retain(|(n, _)| seen.insert(n.clone()));
            for (_, entries) in &mut sections {
                let mut keys = std::collections::HashSet::new();
                entries.retain(|(k, ..)| keys.insert(k.clone()));
            }
            Gen { sections }
        })
    }

    fn render(g: &Gen) -> String {
        let mut out = String::from("# header\n");
        for (name, entries) in &g.sections {
            out.push_str(&format!("{name}:\n"));
            for (k, v, trailing, above) in entries {
                if let Some(c) = above {
                    out.push_str(&format!("  # {c}\n"));
                }
                out.push_str(&format!("  {k}: {v}"));
                if let Some(c) = trailing {
                    out.push_str(&format!(" # {c}"));
                }
                out.push('\n');
            }
            out.push_str("\n# end of section\n");
        }
        out
    }

    fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
        path.iter().try_fold(v, |v, k| v.as_mapping()?.get(*k))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        #[test]
        fn setting_a_value_changes_only_that_value_and_keeps_every_comment(g in gen(), pick in any::<prop::sample::Index>(), new in "[a-zA-Z0-9 :#_./-]{0,12}") {
            let text = render(&g);
            let (sname, entries) = &g.sections[pick.index(g.sections.len())];
            let (key, original, ..) = &entries[pick.index(entries.len())];
            let edit = Edit::Set { path: vec![sname.clone(), key.clone()], value: Scalar::Str(new.clone()) };
            let out = apply(&text, std::slice::from_ref(&edit)).unwrap();
            let before: Value = serde_norway::from_str(&text).unwrap();
            let after: Value = serde_norway::from_str(&out).unwrap();
            // The new value reads back as given.
            let got = at(&after, &[sname, key]).unwrap();
            let same_text = original == &new;
            if !same_text {
                prop_assert_eq!(got.as_str(), Some(new.as_str()));
            }
            // Everything else is untouched.
            for (n2, es) in &g.sections {
                for (k2, ..) in es {
                    if (n2, k2) != (sname, key) {
                        prop_assert_eq!(at(&before, &[n2, k2]), at(&after, &[n2, k2]));
                    }
                }
            }
            // Every comment line survives.
            let lines: Vec<&str> = out.lines().collect();
            for l in text.lines().filter(|l| l.trim_start().starts_with('#')) {
                prop_assert!(lines.contains(&l), "lost {l:?}");
            }
            // Idempotent.
            prop_assert_eq!(apply(&out, std::slice::from_ref(&edit)).unwrap(), out.clone());
            // Setting the value it already has is a no-op.
            prop_assert_eq!(apply(&text, &[Edit::Set { path: vec![sname.clone(), key.clone()], value: Scalar::Str(original.clone()) }]).unwrap(), text.clone());
        }

        #[test]
        fn a_new_key_appears_in_its_section_and_nothing_else_moves(g in gen(), pick in any::<prop::sample::Index>(), n in 0i64..100_000) {
            let text = render(&g);
            let (sname, _) = &g.sections[pick.index(g.sections.len())];
            let edit = Edit::Set { path: vec![sname.clone(), "zz_new".into()], value: Scalar::Int(n) };
            let out = apply(&text, std::slice::from_ref(&edit)).unwrap();
            let before: Value = serde_norway::from_str(&text).unwrap();
            let after: Value = serde_norway::from_str(&out).unwrap();
            prop_assert_eq!(at(&after, &[sname, "zz_new"]).and_then(Value::as_i64), Some(n));
            for (n2, es) in &g.sections {
                for (k2, ..) in es {
                    prop_assert_eq!(at(&before, &[n2, k2]), at(&after, &[n2, k2]));
                }
            }
            // Removing it again gives the original text back.
            let back = apply(&out, &[Edit::Remove { path: vec![sname.clone(), "zz_new".into()] }]).unwrap();
            prop_assert_eq!(back, text);
        }

        #[test]
        fn lists_round_trip_and_are_idempotent(g in gen(), items in proptest::collection::vec("[a-z0-9:/.-]{1,10}", 0..6), second in proptest::collection::vec("[a-z0-9:/.-]{1,10}", 0..6)) {
            let text = render(&g);
            let sname = &g.sections[0].0;
            let path = vec![sname.clone(), "zz_list".to_string()];
            let first = apply(&text, &[Edit::SetList { path: path.clone(), items: items.clone() }]).unwrap();
            let v: Value = serde_norway::from_str(&first).unwrap();
            let read = |v: &Value| -> Vec<String> {
                at(v, &[sname, "zz_list"]).and_then(Value::as_sequence).map(|s| s.iter().map(|i| match i { Value::String(s) => s.clone(), other => serde_norway::to_string(other).unwrap().trim().to_string() }).collect()).unwrap_or_default()
            };
            prop_assert_eq!(read(&v), items.clone());
            prop_assert_eq!(apply(&first, &[Edit::SetList { path: path.clone(), items: items.clone() }]).unwrap(), first.clone());
            // Changing the list to another one reads back too, and comments stay.
            let next = apply(&first, &[Edit::SetList { path: path.clone(), items: second.clone() }]).unwrap();
            let v2: Value = serde_norway::from_str(&next).unwrap();
            prop_assert_eq!(read(&v2), second);
            prop_assert!(next.contains("# header") && next.contains("# end of section"));
        }

        #[test]
        fn maps_round_trip(g in gen(), entries in proptest::collection::btree_map("[A-Z][A-Z0-9_]{0,6}", "[a-z0-9 ]{0,8}", 0..5)) {
            let text = render(&g);
            let sname = &g.sections[0].0;
            let path = vec![sname.clone(), "zz_map".to_string()];
            let list: Vec<(String, String)> = entries.clone().into_iter().collect();
            let out = apply(&text, &[Edit::SetMap { path: path.clone(), entries: list.clone() }]).unwrap();
            let v: Value = serde_norway::from_str(&out).unwrap();
            if entries.is_empty() {
                prop_assert!(at(&v, &[sname, "zz_map"]).is_none_or(|m| m.as_mapping().is_some_and(|m| m.is_empty())));
            } else {
                let got: std::collections::BTreeMap<String, String> = at(&v, &[sname, "zz_map"]).unwrap().as_mapping().unwrap().iter()
                    .map(|(k, v)| (k.as_str().unwrap().to_string(), v.as_str().unwrap_or("").to_string())).collect();
                prop_assert_eq!(got, entries);
            }
            prop_assert_eq!(apply(&out, &[Edit::SetMap { path, entries: list }]).unwrap(), out.clone());
        }

        #[test]
        fn arbitrary_text_never_panics(text in "(?s).{0,400}", pathlen in 1usize..4, k in "[a-z]{1,4}") {
            let path: Vec<String> = (0..pathlen).map(|i| format!("{k}{i}")).collect();
            for e in [
                Edit::Set { path: path.clone(), value: Scalar::Str("x y".into()) },
                Edit::SetList { path: path.clone(), items: vec!["a".into(), "b".into()] },
                Edit::SetMap { path: path.clone(), entries: vec![("K".into(), "v".into())] },
                Edit::Remove { path: path.clone() },
            ] {
                let _ = apply(&text, &[e]);
            }
            let _ = line_of(&text, &path);
        }

        #[test]
        fn structured_noise_never_panics(lines in proptest::collection::vec("[ -~]{0,30}", 0..12), k in "[a-z]{1,3}") {
            let text = lines.join("\n");
            let path = vec![k.clone(), "x".to_string()];
            let _ = apply(&text, &[Edit::Set { path: path.clone(), value: Scalar::Int(1) }, Edit::SetList { path: path.clone(), items: vec!["i".into()] }, Edit::SetMap { path: path.clone(), entries: vec![("A".into(), "b".into())] }, Edit::Remove { path }]);
        }
    }
}
