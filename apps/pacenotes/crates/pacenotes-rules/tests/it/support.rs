//! Builders for findings and rules.

use pacenotes_core::rule::RuleSpec;
use pacenotes_core::{Location, RepoPath, Severity, Span};
use pacenotes_rules::shared_copy::{Finding, FindingFingerprint, FixSentence, FixSource, RuleId};

pub fn path(p: &str) -> RepoPath {
    RepoPath::parse(p).unwrap()
}

pub fn rule(id: &str) -> RuleId {
    RuleId::local(id).unwrap()
}

pub fn finding(id: &str, severity: Severity, file: &str, line: u32, snippet: &str) -> Finding {
    let rule = rule(id);
    Finding {
        fingerprint: Some(FindingFingerprint::compute(&rule, snippet, None)),
        rule,
        severity,
        location: Some(Location {
            path: path(file),
            span: Span::lines(line, line).unwrap(),
        }),
        message: format!("{id} found"),
        fix: FixSentence::parse("Do the thing.").unwrap(),
        fix_source: FixSource::Rule,
        explanation: None,
        evidence: Vec::new(),
        fix_diff: None,
        confidence: None,
    }
}

pub fn spec(toml: &str) -> RuleSpec {
    RuleSpec::from_toml(toml).unwrap()
}

pub const UNWRAP_RULE: &str = r#"
id = "no-unwrap"
title = "Do not unwrap"
severity = "high"
languages = ["rust"]
fix = "Return the error with `?` or handle it."
good = ["fn f() { let a = g()?; }", "fn f() { a.unwrap_or(1); }"]
bad = ["fn f() { a.unwrap(); }"]
check = { pattern = "$X.unwrap()" }
"#;
