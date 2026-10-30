//! The worker's side: answers [`ParseRequest`]s with the parser library, in this process, so a crash costs this process only.

use std::sync::Arc;

use pacenotes_core::RepoPath;
use pacenotes_parse::{Limits, Outline, ParseError, ParseLanguage, Stop};
use paddock_ipc::{CallCtx, Handler, IpcError, RemoteError, child, codes, typed};

use crate::protocol::{PROTOCOL, ParseReply, ParseRequest, WireCall, WireChunk, WireFacts, WireImport, WireSymbol};

/// The parser's cancel check is the call's: the host cancelling the call stops the parse.
struct CallStop<'a>(&'a CallCtx);

impl Stop for CallStop<'_> {
    fn stopped(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// An outline as wire facts.
pub fn to_wire(outline: &Outline) -> WireFacts {
    WireFacts {
        language: outline.language.id().to_owned(),
        symbols: outline
            .symbols
            .iter()
            .map(|s| WireSymbol {
                kind: s.kind,
                name: s.name.clone(),
                qualified_name: s.qualified_name.clone(),
                span: s.span,
                depth: s.depth,
            })
            .collect(),
        imports: outline
            .imports
            .iter()
            .map(|i| WireImport {
                target: i.target.clone(),
                span: i.span,
            })
            .collect(),
        calls: outline
            .calls
            .iter()
            .map(|c| WireCall {
                caller: c.caller.clone(),
                callee: c.callee.clone(),
                span: c.span,
            })
            .collect(),
        chunks: outline
            .chunks
            .iter()
            .map(|c| WireChunk {
                span: c.span,
                symbol: c.symbol.clone(),
                ordinal: c.ordinal,
            })
            .collect(),
        partial: outline.partial,
        has_errors: outline.has_errors,
    }
}

/// Parses a request. Public so tests can run the worker's logic without a process.
pub fn answer(req: &ParseRequest, stop: &dyn Fn() -> bool) -> Result<ParseReply, RemoteError> {
    struct F<'a>(&'a dyn Fn() -> bool);
    impl Stop for F<'_> {
        fn stopped(&self) -> bool {
            (self.0)()
        }
    }
    let path = RepoPath::parse(&req.path).map_err(|e| RemoteError::new(codes::INVALID, e.to_string()))?;
    #[cfg(feature = "test-hooks")]
    crate::hooks::run(&req.path);
    let Some(language) = ParseLanguage::from_path(&path) else {
        return Ok(ParseReply { facts: None });
    };
    let limits = Limits {
        max_nodes: req.max_nodes as usize,
        ..Limits::default()
    };
    match pacenotes_parse::parse_with(language, &req.bytes, &limits, &F(stop)) {
        Ok(outline) => Ok(ParseReply {
            facts: Some(to_wire(&outline)),
        }),
        Err(ParseError::Cancelled) => Err(CallCtx::cancelled_error()),
        Err(ParseError::TooLarge { len, max }) => Err(RemoteError::new(
            codes::TOO_LARGE,
            format!("the file is {len} bytes; at most {max} are parsed"),
        )),
        Err(e) => Err(RemoteError::new(codes::INTERNAL, e.to_string())),
    }
}

/// The handler of the worker's one call.
pub fn handler() -> impl Handler {
    typed(|ctx: &CallCtx, req: ParseRequest| answer(&req, &|| CallStop(ctx).stopped()))
}

/// The worker's `main` after its early exits: connects to the host named on stdin and serves until the host goes away.
pub fn run() -> Result<(), IpcError> {
    child::serve("pacenotes-parse-worker", PROTOCOL, |_| Arc::new(handler()), |_, _| {})
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn request(path: &str, bytes: &[u8]) -> ParseRequest {
        ParseRequest {
            path: path.to_owned(),
            bytes: bytes.to_vec(),
            max_nodes: pacenotes_parse::DEFAULT_MAX_NODES as u32,
        }
    }

    #[test]
    fn a_file_is_answered_with_its_symbols_imports_calls_and_chunks() {
        let src = b"use std::fmt;\n\npub fn a() { b(); }\n\npub fn b() {}\n";
        let facts = answer(&request("src/lib.rs", src), &|| false).unwrap().facts.unwrap();
        assert_eq!(facts.language, "rust");
        assert_eq!(facts.symbols.len(), 2);
        assert!(facts.imports.iter().any(|i| i.target.contains("std")));
        assert!(facts.calls.iter().any(|c| c.callee == "b" && c.caller.as_deref() == Some("a")));
        assert!(!facts.chunks.is_empty());
    }

    #[test]
    fn a_file_in_no_known_language_has_no_facts_and_a_bad_path_is_refused() {
        assert_eq!(answer(&request("notes.txt", b"x"), &|| false).unwrap(), ParseReply { facts: None });
        let err = answer(&request("../escape.rs", b"x"), &|| false).unwrap_err();
        assert_eq!(err.code, codes::INVALID);
    }

    #[test]
    fn a_stopped_call_is_answered_as_cancelled_and_a_huge_file_as_too_large() {
        let big = vec![b'a'; 64 * 1024 * 1024];
        let err = answer(&request("big.rs", &big), &|| false).unwrap_err();
        assert_eq!(err.code, codes::TOO_LARGE);
        let src = b"pub fn a() {}\n".repeat(200);
        let err = answer(&request("src/lib.rs", &src), &|| true).unwrap_err();
        assert_eq!(err.code, CallCtx::cancelled_error().code);
    }
}
