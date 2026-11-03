//! Counts and exit codes.

use pacenotes_core::Severity;
use pacenotes_rules::shared_copy::{Report, Skipped, exit_code};

use crate::support::{finding, rule};

fn report(findings: Vec<pacenotes_rules::shared_copy::Finding>, skipped: Vec<Skipped>) -> Report {
    Report {
        scope: "repo".into(),
        tool: "pacenotes".into(),
        tool_version: "0.1.0".into(),
        rules: Vec::new(),
        findings,
        hidden: Vec::new(),
        skipped,
    }
}

#[test]
fn counts_group_by_severity() {
    let r = report(
        vec![
            finding("a", Severity::High, "x.rs", 1, "1"),
            finding("b", Severity::High, "x.rs", 2, "2"),
            finding("c", Severity::Info, "x.rs", 3, "3"),
        ],
        Vec::new(),
    );
    let c = r.counts();
    assert_eq!(
        (c.by_severity[&Severity::High], c.by_severity[&Severity::Info], c.hidden),
        (2, 1, 0)
    );
}

#[test]
fn exit_codes_never_call_cut_short_work_clean() {
    let clean = report(Vec::new(), Vec::new());
    assert_eq!(exit_code(&clean, Severity::High, false, false), 0);
    let low = report(vec![finding("a", Severity::Low, "x.rs", 1, "1")], Vec::new());
    assert_eq!(exit_code(&low, Severity::High, false, false), 0, "below the threshold");
    assert_eq!(exit_code(&low, Severity::Low, false, false), 1);
    assert_eq!(exit_code(&clean, Severity::High, true, false), 3, "partial");
    let skipped = report(
        Vec::new(),
        vec![Skipped {
            rule: rule("prose"),
            why: "needs the local model".into(),
        }],
    );
    assert_eq!(
        exit_code(&skipped, Severity::High, false, false),
        3,
        "a skipped rule is not a clean run"
    );
    assert_eq!(exit_code(&low, Severity::Low, true, false), 1, "findings come first");
    assert_eq!(exit_code(&low, Severity::Low, true, true), 2, "an error comes first of all");
}
