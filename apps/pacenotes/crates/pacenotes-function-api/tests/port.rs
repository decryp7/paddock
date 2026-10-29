#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The port's contract (plan FN0): every message type round-trips through postcard and survives hostile bytes seeded from
//! its own encoding, every validated type refuses an invalid value, and a host call has a budget and a cancel token.

use pacenotes_core::ports::{ChangeKind, ChangeSet, FileChange, ModelId, SymbolRecord, TokenBudget};
use pacenotes_core::{
    CommitHash, Confidence, FileIdAllocator, Generation, KnownLanguage, Language, RepoPath, Rev, Span, SymbolId, SymbolKind,
};
use pacenotes_function_api::contract::{HostFixture, host_contract};
use pacenotes_function_api::{
    Budget, BudgetExceeded, BudgetKind, Contributions, Function, FunctionError, FunctionReply, FunctionRequest, Host, HostCall, HostError,
    HostReply, HostRequest, Origin, SettingSpec, SettingValue, ToolOutput, ToolSpec, ViewKind, ViewSpec, decode, encode,
};
use paddock_jobs::CancelToken;
use serde::{Serialize, de::DeserializeOwned};

fn path(p: &str) -> RepoPath {
    RepoPath::parse(p).unwrap()
}

fn budget() -> Budget {
    Budget::new(30, 1000, 1 << 20).unwrap()
}

/// Round-trips `value` and fuzzes the decoder of its type with hostile variants of its own encoding.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
    let bytes = encode(&value).unwrap();
    assert_eq!(decode::<T>(&bytes).unwrap(), value);
    paddock_testkit::assert_never_panics(&[bytes.as_slice()], |b| decode::<T>(b).is_ok());
}

fn rev() -> Rev {
    Rev::Commit(CommitHash::parse(&"ab".repeat(20)).unwrap())
}

fn symbol() -> SymbolRecord {
    let file = FileIdAllocator::new().issue().unwrap();
    SymbolRecord {
        id: SymbolId::derive(file, &Language::Known(KnownLanguage::Rust), SymbolKind::Function, "f", 0),
        file,
        kind: SymbolKind::Function,
        qualified_name: "f".into(),
        span: Span::lines(1, 2).unwrap(),
        confidence: Confidence::Resolved,
    }
}

fn request(call: HostCall) -> HostRequest {
    HostRequest::new(1, call, budget()).unwrap()
}

#[test]
fn every_host_call_round_trips() {
    let file = symbol().file;
    for call in [
        HostCall::FileId { path: path("a/b.rs") },
        HostCall::SymbolsInFile { file },
        HostCall::FindSymbols { query: "parse".into() },
        HostCall::ReadAt {
            rev: rev(),
            path: path("a.rs"),
        },
        HostCall::ChangedBetween {
            from: rev(),
            to: Rev::WorkingTree(Generation(3)),
        },
        HostCall::Complete {
            prompt: "why?".into(),
            origin: Origin::UserRequest,
            tokens: TokenBudget::new(256).unwrap(),
        },
        HostCall::Complete {
            prompt: "why?".into(),
            origin: Origin::Background,
            tokens: TokenBudget::new(1).unwrap(),
        },
    ] {
        round_trip(request(call));
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
        budget: Budget::new(5, 10, 1000).unwrap(),
    });
    round_trip(FunctionReply {
        id: 2,
        result: Ok(ToolOutput {
            json: "[]".into(),
            partial: true,
        }),
    });
    let exceeded = BudgetExceeded {
        kind: BudgetKind::Ticks,
        limit: 30,
    };
    for e in [
        FunctionError::UnknownTool("x".into()),
        FunctionError::InvalidArguments("q is required".into()),
        FunctionError::Cancelled,
        FunctionError::BudgetExceeded(exceeded),
        FunctionError::Host(HostError::Unavailable("set up Pitwall".into())),
        FunctionError::Host(HostError::Refused("outside the workspace".into())),
        FunctionError::Internal("bug".into()),
    ] {
        round_trip(FunctionReply::failed(3, e));
    }
    for e in [
        HostError::Cancelled,
        HostError::BudgetExceeded(exceeded),
        HostError::NotFound,
        HostError::Internal("x".into()),
    ] {
        round_trip(e);
    }
    round_trip(
        Contributions::new(
            vec![ToolSpec {
                name: "find_symbol".into(),
                description: "Finds.".into(),
                read_only: true,
                input_schema: "{}".into(),
            }],
            vec![ViewSpec {
                id: "symbols".into(),
                title: "Symbols".into(),
                kind: ViewKind::Graph,
                tool: "find_symbol".into(),
            }],
            vec![
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
        )
        .unwrap(),
    );
    round_trip(Budget::new(1, 1, 1).unwrap());
}

