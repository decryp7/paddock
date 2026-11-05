//! Baselines and fingerprint ordinals.

use std::collections::BTreeMap;

use pacenotes_core::Severity;
use pacenotes_rules::shared_copy::{Baseline, BaselineEntry, BaselineError, IsoDate, assign_ordinals, compare};

use crate::support::{finding, rule};

fn entry_of(f: &pacenotes_rules::shared_copy::Finding, ordinal: u32) -> BaselineEntry {
    BaselineEntry {
        rule: f.rule.clone(),
        rule_version: 1,
        fingerprint: f.fingerprint.unwrap(),
        ordinal,
        first_seen: IsoDate::parse("2026-10-01").unwrap(),
    }
}

#[test]
fn identical_findings_in_a_file_are_numbered_in_order() {
    let a = finding("r", Severity::High, "a.rs", 1, "x.unwrap()");
    let b = finding("r", Severity::High, "a.rs", 9, "x.unwrap()");
    let c = finding("r", Severity::High, "a.rs", 12, "y.unwrap()");
    let mut d = finding("s", Severity::High, "a.rs", 14, "x.unwrap()");
    d.fingerprint = None;
    assert_eq!(assign_ordinals(&[a, b, c, d]), [1, 2, 1, 0]);
}

#[test]
fn only_new_findings_are_new_and_a_moved_file_is_not() {
    let old = finding("r", Severity::High, "a.rs", 1, "x.unwrap()");
    let baseline = Baseline {
        entries: vec![entry_of(&old, 1)],
    };
    // the same finding in a file that moved: same identity, known
    let moved = finding("r", Severity::High, "deeper/a.rs", 40, "x.unwrap()");
    let fresh = finding("r", Severity::High, "a.rs", 7, "other()");
    let got = compare(&[moved, fresh], &baseline, &BTreeMap::new());
    assert_eq!((got.known, got.updated, got.new), (vec![0], vec![], vec![1]));
}

/// Per (rule, fingerprint) the baseline holds `n` entries: the first `n` findings are known or updated, any beyond are new.
#[test]
fn identical_findings_beyond_the_recorded_count_are_new_and_a_shifted_ordinal_is_updated() {
    let same = |line| finding("r", Severity::High, "a.rs", line, "x.unwrap()");
    let one = Baseline {
        entries: vec![entry_of(&same(1), 1)],
    };
    let got = compare(&[same(3), same(9), same(12)], &one, &BTreeMap::new());
    assert_eq!(
        (got.known, got.updated, got.new),
        (vec![0], vec![], vec![1, 2]),
        "a copy-pasted finding is not absorbed"
    );
    // the baseline recorded the second of two; now only one is left, and it is the first
    let second = Baseline {
        entries: vec![entry_of(&same(1), 2)],
    };
    let got = compare(&[same(3)], &second, &BTreeMap::new());
    assert_eq!((got.known.clone(), got.updated.clone(), got.new.clone()), (vec![], vec![0], vec![]));
    let states = got.states(2);
    assert_eq!(states[0].map(|s| s.sarif_name()), Some("updated"));
    assert_eq!(states[1], None);
}

#[test]
fn a_renamed_rule_keeps_its_baseline_through_the_alias() {
    let old = finding("old-name", Severity::High, "a.rs", 1, "x");
    let baseline = Baseline {
        entries: vec![BaselineEntry {
            fingerprint: finding("new-name", Severity::High, "a.rs", 1, "x").fingerprint.unwrap(),
            ..entry_of(&old, 1)
        }],
    };
    let now = finding("new-name", Severity::High, "a.rs", 1, "x");
    // the baseline recorded the old id, the fingerprint is the new rule's
    let aliases = BTreeMap::from([(rule("old-name"), rule("new-name"))]);
    assert_eq!(compare(std::slice::from_ref(&now), &baseline, &aliases).known, [0]);
    assert_eq!(compare(&[now], &baseline, &BTreeMap::new()).new, [0]);
}

#[test]
fn a_baseline_round_trips_and_a_bad_one_is_refused_with_a_message() {
    let f = finding("clippy-x", Severity::Low, "a.rs", 1, "x");
    let mut ext = f.clone();
    ext.rule = pacenotes_rules::shared_copy::RuleId::external("clippy", "clippy::needless_return").unwrap();
    let b = Baseline {
        entries: vec![entry_of(&f, 1), entry_of(&ext, 2)],
    };
    let text = b.to_toml();
    assert_eq!(Baseline::from_toml(&text).unwrap(), b);
    assert!(matches!(
        Baseline::from_toml("version = 2\nalgorithm = 1\n"),
        Err(BaselineError::Version { version: 2, .. })
    ));
    assert!(matches!(
        Baseline::from_toml("version = 1\nalgorithm = 9\n"),
        Err(BaselineError::Version { algorithm: 9, .. })
    ));
    assert!(Baseline::from_toml("version = 1\nalgorithm = 1\n[[finding]]\nrule = \"r\"\nrule_version = 1\nfingerprint = \"zz\"\nordinal = 1\nfirst_seen = \"2026-10-01\"\n")
        .unwrap_err()
        .to_string()
        .contains("64 hex digits"));
    assert!(Baseline::from_toml("not toml [").is_err());
    assert!(Baseline::from_toml(&"#".repeat(9 * 1024 * 1024)).is_err());
}

#[test]
fn a_baseline_that_lists_a_finding_twice_is_refused_with_a_message() {
    let f = finding("r", Severity::Low, "a.rs", 1, "x");
    let text = Baseline {
        entries: vec![entry_of(&f, 1), entry_of(&f, 1)],
    }
    .to_toml();
    let err = Baseline::from_toml(&text).unwrap_err().to_string();
    assert!(err.contains("finding number 2") && err.contains("listed twice"), "{err}");
    assert!(
        Baseline::from_toml(
            &Baseline {
                entries: vec![entry_of(&f, 1), entry_of(&f, 2)]
            }
            .to_toml()
        )
        .is_ok()
    );
}
