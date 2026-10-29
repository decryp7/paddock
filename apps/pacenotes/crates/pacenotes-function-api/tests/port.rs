#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The port's contract (plan FN0): every message type round-trips through postcard, and a host call has a budget and a
//! cancel token.

use pacenotes_core::ports::{ChangeKind, ChangeSet, FileChange, ModelId, SymbolRecord};
use pacenotes_core::{
    CommitHash, Confidence, FileId, Fingerprint, Generation, KnownLanguage, Language, RepoPath, Rev, Span, SymbolId, SymbolKind,
};
use pacenotes_function_api::{
    Budget, BudgetKind, Contributions, Function, FunctionError, FunctionReply, FunctionRequest, Host, HostCall, HostError, HostReply,
    HostRequest, SettingSpec, SettingValue, ToolOutput, ToolSpec, ViewKind, ViewSpec, decode, encode,
};
use paddock_jobs::CancelToken;
use serde::{Serialize, de::DeserializeOwned};

fn path(p: &str) -> RepoPath {
    RepoPath::parse(p).unwrap()
}

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
    let bytes = encode(&value).unwrap();
    assert_eq!(decode::<T>(&bytes).unwrap(), value);
}

fn rev() -> Rev {
    Rev::Commit(CommitHash::parse(&"ab".repeat(20)).unwrap())
}

fn symbol() -> SymbolRecord {
    let file = FileId::mint(&path("a.rs"), &Fingerprint::of_content(b"x"));
    SymbolRecord {
        id: SymbolId::derive(file, &Language::Known(KnownLanguage::Rust), SymbolKind::Function, "f", 0),
        file,
        kind: SymbolKind::Function,
        qualified_name: "f".into(),
        span: Span::lines(1, 2).unwrap(),
        confidence: Confidence::Resolved,
    }
}

#[test]
fn every_host_call_round_trips() {
    let file = symbol().file;
    for call in [
        HostCall::FileId { path: path("a/b.rs") },
        HostCall::SymbolsInFile { file },
        HostCall::FindSymbols {
            query: "parse".into(),
            limit: 20,
        },
        HostCall::ReadAt {
            rev: rev(),
            path: path("a.rs"),
            max_bytes: 1 << 20,
        },
        HostCall::ChangedBetween {
            from: rev(),
            to: Rev::WorkingTree(Generation(3)),
            limit: 100,
        },
        HostCall::Complete {
            prompt: "why?".into(),
            max_tokens: 256,
        },
    ] {
        round_trip(HostRequest {
            id: 1,
            call,
            budget: Budget::INTERACTIVE,
        });
    }
}

#[test]
fn every_host_reply_round_trips() {
    let changes = ChangeSet {
        changes: vec![
            FileChange {
                path: path("a.rs"),
                kind: ChangeKind::Added,
            },
            FileChange {
                path: path("b.rs"),
                kind: ChangeKind::Renamed { from: path("c.rs") },
            },
        ],
        truncated: true,
    };
    for reply in [
        HostReply::FileId(None),
        HostReply::FileId(Some(symbol().file)),
        HostReply::Symbols(vec![symbol()]),
        HostReply::Content(b"fn main() {}".to_vec()),
        HostReply::Changes(changes),
        HostReply::Completion {
            text: "because".into(),
            tokens_used: 4,
            model: ModelId::parse("pitwall/qwen3").unwrap(),
        },
    ] {
        round_trip(reply);
    }
}