/// An invalid value as JSON text and as the postcard bytes of an equal-shaped value must both be refused (ER-21).
fn refuses<T: DeserializeOwned + std::fmt::Debug>(name: &str, json: &str, wire: &impl Serialize) {
    assert!(serde_json::from_str::<T>(json).is_err(), "{name}: JSON accepted {json}");
    let bytes = postcard::to_stdvec(wire).unwrap();
    assert!(
        postcard::from_bytes::<T>(&bytes).is_err(),
        "{name}: postcard accepted an invalid value"
    );
}

#[test]
fn every_validated_type_refuses_an_invalid_value_on_json_and_postcard() {
    refuses::<Budget>("Budget (zero ticks)", r#"{"ticks":0,"items":1,"bytes":1}"#, &(0u32, 1u32, 1u32));
    refuses::<Budget>(
        "Budget (too many items)",
        r#"{"ticks":1,"items":100001,"bytes":1}"#,
        &(1u32, 100_001u32, 1u32),
    );
    // Contributions: a view that names a tool the function does not have (a struct is its fields, in order, on the wire)
    let view = ViewSpec {
        id: "v".into(),
        title: "V".into(),
        kind: ViewKind::Table,
        tool: "missing".into(),
    };
    let wire = (Vec::<ToolSpec>::new(), vec![view], Vec::<SettingSpec>::new());
    let json = r#"{"tools":[],"views":[{"id":"v","title":"V","kind":"Table","tool":"missing"}],"settings":[]}"#;
    refuses::<Contributions>("Contributions", json, &wire);
    // HostRequest: a query longer than its bound
    let long = "q".repeat(2000);
    let call = HostCall::FindSymbols { query: long.clone() };
    let wire = (1u64, call.clone(), (30u32, 10u32, 1000u32));
    let json =
        serde_json::json!({ "id": 1, "call": { "FindSymbols": { "query": long } }, "budget": { "ticks": 30, "items": 10, "bytes": 1000 } });
    refuses::<HostRequest>("HostRequest", &json.to_string(), &wire);
}

#[test]
fn hostile_bytes_never_panic_any_decoder() {
    // the seeds are the types' own encodings (see `round_trip`); this adds the empty and the all-0xff inputs per type
    for bytes in [vec![], vec![0xff; 64], vec![0; 64]] {
        let _ = decode::<HostRequest>(&bytes);
        let _ = decode::<HostReply>(&bytes);
        let _ = decode::<FunctionRequest>(&bytes);
        let _ = decode::<FunctionReply>(&bytes);
        let _ = decode::<Contributions>(&bytes);
        let _ = decode::<Budget>(&bytes);
    }
}

/// A host that honours the cancel token and the budget, as every host must.
struct BoundedHost {
    symbols: Vec<SymbolRecord>,
    /// The job context the host runs the function in; it, not the request, decides who may call the model.
    background: bool,
}

impl Host for BoundedHost {
    fn call(&self, request: &HostRequest, cancel: &CancelToken) -> Result<HostReply, HostError> {
        if cancel.is_cancelled() {
            return Err(HostError::Cancelled);
        }
        match request.call() {
            HostCall::FindSymbols { .. } => {
                if let Some(b) = request.budget().exceeded_by(self.symbols.len(), 0) {
                    return Err(HostError::BudgetExceeded(b));
                }
                Ok(HostReply::Symbols(self.symbols.clone()))
            }
            HostCall::Complete { origin, .. } if self.background || *origin == Origin::Background => {
                Err(HostError::Refused("the model answers only a person who is waiting".into()))
            }
            _ => Err(HostError::Refused("not in this fake".into())),
        }
    }
}

#[test]
fn the_fake_host_passes_the_host_contract() {
    let host = BoundedHost {
        symbols: vec![symbol(), symbol(), symbol()],
        background: false,
    };
    let background = BoundedHost {
        symbols: vec![],
        background: true,
    };
    host_contract(&HostFixture {
        host: &host,
        background_host: &background,
        query: "f",
    });
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
        let ask = HostRequest::new(
            request.id,
            HostCall::FindSymbols {
                query: request.arguments.clone(),
            },
            request.budget,
        )
        .unwrap();
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
fn a_function_passes_cancel_and_budget_errors_through() {
    let host = BoundedHost {
        symbols: vec![symbol(), symbol(), symbol()],
        background: false,
    };
    let f = Finder(Contributions::default());
    let req = |budget| FunctionRequest {
        id: 1,
        tool: "find_symbol".into(),
        arguments: "x".into(),
        budget,
    };

    let ok = f.invoke(&req(budget()), &host, &CancelToken::new());
    assert_eq!(ok.result.unwrap().json, "3");

    let tight = f.invoke(&req(Budget::new(100, 2, 1000).unwrap()), &host, &CancelToken::new());
    match tight.result {
        Err(FunctionError::BudgetExceeded(b)) => assert_eq!((b.kind, b.limit), (BudgetKind::Items, 2)),
        other => panic!("{other:?}"),
    }

    let token = CancelToken::new();
    token.cancel();
    assert_eq!(f.invoke(&req(budget()), &host, &token).result, Err(FunctionError::Cancelled));

    let unknown = f.invoke(
        &FunctionRequest {
            tool: "nope".into(),
            ..req(budget())
        },
        &host,
        &CancelToken::new(),
    );
    assert_eq!(unknown.result, Err(FunctionError::UnknownTool("nope".into())));
}

#[test]
fn a_host_request_is_bounded_and_exposes_its_parts() {
    let r = request(HostCall::FindSymbols { query: "q".into() });
    assert_eq!((r.id(), r.budget().items()), (1, 1000));
    assert!(matches!(r.call(), HostCall::FindSymbols { .. }));
    let long = "p".repeat(pacenotes_function_api::message::MAX_PROMPT_BYTES + 1);
    let call = HostCall::Complete {
        prompt: long,
        origin: Origin::UserRequest,
        tokens: TokenBudget::new(1).unwrap(),
    };
    let err = HostRequest::new(1, call, budget()).unwrap_err();
    assert!(err.to_string().contains("prompt"), "{err}");
    let q = HostCall::FindSymbols {
        query: "q".repeat(pacenotes_function_api::message::MAX_QUERY_BYTES + 1),
    };
    assert!(HostRequest::new(1, q, budget()).unwrap_err().to_string().contains("query"));
}

/// The class test lists its types by hand, so this test finds them: every type of this crate read through `try_from` must
/// have a `refuses::<Type>` line in this file.
#[test]
fn the_class_test_covers_every_validated_type() {
    let this = include_str!("port.rs");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let source = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        let lines: Vec<&str> = source.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("#[serde(try_from") {
                let decl = lines[i + 1..].iter().find(|l| l.contains("pub struct ")).unwrap();
                let rest = decl.split("pub struct ").nth(1).unwrap();
                names.push(rest.split(|c: char| !c.is_alphanumeric()).next().unwrap().to_owned());
            }
        }
    }
    assert!(names.len() >= 3, "the scan found only {names:?}");
    for name in &names {
        assert!(
            this.contains(&format!("refuses::<{name}>")),
            "{name} is read through a validator but the class test does not check it"
        );
    }
}
