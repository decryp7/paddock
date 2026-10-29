//! The pure decoding of a language server's reply (`Location[]`), so the adapter that talks to the process stays thin and
//! this part is tested without one. A malformed or oversized reply is [`LspError::BadReply`], never a panic.
//!
//! Known limit: LSP columns are UTF-16 code units; here they are taken as columns as they come. The index converts them
//! against the file text when it caches the answer (A-lsp).

use serde::Deserialize;

use crate::location::{Location, Position, Span};
use crate::path::RepoPath;
use crate::ports::{LspError, LspReply};

/// The largest reply text decoded.
pub const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct WireLocation {
    uri: String,
    range: WireRange,
}

#[derive(Deserialize)]
struct WireRange {
    start: WirePosition,
    end: WirePosition,
}

#[derive(Deserialize)]
struct WirePosition {
    line: u32,
    character: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Reply {
    Many(Vec<WireLocation>),
    One(WireLocation),
}

fn bad(why: impl std::fmt::Display) -> LspError {
    LspError::BadReply(why.to_string())
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn position(p: &WirePosition) -> Result<Position, LspError> {
    let line = p.line.checked_add(1).ok_or_else(|| bad("line out of range"))?;
    let column = p.character.checked_add(1).ok_or_else(|| bad("column out of range"))?;
    Position::new(line, column).ok_or_else(|| bad("position out of range"))
}

/// A `file:///` URI with its percent escapes decoded and a Windows drive letter normalised (`file:///c%3A/x` and
/// `file:///C:/x` are one place). The result is for comparing and slicing, not for opening.
fn normal_uri(uri: &str) -> Option<(String, bool)> {
    let decoded = percent_decode(uri)?;
    let drive = decoded
        .strip_prefix("file:///")
        .is_some_and(|r| r.len() >= 2 && r.as_bytes()[0].is_ascii_alphabetic() && r.as_bytes()[1] == b':');
    Some((decoded, drive))
}

/// Decodes the JSON `result` of a definition or references request. `root_uri` is the workspace root as the server was told
/// (`file:///work/repo`). `null` is an empty reply. A location outside the workspace (the standard library, a dependency)
/// is dropped and counted in [`LspReply::outside_workspace`]; it does not fail the reply. A location inside the
/// workspace that is malformed (a `..` segment, a bad escape, a reversed range) does.
pub fn decode_locations(root_uri: &str, json: &str) -> Result<LspReply, LspError> {
    if json.len() > MAX_REPLY_BYTES {
        return Err(bad(format!(
            "the reply is {} bytes; at most {MAX_REPLY_BYTES} are decoded",
            json.len()
        )));
    }
    let reply: Option<Reply> = serde_json::from_str(json).map_err(|e| bad(e.classify_name()))?;
    let wire = match reply {
        None => Vec::new(),
        Some(Reply::Many(v)) => v,
        Some(Reply::One(l)) => vec![l],
    };
    let (root, root_drive) =
        normal_uri(root_uri.trim_end_matches('/')).ok_or_else(|| bad("the workspace root has a bad percent escape"))?;
    let fold = |s: &str| if root_drive { s.to_ascii_lowercase() } else { s.to_owned() };
    let prefix = fold(&format!("{root}/"));
    let mut out = Vec::with_capacity(wire.len().min(crate::ports::MAX_LSP_LOCATIONS + 1));
    let mut outside = 0u32;
    for l in &wire {
        let (uri, _) = normal_uri(&l.uri).ok_or_else(|| bad("a location has a bad percent escape"))?;
        // `get` rather than a slice: the prefix length is in bytes and a multi-byte character could straddle it
        let inside = uri.get(..prefix.len()).is_some_and(|head| fold(head) == prefix);
        if !inside {
            outside = outside.saturating_add(1);
            continue;
        }
        let path = RepoPath::parse(&uri[prefix.len()..]).map_err(bad)?;
        let span = Span::new(position(&l.range.start)?, position(&l.range.end)?).map_err(bad)?;
        out.push(Location { path, span });
        if out.len() > crate::ports::MAX_LSP_LOCATIONS {
            break;
        }
    }
    Ok(LspReply::with_outside(out, outside))
}

/// A short category for a JSON error, without echoing the (possibly source-bearing) text.
trait ClassifyName {
    fn classify_name(&self) -> &'static str;
}

impl ClassifyName for serde_json::Error {
    fn classify_name(&self) -> &'static str {
        match self.classify() {
            serde_json::error::Category::Syntax => "the reply is not valid JSON",
            serde_json::error::Category::Data => "the reply is not a location or a list of locations",
            serde_json::error::Category::Io | serde_json::error::Category::Eof => "the reply is cut short",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "file:///work/repo";

    fn loc(uri: &str) -> String {
        format!(r#"{{"uri":"{uri}","range":{{"start":{{"line":0,"character":4}},"end":{{"line":1,"character":0}}}}}}"#)
    }

    #[test]
    fn a_list_a_single_location_and_null_decode() {
        let list = decode_locations(
            ROOT,
            &format!("[{},{}]", loc("file:///work/repo/src/a.rs"), loc("file:///work/repo/b%20c.rs")),
        )
        .unwrap();
        assert_eq!(list.locations().len(), 2);
        assert_eq!(list.locations()[0].path.as_str(), "src/a.rs");
        assert_eq!(list.locations()[1].path.as_str(), "b c.rs");
        assert_eq!(
            (list.locations()[0].span.start().line(), list.locations()[0].span.start().column()),
            (1, 5)
        );
        assert_eq!(decode_locations(ROOT, &loc("file:///work/repo/a.rs")).unwrap().locations().len(), 1);
        assert_eq!(decode_locations(ROOT, "null").unwrap(), LspReply::default());
        assert_eq!(decode_locations("file:///work/repo/", "[]").unwrap(), LspReply::default());
    }

    #[test]
    fn bad_replies_are_errors_not_panics() {
        for (json, needle) in [
            ("{", "cut short"),
            ("[1]", "not a location"),
            ("nonsense", "not valid JSON"),
            (&format!("[{}]", loc("file:///work/repo/../x.rs")), "segment"),
            (&format!("[{}]", loc("file:///work/repo/a%zz.rs")), "percent"),
            (&format!("[{}]", loc("file:///work/repo/a%ff.rs")), "percent"),
            (
                r#"[{"uri":"file:///work/repo/a.rs","range":{"start":{"line":4294967295,"character":0},"end":{"line":0,"character":0}}}]"#,
                "out of range",
            ),
            (
                r#"[{"uri":"file:///work/repo/a.rs","range":{"start":{"line":0,"character":4294967295},"end":{"line":0,"character":0}}}]"#,
                "out of range",
            ),
            (
                r#"[{"uri":"file:///work/repo/a.rs","range":{"start":{"line":5,"character":0},"end":{"line":1,"character":0}}}]"#,
                "ends before",
            ),
        ] {
            let err = decode_locations(ROOT, json).unwrap_err();
            assert!(matches!(&err, LspError::BadReply(m) if m.contains(needle)), "{json}: {err:?}");
        }
        assert!(matches!(
            decode_locations(ROOT, &" ".repeat(MAX_REPLY_BYTES + 1)),
            Err(LspError::BadReply(_))
        ));
    }

    #[test]
    fn locations_outside_the_workspace_are_dropped_and_counted_not_fatal() {
        let json = format!(
            "[{},{},{}]",
            loc("file:///usr/lib/rustlib/src/lib.rs"),
            loc("file:///work/repo/src/a.rs"),
            loc("file:///work/repo-other/a.rs")
        );
        let r = decode_locations(ROOT, &json).unwrap();
        assert_eq!((r.locations().len(), r.outside_workspace()), (1, 2));
        let only_outside = decode_locations(ROOT, &format!("[{}]", loc("https://example.invalid/x"))).unwrap();
        assert_eq!((only_outside.locations().len(), only_outside.outside_workspace()), (0, 1));
    }

    #[test]
    fn windows_drive_letters_and_their_encodings_are_one_place() {
        for root in ["file:///c%3A/work/repo", "file:///C:/work/repo", "file:///c:/work/repo/"] {
            for uri in [
                "file:///c%3A/work/repo/src/a.rs",
                "file:///C:/work/repo/src/a.rs",
                "file:///c:/Work/Repo/src/a.rs",
            ] {
                let r = decode_locations(root, &format!("[{}]", loc(uri))).unwrap();
                assert_eq!((r.locations().len(), r.outside_workspace()), (1, 0), "{root} {uri}");
                assert_eq!(r.locations()[0].path.as_str().to_ascii_lowercase(), "src/a.rs");
            }
        }
        let other_drive = decode_locations("file:///c%3A/work/repo", &format!("[{}]", loc("file:///d%3A/work/repo/a.rs"))).unwrap();
        assert_eq!(other_drive.outside_workspace(), 1);
        assert!(decode_locations("file:///c%3A/%zz", "[]").is_err());
        // a multi-byte character where the root prefix ends does not panic
        let r = decode_locations(ROOT, &format!("[{}]", loc("file:///work/rep\u{00e9}/a.rs"))).unwrap();
        assert_eq!(r.outside_workspace(), 1);
    }

    #[test]
    fn a_reply_with_too_many_locations_is_cut_and_says_so() {
        let one = loc("file:///work/repo/a.rs");
        let json = format!("[{}]", vec![one; crate::ports::MAX_LSP_LOCATIONS + 5].join(","));
        let r = decode_locations(ROOT, &json).unwrap();
        assert_eq!((r.locations().len(), r.truncated()), (crate::ports::MAX_LSP_LOCATIONS, true));
    }

    #[test]
    fn decoding_never_panics_on_hostile_input() {
        let seed = format!("[{}]", loc("file:///work/repo/src/a.rs"));
        paddock_testkit::assert_never_panics(&[seed.as_bytes()], |b| decode_locations(ROOT, &String::from_utf8_lossy(b)).is_ok());
    }
}
