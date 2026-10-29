//! Every serde type of the core round-trips through JSON and postcard, and every validated type refuses an invalid value on
//! both (ADR 0002, ER-21: validation cannot be bypassed by reading a value instead of parsing it, and a test over the whole
//! list keeps the class closed).

use serde::{Serialize, de::DeserializeOwned};

use crate::pack::{Download, PackWire};
use crate::ports::{
    ChangeKind, ChangeSet, Completion, FileChange, LspReply, LspRequest, ModelId, SymbolPage, SymbolRecord, TokenBudget, VectorHit,
};
use crate::rule::RuleWire;
use crate::*;

fn both<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<T>(&json).unwrap(), value, "json {json}");
    let bytes = postcard::to_stdvec(&value).unwrap();
    assert_eq!(postcard::from_bytes::<T>(&bytes).unwrap(), value, "postcard {value:?}");
}

/// `json` is an invalid value as JSON text; `wire` is the same invalid value as the postcard bytes of an equal-shaped value.
fn refuses<T: DeserializeOwned + std::fmt::Debug>(name: &str, json: &str, wire: &impl Serialize) {
    assert!(serde_json::from_str::<T>(json).is_err(), "{name}: JSON accepted {json}");
    let bytes = postcard::to_stdvec(wire).unwrap();
    assert!(
        postcard::from_bytes::<T>(&bytes).is_err(),
        "{name}: postcard accepted an invalid value"
    );
}

fn path(p: &str) -> RepoPath {
    RepoPath::parse(p).unwrap()
}

fn pos(l: u32, c: u32) -> Position {
    Position::new(l, c).unwrap()
}

const PACK: &str = "[pack]\nid = \"go\"\nlanguages = [\"go\"]\nglobs = [\"**/*.go\"]\n[server]\ncommand = \"gopls\"\ninit_options = { a = 1 }\n[features]\nuse = [\"definition\"]\n";
const RULE: &str = "id = \"r\"\ntitle = \"T\"\nseverity = \"low\"\ngood = [\"a\"]\nbad = [\"b\"]\n[check]\npattern = \"$X\"\n";

#[test]
fn every_core_type_round_trips_through_json_and_postcard() {
    let file = FileIdAllocator::new().issue();
    let sym = SymbolId::derive(file, &Language::Known(KnownLanguage::Rust), SymbolKind::Function, "f", 0);
    let record = SymbolRecord {
        id: sym,
        file,
        kind: SymbolKind::Method,
        qualified_name: "A::f".into(),
        span: Span::lines(1, 2).unwrap(),
        confidence: Confidence::Exact,
    };
    both(path("a/b.rs"));
    both(pos(3, 4));
    both(Span::new(pos(1, 1), pos(2, 5)).unwrap());
    both(Location {
        path: path("a.rs"),
        span: Span::lines(1, 1).unwrap(),
    });
    let hash = CommitHash::parse(&"ab".repeat(20)).unwrap();
    both(hash.clone());
    both(Generation(9));
    both(Rev::Commit(hash));
    both(Rev::WorkingTree(Generation(2)));
    for t in [Tier::Text, Tier::Syntax, Tier::Semantic] {
        both(t);
    }
    for c in [Confidence::Guess, Confidence::Heuristic, Confidence::Resolved, Confidence::Exact] {
        both(c);
    }
    for s in [Severity::Info, Severity::Low, Severity::Medium, Severity::High, Severity::Blocker] {
        both(s);
    }
    for k in KnownLanguage::ALL {
        both(Language::Known(k));
    }
    both(Language::parse("zig").unwrap());
    for f in [
        FindingState::Open,
        FindingState::Confirmed,
        FindingState::Dismissed,
        FindingState::Fixed,
        FindingState::Suppressed,
    ] {
        both(f);
    }
    both(file);
    both(sym);
    both(ChunkId::derive(file, Some(sym), 1));
    both(SymbolKind::TypeAlias);
    both(ModelId::parse("a/b").unwrap());
    both(TokenBudget::new(10).unwrap());
    both(LspFeature::CallHierarchy);
    both(RuleId::parse("r.1").unwrap());
    both(Check::Pattern("p".into()));
    both(Check::Markdown("m".into()));
    both(record.clone());
    both(SymbolPage {
        symbols: vec![record],
        truncated: true,
    });
    both(VectorHit {
        chunk: ChunkId::from_bytes([1; 16]),
        score: 0.5,
    });
    both(ChangeSet {
        changes: vec![FileChange {
            path: path("a.rs"),
            kind: ChangeKind::Renamed { from: path("b.rs") },
        }],
        truncated: false,
    });
    both(LspRequest {
        feature: LspFeature::Definition,
        path: path("a.rs"),
        position: Some(pos(1, 2)),
    });
    both(LspReply::bounded(vec![Location {
        path: path("a.rs"),
        span: Span::lines(1, 1).unwrap(),
    }]));
    both(Completion {
        text: "t".into(),
        tokens_used: 1,
        model: ModelId::parse("m").unwrap(),
    });
    both(ServerPack::parse(PACK).unwrap());
    both(RuleSpec::from_toml(RULE).unwrap());
    both(Download::new("https://example.invalid/x", Some(&"a".repeat(64))).unwrap());
}

