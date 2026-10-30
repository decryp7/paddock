//! SARIF 2.1.0 and rdjson.

use pacenotes_core::{Location, Position, RepoPath, Severity, Span};
use pacenotes_rules::shared_copy::{
    Finding, FindingFingerprint, FixSentence, FixSource, Reason, Report, Rule, RuleId, SarifLimits, Scope, read_rdjson, read_sarif,
    write_sarif,
};
use serde_json::Value;

use crate::support::{finding, path, rule};

fn report(findings: Vec<Finding>, rules: Vec<Rule>) -> Report {
    Report {
        scope: "repo".into(),
        tool: "pacenotes".into(),
        tool_version: "0.1.0".into(),
        rules,
        findings,
        hidden: Vec::new(),
        skipped: Vec::new(),
    }
}

fn no_text(_: &RepoPath) -> Option<String> {
    None
}

/// A finding whose columns lie after multi-byte characters.
fn wide(text_line: &str, from_byte: u32, to_byte: u32) -> Finding {
    let mut f = finding("wide", Severity::High, "src/w.rs", 2, text_line);
    f.location = Some(Location {
        path: path("src/w.rs"),
        span: Span::new(Position::new(2, from_byte).unwrap(), Position::new(2, to_byte).unwrap()).unwrap(),
    });
    f
}

