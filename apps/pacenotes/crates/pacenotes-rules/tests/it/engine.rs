//! Waivers and suppressions applied to findings.

use std::collections::{BTreeMap, BTreeSet};

use pacenotes_core::Severity;
use pacenotes_rules::shared_copy::{HiddenBy, IsoDate, Origin, Reason, RuleId, Suppression, SuppressionScope, Waiver, apply_waivers};

use crate::support::{finding, rule};

fn date(s: &str) -> IsoDate {
    IsoDate::parse(s).unwrap()
}

fn waiver(id: &str, glob: &str, expires: Option<&str>, max: Option<u32>) -> Waiver {
    Waiver {
        rule: rule(id),
        path: glob.into(),
        reason: Reason::parse("legacy").unwrap(),
        expires: expires.map(date),
        max_count: max,
        owner: None,
        origin: Origin::File,
    }
}

fn ran(ids: &[&str]) -> BTreeSet<RuleId> {
    ids.iter().map(|i| rule(i)).collect()
}

fn ids(app: &pacenotes_rules::shared_copy::Applied) -> Vec<String> {
    app.engine.iter().map(|f| f.rule.to_string()).collect()
}

#[test]
fn a_live_waiver_hides_what_it_covers_and_the_expiry_day_is_inclusive() {
    let findings = vec![
        finding("r", Severity::High, "src/a.rs", 1, "x"),
        finding("r", Severity::High, "tests/a.rs", 1, "x"),
        finding("s", Severity::High, "src/b.rs", 1, "y"),
    ];
    let w = [waiver("r", "src/**", Some("2026-10-30"), None)];
    let on_the_day = apply_waivers(findings.clone(), &w, &[], &ran(&["r", "s"]), &BTreeMap::new(), date("2026-10-30"));
    assert_eq!((on_the_day.kept.len(), on_the_day.hidden.len()), (2, 1));
    assert_eq!(on_the_day.hidden[0].by, HiddenBy::Waiver(0));
    assert!(on_the_day.engine.is_empty());
    let next_day = apply_waivers(findings, &w, &[], &ran(&["r", "s"]), &BTreeMap::new(), date("2026-10-31"));
    assert_eq!(
        (next_day.kept.len(), next_day.hidden.len()),
        (3, 0),
        "an expired waiver waives nothing"
    );
    assert_eq!(ids(&next_day), ["w001"]);
}

#[test]
fn a_file_waiver_without_an_expiry_is_incomplete_but_a_config_waiver_needs_none() {
    let findings = vec![finding("r", Severity::High, "src/a.rs", 1, "x")];
    let mut file = waiver("r", "src/**", None, None);
    let app = apply_waivers(
        findings.clone(),
        std::slice::from_ref(&file),
        &[],
        &ran(&["r"]),
        &BTreeMap::new(),
        date("2026-10-30"),
    );
    assert_eq!((ids(&app), app.hidden.len()), (vec!["w002".to_owned()], 0));
    file.origin = Origin::Config;
    let app = apply_waivers(findings, &[file], &[], &ran(&["r"]), &BTreeMap::new(), date("2026-10-30"));
    assert_eq!((app.hidden.len(), app.engine.len()), (1, 0));
}

#[test]
fn a_counted_waiver_over_its_ceiling_hides_nothing_and_under_it_suggests_lowering() {
    let many: Vec<_> = (1..=3)
        .map(|l| finding("r", Severity::High, "src/a.rs", l, &format!("x{l}")))
        .collect();
    let over = apply_waivers(
        many.clone(),
        &[waiver("r", "src/**", Some("2027-01-01"), Some(2))],
        &[],
        &ran(&["r"]),
        &BTreeMap::new(),
        date("2026-10-30"),
    );
    assert_eq!((over.kept.len(), over.hidden.len(), ids(&over)), (3, 0, vec!["w004".to_owned()]));
    assert!(over.engine[0].message.contains("may hide 2 findings and covers 3"));
    let under = apply_waivers(
        many.clone(),
        &[waiver("r", "src/**", Some("2027-01-01"), Some(5))],
        &[],
        &ran(&["r"]),
        &BTreeMap::new(),
        date("2026-10-30"),
    );
    assert_eq!((under.hidden.len(), ids(&under)), (3, vec!["w005".to_owned()]));
    assert_eq!(under.engine[0].severity, Severity::Info);
    assert!(under.engine[0].fix.as_str().contains("Lower the ceiling to 3"));
    let exact = apply_waivers(
        many,
        &[waiver("r", "src/**", Some("2027-01-01"), Some(3))],
        &[],
        &ran(&["r"]),
        &BTreeMap::new(),
        date("2026-10-30"),
    );
    assert_eq!((exact.hidden.len(), exact.engine.len()), (3, 0));
}

