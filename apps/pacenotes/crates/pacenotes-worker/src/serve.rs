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
    hooks(&req.path);
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

/// Behaviours for the crash tests, chosen by the file name's suffix. Only in builds with the `test-hooks` feature.
#[cfg(feature = "test-hooks")]
fn hooks(path: &str) {
    #[allow(unconditional_recursion)]
    fn recurse(n: u64) -> u64 {
        let pad = [n as u8; 4096];
        std::hint::black_box(&pad);
        recurse(n + 1) + u64::from(pad[0])
    }
    if path.ends_with(".abort") {
        std::process::abort();
    } else if path.ends_with(".exit") {
        std::process::exit(7);
    } else if path.ends_with(".spin") {
        loop {
            std::hint::black_box(0u64);
        }
    } else if path.ends_with(".block") {
        loop {
            std::thread::park();
        }
    } else if path.ends_with(".overflow") {
        std::hint::black_box(recurse(0));
    }
}