#[test]
fn columns_in_bytes_round_trip_through_code_points_with_non_ascii_text() {
    // line 2: a 2-byte, a 3-byte and a 4-byte character, then the flagged `x`
    let text = "first\n\u{e9}\u{20ac}\u{1f600}x tail\n";
    let from = 1 + 2 + 3 + 4; // bytes before `x` on the line, plus one for 1-based
    let f = wide("x", from, from + 1);
    let texts = |p: &RepoPath| (p.as_str() == "src/w.rs").then(|| text.to_owned());
    let sarif = write_sarif(&report(vec![f.clone()], Vec::new()), &[], &texts);
    let doc: Value = serde_json::from_str(&sarif).unwrap();
    let region = &doc["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
    assert_eq!(
        (region["startColumn"].as_u64(), region["endColumn"].as_u64()),
        (Some(4), Some(5)),
        "code points, not bytes: {region}"
    );
    assert_eq!(doc["runs"][0]["columnKind"], "unicodeCodePoints");
    assert_eq!(
        (region["byteOffset"].as_u64(), region["byteLength"].as_u64()),
        (Some(6 + 9), Some(1))
    );
    let (back, stats) = read_sarif(&sarif, &SarifLimits::default(), "pacenotes", &texts).unwrap();
    assert_eq!(back[0].finding.location, f.location, "bytes again");
    assert_eq!(stats.columns_dropped, 0);
}

#[test]
fn the_default_column_kind_is_utf16_code_units_and_a_missing_text_drops_the_columns_not_the_finding() {
    // 4-byte character = a surrogate pair = 2 UTF-16 units: `x` is at UTF-16 column 3
    let text = "first\n\u{1f600}x\n";
    let doc = serde_json::json!({
        "version": "2.1.0",
        "runs": [{ "tool": { "driver": { "name": "other" } }, "results": [{
            "ruleId": "r", "message": { "text": "m" },
            "locations": [{ "physicalLocation": { "artifactLocation": { "uri": "src/w.rs" },
                "region": { "startLine": 2, "startColumn": 3, "endLine": 2, "endColumn": 4 } } }]
        }] }]
    })
    .to_string();
    let texts = |_: &RepoPath| Some(text.to_owned());
    let (got, _) = read_sarif(&doc, &SarifLimits::default(), "pacenotes", &texts).unwrap();
    let span = got[0].finding.location.as_ref().unwrap().span;
    assert_eq!(
        (span.start().column(), span.end().column()),
        (5, 6),
        "bytes: 4 for the emoji, then x at 5"
    );
    let (got, stats) = read_sarif(&doc, &SarifLimits::default(), "pacenotes", &no_text).unwrap();
    let span = got[0].finding.location.as_ref().unwrap().span;
    assert_eq!(
        (span.start().line(), span.start().column(), stats.columns_dropped),
        (2, 1, 1),
        "lines only"
    );
}

#[test]
fn imported_results_go_back_in_their_own_run_with_their_raw_id_and_suppressed_ones_stay_in_results() {
    let ext = RuleId::external("clippy", "clippy::Needless_Return").unwrap();
    let mut theirs = finding("x", Severity::Medium, "src/a.rs", 3, "return x;");
    theirs.rule = ext.clone();
    theirs.fix_source = FixSource::Tool;
    theirs.fix = FixSentence::parse("Remove the `return`.").unwrap();
    let mut generated_one = theirs.clone();
    generated_one.rule = RuleId::external("eslint", "no-var").unwrap();
    generated_one.fix_source = FixSource::Generated;
    generated_one.fix = FixSentence::parse("eslint gives no fix for no-var; see its documentation.").unwrap();
    let ours = finding("mine", Severity::High, "src/a.rs", 1, "x");
    let hidden = (
        finding("mine", Severity::High, "src/a.rs", 8, "y"),
        Reason::parse("legacy code").unwrap(),
        true,
    );
    let r = report(vec![ours, theirs, generated_one], vec![]);
    let text = write_sarif(&r, std::slice::from_ref(&hidden), &no_text);
    let doc: Value = serde_json::from_str(&text).unwrap();
    let runs = doc["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0]["tool"]["driver"]["name"], "pacenotes");
    assert_eq!(runs[0]["results"].as_array().unwrap().len(), 2, "one kept and one suppressed");
    assert_eq!(runs[0]["results"][1]["suppressions"][0]["justification"], "legacy code");
    assert_eq!(runs[0]["results"][1]["suppressions"][0]["kind"], "inSource");
    assert_eq!(runs[0]["results"][0]["ruleIndex"], 0);
    assert_eq!(
        runs[0]["originalUriBaseIds"]["%SRCROOT%"]["description"]["text"],
        "The root of the repository"
    );
    assert_eq!(runs[0]["automationDetails"]["id"], "repo");
    let clippy = runs.iter().find(|r| r["tool"]["driver"]["name"] == "clippy").unwrap();
    assert_eq!(clippy["results"][0]["ruleId"], "clippy::Needless_Return");
    assert_eq!(clippy["tool"]["driver"]["rules"][0]["help"]["text"], "Remove the `return`.");
    let eslint = runs.iter().find(|r| r["tool"]["driver"]["name"] == "eslint").unwrap();
    assert!(
        eslint["tool"]["driver"]["rules"][0].get("help").is_none(),
        "a generated sentence is never written as the tool's help"
    );
    assert!(eslint["results"][0]["properties"].get("fix").is_none());

    let (back, stats) = read_sarif(&text, &SarifLimits::default(), "pacenotes", &no_text).unwrap();
    assert_eq!(stats, Default::default());
    assert_eq!(back.len(), 4);
    let c = back.iter().find(|i| i.finding.rule == ext).unwrap();
    assert_eq!(
        (c.finding.fix.as_str(), c.finding.fix_source),
        ("Remove the `return`.", FixSource::Tool)
    );
    let g = back.iter().find(|i| i.finding.rule.to_string() == "eslint:no-var").unwrap();
    assert_eq!(g.finding.fix_source, FixSource::Generated);
    assert!(g.finding.fix.as_str().starts_with("eslint gives no fix for no-var"));
    let s = back.iter().find(|i| i.suppressed_by.is_some()).unwrap();
    assert_eq!(s.suppressed_by.as_ref().unwrap().as_str(), "legacy code");
    assert_eq!(s.finding.severity, Severity::High, "the exact severity survives");
}

#[test]
fn our_own_results_round_trip_with_their_identity_and_rule_metadata() {
    let rule_ = Rule {
        id: rule("mine"),
        title: "Mine".into(),
        default_severity: Severity::High,
        fix: FixSentence::parse("Fix mine.").unwrap(),
        policy_ref: Some("rule 37".into()),
        version: 3,
        deprecated_ids: Vec::new(),
        scope: Scope::default(),
    };
    let f = finding("mine", Severity::High, "src/a.rs", 4, "snippet");
    let text = write_sarif(&report(vec![f.clone()], vec![rule_]), &[], &no_text);
    let doc: Value = serde_json::from_str(&text).unwrap();
    let d = &doc["runs"][0]["tool"]["driver"]["rules"][0];
    assert_eq!(
        (d["properties"]["policyRef"].as_str(), d["properties"]["ruleVersion"].as_u64()),
        (Some("rule 37"), Some(3))
    );
    assert!(d.get("helpUri").is_none(), "policy_ref is not a URL");
    assert_eq!(
        doc["runs"][0]["results"][0]["partialFingerprints"]["paddockFindingHash/v1"],
        f.fingerprint.unwrap().to_hex()
    );
    let (back, _) = read_sarif(&text, &SarifLimits::default(), "pacenotes", &no_text).unwrap();
    assert_eq!(back[0].finding.rule, f.rule);
    assert_eq!(back[0].finding.fingerprint, f.fingerprint);
    assert_eq!(back[0].finding.fix_source, FixSource::Rule);
    assert_eq!(back[0].finding.location.as_ref().unwrap().span.start().line(), 4);
}

#[test]
fn the_reader_applies_sarifs_defaults_and_skips_what_is_not_a_failure() {
    let doc = serde_json::json!({
        "version": "2.1.0",
        "runs": [{ "tool": { "driver": { "name": "t", "rules": [
            { "id": "a", "defaultConfiguration": { "level": "error" }, "help": { "text": "Do A. Then B." } },
            { "id": "b", "fullDescription": { "text": "Full text of b. More." } },
            { "id": "c" }
        ] } }, "results": [
            { "ruleId": "a", "message": { "text": "m" } },
            { "ruleId": "b", "level": "note", "message": { "text": "m" }, "fixes": [{ "description": { "text": "Own fix." } }] },
            { "ruleId": "c", "message": { "text": "m" } },
            { "ruleId": "a", "kind": "pass", "message": { "text": "m" } },
            { "ruleIndex": 1, "message": { "text": "m" } },
            { "message": { "text": "no rule at all" } },
            { "ruleId": "a", "message": { "text": "m" }, "locations": [{ "physicalLocation": { "artifactLocation": { "uri": "/etc/passwd" }, "region": { "startLine": 1 } } }] },
            { "ruleId": "a", "message": { "text": "m" }, "locations": [{ "physicalLocation": { "artifactLocation": { "uri": "a.rs" }, "region": { "startLine": 0 } } }] }
        ] }]
    })
    .to_string();
    let (got, stats) = read_sarif(&doc, &SarifLimits::default(), "pacenotes", &no_text).unwrap();
    let sev: Vec<_> = got.iter().map(|i| i.finding.severity).collect();
    assert_eq!(
        sev[..4],
        [Severity::High, Severity::Info, Severity::Medium, Severity::Medium],
        "{sev:?}"
    );
    assert_eq!(got[0].finding.fix.as_str(), "Do A.", "the rule's help, first sentence");
    assert_eq!(got[1].finding.fix.as_str(), "Own fix.", "the result's own fix comes first");
    assert_eq!(got[2].finding.fix_source, FixSource::Generated);
    assert_eq!(got[3].finding.rule.to_string(), "t:b", "ruleIndex resolves the rule");
    assert_eq!(
        got[3].finding.fix.as_str(),
        "Full text of b.",
        "fullDescription when nothing better"
    );
    assert_eq!((stats.skipped_kind, stats.no_rule, stats.bad_location), (1, 1, 2));
    assert!(got[4].finding.location.is_none() && got[5].finding.location.is_none());
}

#[test]
fn hostile_or_wrong_input_is_an_error_or_bounded_and_never_a_panic() {
    let l = SarifLimits {
        max_bytes: 100,
        max_results: 2,
        max_message: 5,
    };
    assert!(
        read_sarif(&"x".repeat(101), &l, "p", &no_text)
            .unwrap_err()
            .to_string()
            .contains("at most 100")
    );
    assert!(read_sarif("not json", &SarifLimits::default(), "p", &no_text).is_err());
    assert!(read_sarif("{\"version\":\"2.0.0\",\"runs\":[]}", &SarifLimits::default(), "p", &no_text).is_err());
    assert!(read_sarif("{\"version\":\"2.1.0\"}", &SarifLimits::default(), "p", &no_text).is_err());
    let results = (0..5)
        .map(|i| serde_json::json!({ "ruleId": format!("r{i}"), "message": { "text": "a long message" } }))
        .collect::<Vec<_>>();
    let doc =
        serde_json::json!({ "version": "2.1.0", "runs": [{ "tool": { "driver": { "name": "t" } }, "results": results }] }).to_string();
    let l = SarifLimits {
        max_bytes: 10_000,
        max_results: 2,
        max_message: 5,
    };
    let (got, stats) = read_sarif(&doc, &l, "p", &no_text).unwrap();
    assert_eq!((got.len(), stats.truncated, got[0].finding.message.as_str()), (2, 3, "a lon"));
    let deep = format!("{}1{}", "[".repeat(5000), "]".repeat(5000));
    assert!(read_sarif(&deep, &SarifLimits::default(), "p", &no_text).is_err());
    for junk in ["{}", "[]", "null", "{\"version\":\"2.1.0\",\"runs\":[1,null,{\"results\":5}]}"] {
        let _ = read_sarif(junk, &SarifLimits::default(), "p", &no_text);
    }
}

#[test]
fn rdjson_is_read_with_code_point_columns_and_its_codes_as_external_rules() {
    let text = "a\n\u{e9}x\n";
    let texts = |_: &RepoPath| Some(text.to_owned());
    let doc = serde_json::json!({
        "source": { "name": "golint" },
        "diagnostics": [
            { "message": "m1", "severity": "ERROR", "code": { "value": "G1", "url": "https://example.test/g1" },
              "location": { "path": "src/a.go", "range": { "start": { "line": 2, "column": 2 }, "end": { "line": 2, "column": 3 } } } },
            { "message": "m2", "code": { "value": "G2" }, "location": { "path": "src/a.go", "range": { "start": { "line": 1 } } } },
            { "message": "no code" },
            { "message": "bad path", "code": { "value": "G3" }, "location": { "path": "../x", "range": { "start": { "line": 1 } } } }
        ]
    })
    .to_string();
    let (got, stats) = read_rdjson(&doc, &SarifLimits::default(), &texts).unwrap();
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].finding.rule.to_string(), "golint:G1");
    assert_eq!(got[0].finding.severity, Severity::High);
    let span = got[0].finding.location.as_ref().unwrap().span;
    assert_eq!(
        (span.start().column(), span.end().column()),
        (3, 4),
        "code point 2 is byte 3 after the 2-byte character"
    );
    assert_eq!(got[1].finding.severity, Severity::Medium);
    assert_eq!(got[0].finding.fix_source, FixSource::Generated);
    assert_eq!((stats.no_rule, stats.bad_location), (1, 1));
    assert!(got[2].finding.location.is_none());
    assert!(read_rdjson("{}", &SarifLimits::default(), &texts).is_err() && read_rdjson("nope", &SarifLimits::default(), &texts).is_err());
    let _ = FindingFingerprint::compute(&rule("x"), "", None);
}
