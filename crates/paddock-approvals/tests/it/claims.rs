//! The claims of the kinds that carry one word (an npm audit exception, the active apps, a date-move trailer) judged by the one
//! verdict (user decision 125).
use paddock_approvals::{Approver, Claim, Verdict, date_move_claim};

#[test]
fn a_word_is_read_into_a_claim() {
    assert_eq!(Claim::from_word(None), Claim::Absent);
    assert_eq!(Claim::from_word(Some("user")), Claim::By(Approver::User));
    assert_eq!(
        Claim::from_word(Some(" coordinator ")),
        Claim::By(Approver::Coordinator)
    );
    assert_eq!(
        Claim::from_word(Some("someone")),
        Claim::Unrecognised("someone".into())
    );
    assert_eq!(
        Claim::from_word(Some("")),
        Claim::Absent,
        "an empty word names nobody"
    );
}

#[test]
fn a_claim_is_judged_against_who_must_give_it() {
    let line = "approvedBy = \"user\"";
    assert_eq!(
        Claim::By(Approver::User).verdict("approvedBy", Approver::User, line),
        Verdict::Approved
    );
    assert_eq!(
        Claim::By(Approver::User).verdict("approvedBy", Approver::Coordinator, line),
        Verdict::Approved
    );
    assert_eq!(
        Claim::By(Approver::Coordinator).verdict("approvedBy", Approver::Coordinator, line),
        Verdict::Approved
    );
    assert!(matches!(
        Claim::By(Approver::Coordinator).verdict("approvedBy", Approver::User, line),
        Verdict::Mismatch { why } if why.contains("approvedBy") && why.contains("needs the user")
    ));
    assert!(matches!(
        Claim::Unrecognised("someone".into()).verdict("approvedBy", Approver::User, line),
        Verdict::Mismatch { why } if why.contains("someone")
    ));
    assert_eq!(
        Claim::Absent.verdict("approvedBy", Approver::User, line),
        Verdict::Missing {
            line_to_add: line.into()
        }
    );
}

const T: &str = "Gates-date: C13 2026-12-06 -> 2026-12-20; approved: coordinator";

#[test]
fn a_date_move_trailer_names_exactly_its_move_and_its_approver() {
    let claim = |trailers: &str, what: &str| date_move_claim(trailers, "Gates-date", what);
    assert_eq!(
        claim(T, "C13 2026-12-06 -> 2026-12-20"),
        Claim::By(Approver::Coordinator)
    );
    assert_eq!(
        claim(
            "Gates-date: C13 2026-12-06 -> 2026-12-20; approved: user",
            "C13 2026-12-06 -> 2026-12-20"
        ),
        Claim::By(Approver::User)
    );
    // git's tokens are case-insensitive
    assert_eq!(
        claim(
            &T.replace("Gates-date", "gates-date"),
            "C13 2026-12-06 -> 2026-12-20"
        ),
        Claim::By(Approver::Coordinator)
    );
    // another move, another id, no trailer at all
    assert_eq!(claim(T, "C13 2026-12-06 -> 2027-01-10"), Claim::Absent);
    assert_eq!(claim(T, "C130 2026-12-06 -> 2026-12-20"), Claim::Absent);
    assert_eq!(
        claim("Signed-off-by: x", "C13 2026-12-06 -> 2026-12-20"),
        Claim::Absent
    );
    assert_eq!(claim("", "C13 2026-12-06 -> 2026-12-20"), Claim::Absent);
    // the approver word is exact: text after it, or another word, approves nothing
    assert!(matches!(
        claim(&format!("{T} maybe"), "C13 2026-12-06 -> 2026-12-20"),
        Claim::Unrecognised(_)
    ));
    assert!(matches!(
        claim(
            &T.replace("coordinator", "somebody"),
            "C13 2026-12-06 -> 2026-12-20"
        ),
        Claim::Unrecognised(_)
    ));
    // another key is another trailer
    assert_eq!(
        date_move_claim(T, "Copies-date", "C13 2026-12-06 -> 2026-12-20"),
        Claim::Absent
    );
}

#[test]
fn the_strongest_of_several_trailers_counts() {
    let both = format!("{T}\nGates-date: C13 2026-12-06 -> 2026-12-20; approved: user");
    assert_eq!(
        date_move_claim(&both, "Gates-date", "C13 2026-12-06 -> 2026-12-20"),
        Claim::By(Approver::User)
    );
    let bad_and_good = format!("{T}\nGates-date: C13 2026-12-06 -> 2026-12-20; approved: nobody");
    assert_eq!(
        date_move_claim(&bad_and_good, "Gates-date", "C13 2026-12-06 -> 2026-12-20"),
        Claim::By(Approver::Coordinator)
    );
}