#[test]
fn an_unused_waiver_is_reported_only_for_rules_that_ran() {
    let none: Vec<_> = Vec::new();
    let w = [waiver("r", "src/**", Some("2027-01-01"), None)];
    let ran_it = apply_waivers(none.clone(), &w, &[], &ran(&["r"]), &BTreeMap::new(), date("2026-10-30"));
    assert_eq!(ids(&ran_it), ["w003"]);
    assert_eq!(ran_it.engine[0].severity, Severity::Medium);
    let skipped = apply_waivers(none, &w, &[], &ran(&["other"]), &BTreeMap::new(), date("2026-10-30"));
    assert!(skipped.engine.is_empty(), "a rule that did not run cannot be judged");
}

#[test]
fn a_deprecated_rule_id_still_matches_through_the_alias() {
    let findings = vec![finding("new-name", Severity::High, "src/a.rs", 1, "x")];
    let aliases = BTreeMap::from([(rule("old-name"), rule("new-name"))]);
    let app = apply_waivers(
        findings,
        &[waiver("old-name", "src/**", Some("2027-01-01"), None)],
        &[],
        &ran(&["new-name"]),
        &aliases,
        date("2026-10-30"),
    );
    assert_eq!((app.hidden.len(), app.engine.len()), (1, 0));
}

#[test]
fn suppressions_cover_a_line_a_file_a_glob_or_a_rule_and_need_not_be_expired() {
    let reason = Reason::parse("checked above").unwrap();
    let s = |scope, rule_id: Option<&str>, expires: Option<&str>| Suppression {
        scope,
        rule: rule_id.map(rule),
        reason: reason.clone(),
        expires: expires.map(date),
    };
    let findings = vec![
        finding("r", Severity::High, "src/a.rs", 5, "a"),
        finding("r", Severity::High, "src/a.rs", 9, "b"),
        finding("r", Severity::High, "src/b.rs", 1, "c"),
        finding("r", Severity::High, "lib/c.rs", 1, "d"),
        finding("t", Severity::High, "x/y.rs", 1, "e"),
    ];
    let sup = [
        s(
            SuppressionScope::Line {
                path: "src/a.rs".into(),
                line: 5,
            },
            Some("r"),
            None,
        ),
        s(SuppressionScope::File { path: "src/b.rs".into() }, None, None),
        s(SuppressionScope::Path { glob: "lib/**".into() }, Some("r"), Some("2026-10-30")),
        s(SuppressionScope::Everywhere, Some("t"), Some("2026-01-01")),
    ];
    let app = apply_waivers(findings, &[], &sup, &ran(&["r", "t"]), &BTreeMap::new(), date("2026-10-30"));
    let hidden: Vec<_> = app.hidden.iter().map(|h| h.by).collect();
    assert_eq!(
        hidden,
        [HiddenBy::Suppression(0), HiddenBy::Suppression(1), HiddenBy::Suppression(2)]
    );
    assert_eq!(app.kept.len(), 2, "line 9 and the finding of the rule whose suppression expired");
    assert_eq!(ids(&app), ["w001"]);
}

#[test]
fn an_unused_suppression_is_reported_when_its_rule_ran() {
    let s = Suppression {
        scope: SuppressionScope::File { path: "nowhere.rs".into() },
        rule: Some(rule("r")),
        reason: Reason::parse("x").unwrap(),
        expires: None,
    };
    let app = apply_waivers(
        Vec::new(),
        &[],
        std::slice::from_ref(&s),
        &ran(&["r"]),
        &BTreeMap::new(),
        date("2026-10-30"),
    );
    assert_eq!(ids(&app), ["w003"]);
    let app = apply_waivers(Vec::new(), &[], &[s], &ran(&[]), &BTreeMap::new(), date("2026-10-30"));
    assert!(app.engine.is_empty());
}
