//! The small JSON the stand-in's script and record use. Std only, because the stand-in binary (`src/bin/paddock-stand-in.rs`)
//! includes this file with `#[path]` and must stay a few hundred lines of std; the builder (`stand_in.rs`) uses the same file, so
//! the writer and the reader cannot drift apart. Numbers are whole (`i64`): the script has no other kind.

#![allow(dead_code)] // the stand-in binary and the builder each use part of it

use std::fmt::Write as _;

/// A parsed JSON value. Objects keep their member order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(i64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

/// FNV-1a over the stand-in's two sources: a name and a version check that change when either does. The binary prints it (with
/// `PADDOCK_STAND_IN_PRINT_HASH` set) so the builder can tell a prebuilt binary from another version of the sources.
pub fn source_hash(stub: &str, json: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in stub.bytes().chain([0]).chain(json.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Nesting deeper than this is refused (a script is two or three levels deep).
const MAX_DEPTH: usize = 32;

impl Value {
    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Obj(m) => Some(m),
            _ => None,
        }
    }

    /// Compact text, one line (a record is one line per call).
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => {
                let _ = write!(out, "{n}");
            }
            Value::Str(s) => write_str(s, out),
            Value::Arr(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses one JSON document.
///
/// # Errors
///
/// A message with the byte offset, for malformed text, nesting over [`MAX_DEPTH`] or text after the value.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        at: 0,
    };
    let v = p.value(0)?;
    p.ws();
    if p.at != p.b.len() {
        return Err(p.err("text after the value"));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("JSON error at byte {}: {what}", self.at)
    }

    fn ws(&mut self) {
        while matches!(self.b.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.b.get(self.at) == Some(&c) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.err(&format!("expected `{}`", c as char)))
        }
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, String> {
        if self.b[self.at..].starts_with(w.as_bytes()) {
            self.at += w.len();
            Ok(v)
        } else {
            Err(self.err("not a JSON value"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err(self.err("nested too deep"));
        }
        self.ws();
        match self.b.get(self.at) {
            None => Err(self.err("the text ends early")),
            Some(b'n') => self.word("null", Value::Null),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.ws();
                if self.b.get(self.at) == Some(&b']') {
                    self.at += 1;
                    return Ok(Value::Arr(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Value::Arr(items));
                        }
                        _ => return Err(self.err("expected `,` or `]`")),
                    }
                }
            }
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                self.ws();
                if self.b.get(self.at) == Some(&b'}') {
                    self.at += 1;
                    return Ok(Value::Obj(members));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(b':')?;
                    members.push((k, self.value(depth + 1)?));
                    self.ws();
                    match self.b.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Value::Obj(members));
                        }
                        _ => return Err(self.err("expected `,` or `}`")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.at;
                self.at += 1;
                while matches!(self.b.get(self.at), Some(b'0'..=b'9')) {
                    self.at += 1;
                }
                // The slice is ASCII digits and a sign, so it is valid UTF-8.
                std::str::from_utf8(&self.b[start..self.at])
                    .ok()
                    .and_then(|t| t.parse().ok())
                    .map(Value::Num)
                    .ok_or_else(|| self.err("not a whole number"))
            }
            Some(_) => Err(self.err("not a JSON value")),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self
            .b
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.err("short \\u escape"))?;
        let h = std::str::from_utf8(h).map_err(|_| self.err("bad \\u escape"))?;
        let n = u32::from_str_radix(h, 16).map_err(|_| self.err("bad \\u escape"))?;
        self.at += 4;
        Ok(n)
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.b.get(self.at), None | Some(b'"' | b'\\')) {
                self.at += 1;
            }
            // Cut at ASCII bytes of valid UTF-8 input, so the piece is valid UTF-8 too.
            out.push_str(
                std::str::from_utf8(&self.b[start..self.at]).map_err(|_| self.err("not UTF-8"))?,
            );
            match self.b.get(self.at) {
                None => return Err(self.err("the string does not end")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(_) => {
                    self.at += 1;
                    let e = *self
                        .b
                        .get(self.at)
                        .ok_or_else(|| self.err("the escape does not end"))?;
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'u' => {
                            let mut n = self.hex4()?;
                            if (0xD800..0xDC00).contains(&n)
                                && self.b[self.at..].starts_with(b"\\u")
                            {
                                self.at += 2;
                                let low = self.hex4()?;
                                n = 0x10000 + ((n - 0xD800) << 10) + low.wrapping_sub(0xDC00);
                            }
                            out.push(char::from_u32(n).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.err("unknown escape")),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_text_with_quotes_newlines_controls_and_non_ascii() {
        let v = Value::Arr(vec![
            Value::Str("a \"b\" \\ c\nd\te\u{1}f naïve 日本 😀".into()),
            Value::Num(-7),
            Value::Null,
            Value::Bool(true),
        ]);
        assert_eq!(parse(&v.to_json()), Ok(v));
    }

    #[test]
    fn reads_escapes_and_surrogate_pairs() {
        assert_eq!(
            parse(r#""\u00e9\ud83d\ude00\/""#),
            Ok(Value::Str("é😀/".into()))
        );
    }

    #[test]
    fn refuses_malformed_deep_and_trailing_text() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "\"abc",
            "1 2",
            "tru",
            "\"\\x\"",
            "1.5",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "[".repeat(40) + &"]".repeat(40);
        assert!(parse(&deep).unwrap_err().contains("too deep"));
    }

    #[test]
    fn objects_keep_their_order_and_answer_get() {
        let v = parse(r#"{"b":1,"a":"x"}"#).unwrap();
        assert_eq!(v.as_obj().unwrap()[0].0, "b");
        assert_eq!(v.get("a").and_then(Value::as_str), Some("x"));
        assert_eq!(v.get("b").and_then(Value::as_i64), Some(1));
    }
}
