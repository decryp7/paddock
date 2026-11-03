//! The wire protocol: one request, one reply (postcard, through `paddock-ipc`).

use pacenotes_core::{Span, SymbolKind};
use paddock_ipc::ProtocolVersion;
use serde::{Deserialize, Serialize};

/// The protocol version. A new major is a different protocol; a new minor may add reply variants at the end.
pub const PROTOCOL: ProtocolVersion = ProtocolVersion::new(1, 0);

/// Parse one file. The host reads the file (through the denylist and the workspace checks) and sends its bytes: the worker has
/// no file access to give a hostile file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseRequest {
    /// The workspace-relative path, for the language and for messages.
    pub path: String,
    /// The file's content.
    pub bytes: Vec<u8>,
    /// The walk's node limit.
    pub max_nodes: u32,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseReply {
    /// The facts, or `None` for a file in no language the worker parses.
    pub facts: Option<WireFacts>,
}

/// The facts of one file, as they cross the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFacts {
    /// The language id.
    pub language: String,
    /// Symbols in source order.
    pub symbols: Vec<WireSymbol>,
    /// Imports.
    pub imports: Vec<WireImport>,
    /// Calls.
    pub calls: Vec<WireCall>,
    /// Chunks.
    pub chunks: Vec<WireChunk>,
    /// The walk stopped at its node limit.
    pub partial: bool,
    /// The file has syntax errors.
    pub has_errors: bool,
}

/// A symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireSymbol {
    /// What it is.
    pub kind: SymbolKind,
    /// Its name.
    pub name: String,
    /// Its qualified name.
    pub qualified_name: String,
    /// Its span.
    pub span: Span,
    /// Containers around it.
    pub depth: u32,
}

/// An import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireImport {
    /// The target.
    pub target: String,
    /// Where.
    pub span: Span,
}

/// A call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCall {
    /// The enclosing callable.
    pub caller: Option<String>,
    /// The called name.
    pub callee: String,
    /// Where.
    pub span: Span,
}

/// A chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireChunk {
    /// Its lines.
    pub span: Span,
    /// The symbol it holds.
    pub symbol: Option<String>,
    /// Its place among the symbol's chunks.
    pub ordinal: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_ipc::wire::{decode, encode};

    #[test]
    fn requests_and_replies_round_trip_through_the_wire_encoding() {
        let req = ParseRequest {
            path: "src/a.rs".into(),
            bytes: b"fn a() {}".to_vec(),
            max_nodes: 500_000,
        };
        assert_eq!(decode::<ParseRequest>(&encode(&req).unwrap()).unwrap(), req);
        let span = Span::lines(1, 1).unwrap();
        let reply = ParseReply {
            facts: Some(WireFacts {
                language: "rust".into(),
                symbols: vec![WireSymbol {
                    kind: SymbolKind::Function,
                    name: "a".into(),
                    qualified_name: "a".into(),
                    span,
                    depth: 0,
                }],
                imports: vec![WireImport {
                    target: "std::fmt".into(),
                    span,
                }],
                calls: vec![WireCall {
                    caller: Some("a".into()),
                    callee: "b".into(),
                    span,
                }],
                chunks: vec![WireChunk {
                    span,
                    symbol: Some("a".into()),
                    ordinal: 0,
                }],
                partial: false,
                has_errors: true,
            }),
        };
        assert_eq!(decode::<ParseReply>(&encode(&reply).unwrap()).unwrap(), reply);
        assert_eq!(
            decode::<ParseReply>(&encode(&ParseReply { facts: None }).unwrap()).unwrap().facts,
            None
        );
    }

    #[test]
    fn hostile_bytes_never_panic_the_decoder() {
        let req = ParseRequest {
            path: "a".into(),
            bytes: vec![1, 2, 3],
            max_nodes: 1,
        };
        let seed = encode(&req).unwrap();
        paddock_testkit::assert_never_panics(&[seed.as_slice()], |b| decode::<ParseRequest>(b).is_ok());
    }
}