#[test]
fn every_validated_type_refuses_an_invalid_value_on_json_and_postcard() {
    refuses::<RepoPath>("RepoPath", "\"../x\"", &"../x");
    refuses::<CommitHash>("CommitHash", "\"zz\"", &"zz");
    refuses::<Position>("Position", r#"{"line":0,"column":1}"#, &(0u32, 1u32));
    refuses::<Span>(
        "Span",
        r#"{"start":{"line":3,"column":1},"end":{"line":2,"column":1}}"#,
        &((3u32, 1u32), (2u32, 1u32)),
    );
    refuses::<Language>("Language", "\"no way\"", &"no way");
    refuses::<FileId>("FileId", "\"nope\"", &"nope");
    refuses::<SymbolId>("SymbolId", "\"sym:zz\"", &"sym:zz");
    refuses::<ChunkId>("ChunkId", "\"chk:00\"", &"chk:00");
    refuses::<ModelId>("ModelId", "\"no no\"", &"no no");
    refuses::<TokenBudget>("TokenBudget", "0", &0u32);
    refuses::<RuleId>("RuleId", "\"Bad Id\"", &"Bad Id");
    refuses::<Download>("Download", r#"{"url":"http://x","sha256":"abc"}"#, &("http://x", "abc"));

    let pack = ServerPack::parse(PACK).unwrap();
    let mut wire = PackWire {
        id: pack.id().into(),
        name: None,
        languages: pack.languages().to_vec(),
        globs: pack.globs().to_vec(),
        command: "bin/not-bare".into(),
        args: vec![],
        env: Default::default(),
        detect: vec![],
        install_offer: None,
        download: None,
        init_options: "{}".into(),
        discover: vec![],
        features: pack.features().to_vec(),
        scip_command: None,
    };
    let json = serde_json::to_string(&wire).unwrap();
    refuses::<ServerPack>("ServerPack (command)", &json, &wire);
    wire.command = "gopls".into();
    wire.env.insert("LD_PRELOAD".into(), "/x.so".into());
    refuses::<ServerPack>("ServerPack (env)", &serde_json::to_string(&wire).unwrap(), &wire);
    wire.env.clear();
    wire.init_options = "[1]".into();
    refuses::<ServerPack>("ServerPack (init_options)", &serde_json::to_string(&wire).unwrap(), &wire);
    wire.init_options = "{}".into();
    assert!(ServerPack::from_wire_for_test(wire), "the same wire with valid values is accepted");

    let rule = RuleSpec::from_toml(RULE).unwrap();
    let bad = RuleWire {
        id: rule.id().clone(),
        title: String::new(),
        severity: rule.severity(),
        include: vec![],
        exclude: vec![],
        languages: vec![],
        check: rule.check().clone(),
        fix: None,
        good: vec![],
        bad: vec![],
    };
    refuses::<RuleSpec>("RuleSpec", &serde_json::to_string(&bad).unwrap(), &bad);
}
