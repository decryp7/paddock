//! The stage policy, every verdict at every stage.
use paddock_approvals::{Severity, Stage, Verdict, severity};

const MISSING: fn() -> Verdict = || Verdict::Missing {
    line_to_add: "x".into(),
};
const MISMATCH: fn() -> Verdict = || Verdict::Mismatch { why: "x".into() };
const STALE: fn() -> Verdict = || Verdict::Stale { why: "x".into() };

#[test]
fn a_missing_approval_on_an_entry_the_branch_adds_warns_at_a_push_and_fails_the_member_gate_and_landing()
 {
    assert_eq!(
        severity(Stage::BranchPush, &MISSING(), true),
        Some(Severity::Warning)
    );
    assert_eq!(
        severity(Stage::MemberGate, &MISSING(), true),
        Some(Severity::Error)
    );
    assert_eq!(
        severity(Stage::Landing, &MISSING(), true),
        Some(Severity::Error)
    );
}

#[test]
fn a_missing_approval_on_an_entry_the_branch_did_not_add_fails_at_every_stage() {
    for stage in [Stage::BranchPush, Stage::MemberGate, Stage::Landing] {
        assert_eq!(
            severity(stage, &MISSING(), false),
            Some(Severity::Error),
            "{stage:?}"
        );
    }
}

#[test]
fn stale_warns_on_a_branch_and_is_a_finding_at_landing() {
    assert_eq!(
        severity(Stage::BranchPush, &STALE(), false),
        Some(Severity::Warning)
    );
    assert_eq!(
        severity(Stage::MemberGate, &STALE(), false),
        Some(Severity::Warning)
    );
    assert_eq!(
        severity(Stage::Landing, &STALE(), false),
        Some(Severity::Error)
    );
}

#[test]
fn a_mismatch_is_an_error_at_every_stage_and_approved_is_nothing() {
    for stage in [Stage::BranchPush, Stage::MemberGate, Stage::Landing] {
        for added in [true, false] {
            assert_eq!(
                severity(stage, &MISMATCH(), added),
                Some(Severity::Error),
                "{stage:?} {added}"
            );
            assert_eq!(
                severity(stage, &Verdict::Approved, added),
                None,
                "{stage:?} {added}"
            );
        }
    }
}

#[test]
fn a_stage_has_a_name_that_parses_back_and_a_caller_that_says_nothing_gets_the_strictest() {
    for s in [Stage::BranchPush, Stage::MemberGate, Stage::Landing] {
        assert_eq!(Stage::parse(s.name()), Some(s));
        assert_eq!(Stage::or_strictest(Some(s.name())), Ok(s));
    }
    assert_eq!(Stage::or_strictest(None), Ok(Stage::Landing));
    assert_eq!(Stage::or_strictest(Some("  ")), Ok(Stage::Landing));
    assert!(
        Stage::or_strictest(Some("lenient"))
            .unwrap_err()
            .contains("not a stage")
    );
    assert_eq!(Stage::parse("lenient"), None);
}

#[test]
fn a_ref_names_its_stage_and_anything_unknown_is_the_strictest() {
    for (r, want) in [
        ("refs/heads/trial/feat-x", Stage::MemberGate),
        ("refs/heads/train/2026-11-27-z", Stage::MemberGate),
        ("refs/heads/main", Stage::Landing),
        ("refs/tags/v1.2.3", Stage::Landing),
        ("refs/tags/trial/not-a-branch", Stage::Landing),
        ("refs/heads/feat/paddock-approvals", Stage::Landing),
        ("refs/heads/trainer", Stage::Landing),
        ("refs/heads/train", Stage::Landing),
        ("refs/pull/1/merge", Stage::Landing),
        ("", Stage::Landing),
    ] {
        assert_eq!(Stage::from_ref(r), want, "{r:?}");
    }
    assert_eq!(
        Stage::from_ref(" refs/heads/train/x "),
        Stage::MemberGate,
        "whitespace is trimmed"
    );
}
