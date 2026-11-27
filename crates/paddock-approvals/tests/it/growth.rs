//! The verdicts on growth approvals.
use paddock_approvals::{
    Approver, Claim, GatesApprovals, GrowthRecord, Verdict, growth_digest, growth_line,
    judge_growth, parse_gates,
};

fn two() -> Vec<String> {
    vec![
        "tools/x.json skip b::".to_string(),
        "tools/x.json skip a::".to_string(),
    ]
}

fn records(check: &str, count: i64, digest: &str, by: &str) -> Vec<GrowthRecord> {
    let text = format!(
        "version = 1\n[[register_growth]]\ncheck = \"{check}\"\nby = \"{by}\"\ndate = \"2026-11-27\"\ncount = {count}\ndigest = \"{digest}\"\n"
    );
    parse_gates(&text).expect("a well-formed register").growth
}

fn mismatch(v: Verdict) -> String {
    match v {
        Verdict::Mismatch { why } => why,
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

#[test]
fn a_matching_approval_of_the_user_is_approved_whatever_the_order_of_the_additions() {
    let r = records("CX", 2, &growth_digest(&two()), "user");
    assert_eq!(judge_growth(&r, "CX", &two()), Verdict::Approved);
}

#[test]
fn growth_without_a_record_of_its_check_is_missing_and_names_the_line_to_add() {
    let digest = growth_digest(&two());
    // Another check's record clears nothing.
    let other = records("CY", 2, &digest, "user");
    for r in [&[][..], &other[..]] {
        match judge_growth(r, "CX", &two()) {
            Verdict::Missing { line_to_add } => {
                assert!(
                    line_to_add.contains("check = \"CX\"")
                        && line_to_add.contains("count = 2")
                        && line_to_add.contains(&digest),
                    "{line_to_add}"
                );
                assert_eq!(line_to_add, growth_line("CX", &two()));
            }
            v => panic!("{v:?}"),
        }
    }
}

#[test]
fn a_record_that_does_not_match_the_additions_is_a_mismatch_that_says_why() {
    let digest = growth_digest(&two());
    // One more addition than the user saw: the count and the digest differ.
    let mut more = two();
    more.push("tools/x.json skip c::".into());
    assert!(
        mismatch(judge_growth(
            &records("CX", 2, &digest, "user"),
            "CX",
            &more
        ))
        .contains("`count` is not 3")
    );
    // A wrong count with the right digest.
    assert!(
        mismatch(judge_growth(
            &records("CX", 3, &digest, "user"),
            "CX",
            &two()
        ))
        .contains("`count` is not 2")
    );
    // The same count with other content.
    let mut other = two();
    other[0] = "tools/x.json skip z::".into();
    assert!(
        mismatch(judge_growth(
            &records("CX", 2, &digest, "user"),
            "CX",
            &other
        ))
        .contains("`digest` is not")
    );
    // Not the user's.
    assert!(
        mismatch(judge_growth(
            &records("CX", 2, &digest, "coordinator"),
            "CX",
            &two()
        ))
        .contains("`by = \"user\"`")
    );
    // No date.
    let undated = vec![GrowthRecord {
        date: None,
        ..records("CX", 2, &digest, "user").remove(0)
    }];
    assert!(mismatch(judge_growth(&undated, "CX", &two())).contains("no `date`"));
}

#[test]
fn a_record_whose_additions_are_already_on_the_target_branch_is_stale() {
    let r = records("CX", 2, &growth_digest(&two()), "user");
    assert!(matches!(judge_growth(&r, "CX", &[]), Verdict::Stale { .. }));
    // Nothing added and no record: nothing to approve.
    assert_eq!(judge_growth(&[], "CX", &[]), Verdict::Approved);
    // Another check's record is not this check's to call stale.
    assert_eq!(judge_growth(&r, "CY", &[]), Verdict::Approved);
}

#[test]
fn the_digest_is_sha256_of_the_sorted_keys_joined_by_a_newline() {
    // The oracle is a known SHA-256 (of "a\nb"), not this crate's code.
    assert_eq!(
        growth_digest(&["b".to_string(), "a".to_string()]),
        "7e18f737311b2dc3b2f269dd78396b0351f14fb66efa879f768cb23181883c78"
    );
}

#[test]
fn a_record_is_read_as_written_and_a_broken_register_approves_nothing() {
    let r = records("CX", 2, "d", "user");
    assert_eq!(r[0].by, Claim::By(Approver::User));
    assert_eq!((r[0].count, r[0].digest.as_deref()), (Some(2), Some("d")));
    // A record without a check names nothing.
    assert!(
        parse_gates("[[register_growth]]\nby = \"user\"\n")
            .unwrap()
            .growth
            .is_empty()
    );
    let broken: Result<GatesApprovals, _> = parse_gates("not toml [");
    assert!(broken.is_err());
    assert_eq!(
        judge_growth(&[], "CX", &two()),
        Verdict::Missing {
            line_to_add: growth_line("CX", &two())
        }
    );
}