#[test]
fn requests_replies_errors_and_contributions_round_trip() {
    round_trip(FunctionRequest {
        id: 2,
        tool: "find_symbol".into(),
        arguments: "{\"q\":\"x\"}".into(),
        budget: Budget::new(100, 10, 1000).unwrap(),
    });
    round_trip(FunctionReply {
        id: 2,
        result: Ok(ToolOutput {
            json: "[]".into(),
            partial: true,
        }),
    });
    for e in [
        FunctionError::UnknownTool("x".into()),
        FunctionError::InvalidArguments("q is required".into()),
        FunctionError::Cancelled,
        FunctionError::BudgetExceeded(BudgetKind::Millis),
        FunctionError::Host(HostError::Unavailable("set up Pitwall".into())),
        FunctionError::Host(HostError::Refused("outside the workspace".into())),
        FunctionError::Internal("bug".into()),
    ] {
        round_trip(FunctionReply::failed(3, e));
    }
    for e in [
        HostError::Cancelled,
        HostError::BudgetExceeded(BudgetKind::Bytes),
        HostError::NotFound,
        HostError::Internal("x".into()),
    ] {
        round_trip(e);
    }
    round_trip(Contributions {
        tools: vec![ToolSpec {
            name: "find_symbol".into(),
            description: "Finds.".into(),
            read_only: true,
            input_schema: "{}".into(),
        }],
        views: vec![ViewSpec {
            id: "symbols".into(),
            title: "Symbols".into(),
            kind: ViewKind::Graph,
            tool: "find_symbol".into(),
        }],
        settings: vec![
            SettingSpec {
                key: "a".into(),
                title: "A".into(),
                default: SettingValue::Bool(true),
            },
            SettingSpec {
                key: "b".into(),
                title: "B".into(),
                default: SettingValue::Int(-5),
            },
            SettingSpec {
                key: "c".into(),
                title: "C".into(),
                default: SettingValue::Text("x".into()),
            },
        ],
    });
}

/// A host that honours the cancel token and the budget, as every host must.
struct BoundedHost {
    symbols: Vec<SymbolRecord>,
}

impl Host for BoundedHost {
    fn call(&self, request: &HostRequest, cancel: &CancelToken) -> Result<HostReply, HostError> {
        if cancel.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        match &request.call {
            HostCall::FindSymbols { limit, .. } => {
                let found: Vec<_> = self.symbols.iter().take(*limit as usize).cloned().collect();
                if let Some(kind) = request.budget.exceeded_by(found.len(), 0) {
                    return Err(HostError::BudgetExceeded(kind));
                }
                Ok(HostReply::Symbols(found))
            }
            _ => Err(HostError::Refused("not in this fake".into())),
        }
    }
}

/// A function whose only tool asks the host for symbols and passes cancel and budget errors on.
struct Finder(Contributions);

impl Function for Finder {
    fn contributions(&self) -> &Contributions {
        &self.0
    }

    fn invoke(&self, request: &FunctionRequest, host: &dyn Host, cancel: &CancelToken) -> FunctionReply {
        if request.tool != "find_symbol" {
            return FunctionReply::failed(request.id, FunctionError::UnknownTool(request.tool.clone()));
        }
        let ask = HostRequest {
            id: request.id,
            call: HostCall::FindSymbols {
                query: request.arguments.clone(),
                limit: 5,
            },
            budget: request.budget,
        };
        match host.call(&ask, cancel) {
            Ok(HostReply::Symbols(s)) => FunctionReply {
                id: request.id,
                result: Ok(ToolOutput {
                    json: format!("{}", s.len()),
                    partial: false,
                }),
            },
            Ok(_) => FunctionReply::failed(request.id, FunctionError::Internal("unexpected reply".into())),
            Err(e) => FunctionReply::failed(request.id, e.into()),
        }
    }
}

#[test]
fn a_host_call_has_a_budget_and_a_cancel_token() {
    let host = BoundedHost {
        symbols: vec![symbol(), symbol(), symbol()],
    };
    let f = Finder(Contributions::default());
    let req = |budget| FunctionRequest {
        id: 1,
        tool: "find_symbol".into(),
        arguments: "x".into(),
        budget,
    };

    let ok = f.invoke(&req(Budget::INTERACTIVE), &host, &CancelToken::new());
    assert_eq!(ok.result.unwrap().json, "3");

    let tight = f.invoke(&req(Budget::new(100, 2, 1000).unwrap()), &host, &CancelToken::new());
    assert_eq!(tight.result, Err(FunctionError::BudgetExceeded(BudgetKind::Items)));

    let token = CancelToken::new();
    token.cancel();
    let cancelled = f.invoke(&req(Budget::INTERACTIVE), &host, &token);
    assert_eq!(cancelled.result, Err(FunctionError::Cancelled));

    let unknown = f.invoke(
        &FunctionRequest {
            tool: "nope".into(),
            ..req(Budget::INTERACTIVE)
        },
        &host,
        &CancelToken::new(),
    );
    assert_eq!(unknown.result, Err(FunctionError::UnknownTool("nope".into())));
}
