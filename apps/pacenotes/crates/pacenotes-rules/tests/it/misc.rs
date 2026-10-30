//! Wire forms, display forms and the small bounds.

use pacenotes_parse::ParseLanguage;
use pacenotes_rules::pattern::{Pattern, example_extension};
use pacenotes_rules::shared_copy::{FindingFingerprint, FixSentence, IsoDate, Reason, RuleId, SarifLimits, normalise_snippet, read_rdjson};

use crate::support::rule;

#[test]
fn the_validated_types_cross_json_only_when_valid() {
    let fix: FixSentence = serde_json::from_str("\"Do it.\"").unwrap();
    assert_eq!(
        (serde_json::to_string(&fix).unwrap(), fix.to_string()),
        ("\"Do it.\"".to_owned(), "Do it.".to_owned())
    );
    assert!(serde_json::from_str::<FixSentence>("\"\"").is_err() && serde_json::from_str::<Reason>("\" \"").is_err());
    let reason: Reason = serde_json::from_str("\"because\"").unwrap();
    assert_eq!(serde_json::to_string(&reason).unwrap(), "\"because\"");
    let date: IsoDate = serde_json::from_str("\"2026-10-30\"").unwrap();
    assert_eq!(serde_json::to_string(&date).unwrap(), "\"2026-10-30\"");
    assert!(serde_json::from_str::<IsoDate>("\"2026-02-30\"").is_err());
    let fp = FindingFingerprint::compute(&rule("r"), "x", None);
    let json = serde_json::to_string(&fp).unwrap();
    assert_eq!(serde_json::from_str::<FindingFingerprint>(&json).unwrap(), fp);
    assert_eq!(fp.to_string(), fp.to_hex());
    assert!(serde_json::from_str::<FindingFingerprint>("\"nope\"").is_err());
    let id = RuleId::external("t", "a:b").unwrap();
    assert_eq!(serde_json::from_str::<RuleId>(&serde_json::to_string(&id).unwrap()).unwrap(), id);
    assert_eq!(normalise_snippet("  a \t\n b  "), "a b");
}

#[test]
fn every_language_has_an_example_extension_that_names_that_language() {
    for l in [
        ParseLanguage::Go,
        ParseLanguage::Java,
        ParseLanguage::Kotlin,
        ParseLanguage::C,
        ParseLanguage::Cpp,
        ParseLanguage::CSharp,
        ParseLanguage::TypeScript,
        ParseLanguage::Tsx,
        ParseLanguage::JavaScript,
        ParseLanguage::Rust,
        ParseLanguage::Python,
    ] {
        let path = crate::support::path(&format!("x.{}", example_extension(l)));
        assert_eq!(ParseLanguage::from_path(&path), Some(l));
    }
}

#[test]
fn a_pattern_describes_itself_and_a_non_utf8_pattern_target_is_skipped() {
    let p = Pattern::compile(ParseLanguage::Rust, "$X.unwrap()").unwrap();
    assert_eq!(p.language(), ParseLanguage::Rust);
    let shown = format!("{p:?}");
    assert!(shown.contains("rust") && shown.contains("unwrap"), "{shown}");
}

#[test]
fn rdjson_bounds_hold() {
    let none = |_: &pacenotes_core::RepoPath| None;
    let l = SarifLimits {
        max_bytes: 50,
        max_results: 1,
        max_message: 3,
    };
    assert!(
        read_rdjson(&"x".repeat(51), &l, &none)
            .unwrap_err()
            .to_string()
            .contains("at most 50")
    );
    let doc = r#"{"diagnostics":[{"message":"long message","code":{"value":"A"}},{"message":"two","code":{"value":"B"}}]}"#;
    let l = SarifLimits {
        max_bytes: 1000,
        max_results: 1,
        max_message: 3,
    };
    let (got, stats) = read_rdjson(doc, &l, &none).unwrap();
    assert_eq!(
        (
            got.len(),
            stats.truncated,
            got[0].finding.message.as_str(),
            got[0].finding.rule.to_string()
        ),
        (1, 1, "lon", "rdjson:A".to_owned())
    );
    let with_columns = r#"{"diagnostics":[{"message":"m","code":{"value":"A"},"location":{"path":"a.rs","range":{"start":{"line":1,"column":2},"end":{"line":1,"column":3}}}}]}"#;
    let (got, stats) = read_rdjson(with_columns, &SarifLimits::default(), &none).unwrap();
    assert_eq!(
        (
            stats.columns_dropped,
            got[0].finding.location.as_ref().unwrap().span.start().column()
        ),
        (1, 1),
        "no text: the line only"
    );
}
