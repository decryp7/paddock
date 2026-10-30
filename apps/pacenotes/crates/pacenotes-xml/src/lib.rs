#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! The one safe way Pacenotes reads untrusted XML (ADR 0010): coverage reports (`pacenotes-cover`), requirement documents
//! (`pacenotes-req`) and anything else that parses a document somebody else wrote.
//!
//! Three rules, applied before the parser sees a byte, in [`clean`]:
//!
//! 1. **No internal subset.** A `DOCTYPE` with a `[` can declare entities, and a few of them expand a kilobyte into gigabytes; it is
//!    refused. A bare `DOCTYPE` (JaCoCo's `<!DOCTYPE report PUBLIC "..." "report.dtd">` names an external DTD) declares nothing the
//!    parser would use, so it is blanked with spaces (same length, same lines) and nothing is ever fetched.
//! 2. **Bounded nesting.** The parser recurses as deep as the document nests. A scan that understands quoted attribute values
//!    (`<a x="/>">` is one tag) counts the depth and refuses a document past [`XmlLimits::max_depth`].
//! 3. **Bounded size of the tree**, through the parser's own node limit, and `allow_dtd` off as a second line of defence.
//!
//! Errors say where: a line (and column) of the cleaned text, which has the same lines as the original.

use std::borrow::Cow;

pub use roxmltree;

/// Bounds of one parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XmlLimits {
    /// The deepest element nesting accepted.
    pub max_depth: usize,
    /// The most nodes the parser may build.
    pub max_nodes: u32,
}

impl Default for XmlLimits {
    fn default() -> Self {
        XmlLimits {
            max_depth: 256,
            max_nodes: 5_000_000,
        }
    }
}

/// Why a document was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XmlError {
    /// A `DOCTYPE` with an internal subset (or one the parser would still see).
    #[error(
        "{part}: the document has a DOCTYPE with declarations (line {line}), which can expand without bound or name outside files; it was refused and nothing was loaded"
    )]
    Dtd {
        /// The part or file.
        part: String,
        /// 1-based line.
        line: usize,
    },
    /// Nesting past the limit.
    #[error("{part}: the elements nest deeper than {max} levels (line {line}); it was refused before parsing")]
    Depth {
        /// The part or file.
        part: String,
        /// The limit.
        max: usize,
        /// 1-based line where the limit was passed.
        line: usize,
    },
    /// More nodes than the limit.
    #[error("{part}: more than {max} XML nodes; split the document or raise the limit")]
    Nodes {
        /// The part or file.
        part: String,
        /// The limit.
        max: u32,
    },
    /// Not well-formed.
    #[error("{part}: not well-formed XML ({message})")]
    Syntax {
        /// The part or file.
        part: String,
        /// The parser's words, which include the line and column.
        message: String,
    },
}

fn line_at(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())].iter().filter(|b| **b == b'\n').count() + 1
}

/// The index just past the `>` that closes the tag starting at `start`, skipping quoted attribute values; `None` if it never closes.
fn tag_end(b: &[u8], start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut i = start;
    while i < b.len() {
        match (quote, b[i]) {
            (Some(q), c) if c == q => quote = None,
            (None, b'"' | b'\'') => quote = Some(b[i]),
            (None, b'>') => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

fn find(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    b.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| from + p)
}

/// Applies rules 1 and 2. The result has the same bytes as `text` except that a bare `DOCTYPE` is spaces.
pub fn clean<'a>(text: &'a str, part: &str, limits: &XmlLimits) -> Result<Cow<'a, str>, XmlError> {
    let b = text.as_bytes();
    let (mut i, mut depth) = (0usize, 0usize);
    let mut blank: Option<(usize, usize)> = None;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &b[i..];
        if rest.starts_with(b"<!--") {
            i = find(b, i + 4, b"-->").map_or(b.len(), |e| e + 3);
        } else if rest.starts_with(b"<![CDATA[") {
            i = find(b, i + 9, b"]]>").map_or(b.len(), |e| e + 3);
        } else if rest.starts_with(b"<?") {
            i = find(b, i + 2, b"?>").map_or(b.len(), |e| e + 2);
        } else if rest.starts_with(b"<!DOCTYPE") || rest.starts_with(b"<!doctype") {
            // up to the closing `>`, outside quotes; a `[` outside quotes opens an internal subset
            let (mut q, mut j, mut subset) = (None::<u8>, i + 9, false);
            while j < b.len() {
                match (q, b[j]) {
                    (Some(c), x) if x == c => q = None,
                    (None, b'"' | b'\'') => q = Some(b[j]),
                    (None, b'[') => {
                        subset = true;
                        break;
                    }
                    (None, b'>') => break,
                    _ => {}
                }
                j += 1;
            }
            if subset || j >= b.len() {
                return Err(XmlError::Dtd {
                    part: part.to_owned(),
                    line: line_at(text, i),
                });
            }
            blank = Some((i, j + 1));
            i = j + 1;
        } else if rest.starts_with(b"<!") {
            i = find(b, i + 2, b">").map_or(b.len(), |e| e + 1);
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            i = find(b, i + 2, b">").map_or(b.len(), |e| e + 1);
        } else {
            let end = tag_end(b, i + 1).unwrap_or(b.len());
            let self_closing = end >= 2 && b.get(end - 2) == Some(&b'/');
            if !self_closing {
                depth += 1;
                if depth > limits.max_depth {
                    return Err(XmlError::Depth {
                        part: part.to_owned(),
                        max: limits.max_depth,
                        line: line_at(text, i),
                    });
                }
            }
            i = end;
        }
    }
    Ok(match blank {
        None => Cow::Borrowed(text),
        Some((from, to)) => {
            let mut s = String::with_capacity(text.len());
            s.push_str(&text[..from]);
            s.extend(text[from..to].chars().map(|c| if c == '\n' { '\n' } else { ' ' }));
            s.push_str(&text[to..]);
            Cow::Owned(s)
        }
    })
}

/// Parses text that [`clean`] returned. `allow_dtd` is off, so a `DOCTYPE` that somehow remained is refused here too.
pub fn parse<'a>(cleaned: &'a str, part: &str, limits: &XmlLimits) -> Result<roxmltree::Document<'a>, XmlError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: limits.max_nodes,
    };
    roxmltree::Document::parse_with_options(cleaned, options).map_err(|e| match e {
        roxmltree::Error::DtdDetected => XmlError::Dtd {
            part: part.to_owned(),
            line: 1,
        },
        roxmltree::Error::NodesLimitReached => XmlError::Nodes {
            part: part.to_owned(),
            max: limits.max_nodes,
        },
        other => XmlError::Syntax {
            part: part.to_owned(),
            message: other.to_string(),
        },
    })
}

/// The 1-based line of a node, for error messages about content (`line 40: a <line> needs a number`).
pub fn line_of(doc: &roxmltree::Document<'_>, node: roxmltree::Node<'_, '_>) -> u32 {
    doc.text_pos_at(node.range().start).row
}

/// [`clean`] then [`parse`] in one call, for callers that hold the text for the whole parse and give the cleaned text to `f` (the
/// document borrows it).
pub fn with_document<T, E: From<XmlError>>(
    text: &str,
    part: &str,
    limits: &XmlLimits,
    f: impl FnOnce(&roxmltree::Document<'_>) -> Result<T, E>,
) -> Result<T, E> {
    let cleaned = clean(text, part, limits)?;
    let doc = parse(&cleaned, part, limits)?;
    f(&doc)
}
