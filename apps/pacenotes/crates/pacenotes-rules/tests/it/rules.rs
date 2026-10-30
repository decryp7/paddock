//! Compiled rules: scope, findings, identity, and the rule tests.

use pacenotes_core::rule::RuleSpec;
use pacenotes_rules::pattern::{CompiledRule, MatchLimits, PatternError};
use pacenotes_rules::rule_test::test_rule;
use pacenotes_rules::shared_copy::FixSource;

use crate::support::{UNWRAP_RULE, path, spec};

#[test]
fn a_rule_finds_its_pattern_with_its_own_fix_severity_and_a_stable_identity() {
    let rule = CompiledRule::compile(&spec(UNWRAP_RULE)).unwrap();
    let code = "struct S;\nimpl S {\n    fn run(&self) {\n        a.unwrap();\n    }\n}\n";
    let at = rule.run(&path("src/a.rs"), code.as_bytes(), &MatchLimits::default());
    assert_eq!(at.len(), 1);
    let f = &at[0];
    assert_eq!(f.severity, pacenotes_core::Severity::High);
    assert_eq!(f.fix.as_str(), "Return the error with `?` or handle it.");
    assert_eq!(f.fix_source, FixSource::Rule);
    assert_eq!(f.location.as_ref().unwrap().span.start().line(), 4);
    assert_eq!(f.evidence, ["a.unwrap()"]);
    // the same code in a moved file has the same identity (no path in it)
    let moved = rule.run(&path("src/deeper/b.rs"), code.as_bytes(), &MatchLimits::default());
    assert_eq!(moved[0].fingerprint, f.fingerprint);
    // the enclosing symbol is part of it
    let other = "struct S;\nimpl S {\n    fn other(&self) {\n        a.unwrap();\n    }\n}\n";
    assert_ne!(
        rule.run(&path("src/a.rs"), other.as_bytes(), &MatchLimits::default())[0].fingerprint,
        f.fingerprint
    );
}

#[test]
fn scope_and_language_decide_where_a_rule_runs() {
    let text = UNWRAP_RULE.replace(
        "languages = [\"rust\"]",
        "languages = [\"rust\"]\ninclude = [\"src/**\"]\nexclude = [\"src/gen/**\"]",
    );
    let rule = CompiledRule::compile(&spec(&text)).unwrap();
    let code = b"fn f() { a.unwrap(); }";
    let n = |p: &str| rule.run(&path(p), code, &MatchLimits::default()).len();
    assert_eq!(
        (n("src/a.rs"), n("src/gen/a.rs"), n("tests/a.rs"), n("src/a.py"), n("src/readme")),
        (1, 0, 0, 0, 0)
    );
}

#[test]
fn a_rule_that_cannot_run_is_refused_with_a_message() {
    let prose: RuleSpec = spec(
        "id = \"p\"\ntitle = \"T\"\nseverity = \"low\"\nfix = \"Do it.\"\ngood = [\"fn f() {}\"]\nbad = [\"fn f() { f(1); }\"]\ncheck = { markdown = \"handlers validate input\" }\n",
    );
    assert!(matches!(CompiledRule::compile(&prose), Err(PatternError::NotAPattern(_))));
    let none = spec(
        "id = \"p\"\ntitle = \"T\"\nseverity = \"low\"\nfix = \"Do it.\"\ngood = [\"fn f() {}\"]\nbad = [\"fn f() { f(1); }\"]\ncheck = { pattern = \"f($X)\" }\n",
    );
    assert!(matches!(CompiledRule::compile(&none), Err(PatternError::NoLanguage(_))));
    let no_fix = spec(
        "id = \"p\"\ntitle = \"T\"\nseverity = \"low\"\nlanguages = [\"rust\"]\ngood = [\"fn f() {}\"]\nbad = [\"fn f() { f(1); }\"]\ncheck = { pattern = \"f($X)\" }\n",
    );
    let err = CompiledRule::compile(&no_fix).err().unwrap();
    assert!(matches!(err, PatternError::MissingFix(_)) && err.to_string().contains("says in one sentence"));
    let json = spec(
        "id = \"p\"\ntitle = \"T\"\nseverity = \"low\"\nlanguages = [\"json\"]\nfix = \"Do it.\"\ngood = [\"{}\"]\nbad = [\"[]\"]\ncheck = { pattern = \"f($X)\" }\n",
    );
    assert!(matches!(
        CompiledRule::compile(&json),
        Err(PatternError::UnsupportedLanguage { .. })
    ));
    let bad = spec(&UNWRAP_RULE.replace("$X.unwrap()", "x.unwrap(("));
    assert!(matches!(CompiledRule::compile(&bad), Err(PatternError::Syntax { .. })));
    let long_fix = spec(&UNWRAP_RULE.replace("Return the error with `?` or handle it.", &"w ".repeat(400)));
    assert!(matches!(CompiledRule::compile(&long_fix), Err(PatternError::BadFix { .. })));
}

#[test]
fn a_rules_own_examples_are_its_test() {
    let rule = CompiledRule::compile(&spec(UNWRAP_RULE)).unwrap();
    let report = test_rule(&rule);
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.passed, 3);

    // a bad example that is missed, a good one that is flagged, and a rule with no bad example
    let weak = UNWRAP_RULE
        .replace("bad = [\"fn f() { a.unwrap(); }\"]", "bad = [\"fn f() { a.unwrap_or(0); }\"]")
        .replace("\"fn f() { a.unwrap_or(1); }\"", "\"fn f() { b.unwrap(); }\"");
    let report = test_rule(&CompiledRule::compile(&spec(&weak)).unwrap());
    let kinds: Vec<_> = report.failures.iter().map(|f| (f.kind, f.number)).collect();
    assert_eq!(kinds, [("bad", 1), ("good", 2)], "{:?}", report.failures);
    assert!(report.failures[0].problem.contains("does not flag") && report.failures[1].problem.contains("flags this"));
}
