//! The `approved` field of the register's entries, and the branch-adds comparison.
use paddock_approvals::{
    Approver, Claim, ENTRY_TABLES, Severity, Stage, Verdict, judge_entries, parse_gates,
};

fn entry(table: &str, id: &str, approved: Option<&str>) -> String {
    let approved = approved
        .map(|a| format!("approved = {a}\n"))
        .unwrap_or_default();
    format!("[[{table}]]\nid = \"{id}\"\nreason = \"r\"\n{approved}\n")
}

fn register(entries: &[String]) -> String {
    format!("version = 1\n\n{}", entries.concat())
}

#[test]
fn every_kind_of_entry_reads_its_approved_field_and_says_who_must_give_it() {
    let text = register(&[
        entry("report_only", "C1", Some("\"coordinator\"")),
        entry("not_gated", "C2", Some("\"user\"")),
        entry("ratchet", "C3", Some("\"user\"")),
        entry("level", "C4", None),
        entry("planned", "C5", Some("7")),
    ]);
    let g = parse_gates(&text).unwrap();
    let by_id = |id: &str| {
        g.entries
            .iter()
            .find(|e| e.identity == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    assert_eq!(by_id("C1").claim, Claim::By(Approver::Coordinator));
    assert_eq!(by_id("C1").verdict(), Verdict::Approved);
    assert_eq!(by_id("C2").verdict(), Verdict::Approved);
    assert_eq!(by_id("C3").needs, Approver::User);
    assert_eq!(by_id("C3").verdict(), Verdict::Approved);
    assert_eq!(by_id("C4").claim, Claim::Absent);
    assert_eq!(
        by_id("C4").verdict(),
        Verdict::Missing {
            line_to_add: "approved = \"coordinator\"".into()
        }
    );
    assert!(matches!(by_id("C5").claim, Claim::Unrecognised(_)));
    assert!(matches!(by_id("C5").verdict(), Verdict::Mismatch { .. }));
    assert_eq!(by_id("C1").key(), "[[report_only]] C1");
}

#[test]
fn a_ratchet_needs_the_user_and_an_unknown_approver_is_a_mismatch() {
    let g = parse_gates(&register(&[
        entry("ratchet", "C3", Some("\"coordinator\"")),
        entry("ratchet", "C6", None),
        entry("report_only", "C7", Some("\"agent\"")),
    ]))
    .unwrap();
    let why = |id: &str| match g
        .entries
        .iter()
        .find(|e| e.identity == id)
        .unwrap()
        .verdict()
    {
        Verdict::Mismatch { why } => why,
        Verdict::Missing { line_to_add } => line_to_add,
        v => panic!("{v:?}"),
    };
    assert!(why("C3").contains("needs the user"));
    assert_eq!(why("C6"), "approved = \"user\"");
    assert!(why("C7").contains("must be \"coordinator\" or \"user\""));
}

#[test]
fn entries_of_one_table_are_told_apart_by_id_apps_and_path() {
    let g = parse_gates(
        "[[not_gated]]\nid = \"C13\"\napps = [\"dyno\", \"pitcrew\"]\n[[not_gated]]\nid = \"C13\"\napps = [\"debrief\"]\n[[ignored]]\npath = \"a.rs\"\ntests = [\"t1\"]\n[[ratchet]]\nid = \"C13\"\napp = \"debrief\"\n",
    )
    .unwrap();
    let keys: Vec<String> = g.entries.iter().map(|e| e.key()).collect();
    assert_eq!(
        keys,
        [
            "[[not_gated]] C13 dyno,pitcrew",
            "[[not_gated]] C13 debrief",
            "[[ignored]] a.rs t1",
            "[[ratchet]] C13 debrief"
        ]
    );
    assert_eq!(g.entry_keys().len(), 4);
}

#[test]
fn a_branch_that_adds_an_entry_without_approval_warns_at_a_push_and_fails_the_member_gate() {
    let base = parse_gates(&register(&[entry(
        "report_only",
        "C1",
        Some("\"coordinator\""),
    )]))
    .unwrap();
    let now = parse_gates(&register(&[
        entry("report_only", "C1", Some("\"coordinator\"")),
        entry("report_only", "C178", None),
    ]))
    .unwrap();
    let severities = |stage| {
        judge_entries(&now, Some(&base), stage)
            .iter()
            .map(|j| (j.subject.clone(), j.severity, j.added_by_branch))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        severities(Stage::BranchPush),
        [("[[report_only]] C178".to_string(), Severity::Warning, true)]
    );
    assert_eq!(
        severities(Stage::MemberGate),
        [("[[report_only]] C178".to_string(), Severity::Error, true)]
    );
    assert_eq!(
        severities(Stage::Landing),
        [("[[report_only]] C178".to_string(), Severity::Error, true)]
    );
}

#[test]
fn an_entry_main_already_carries_without_approval_fails_at_every_stage_and_so_does_an_unreadable_base()
 {
    let both = parse_gates(&register(&[entry("report_only", "C1", None)])).unwrap();
    for stage in [Stage::BranchPush, Stage::MemberGate, Stage::Landing] {
        let on_main = judge_entries(&both, Some(&both), stage);
        assert_eq!(
            (
                on_main.len(),
                on_main[0].severity,
                on_main[0].added_by_branch
            ),
            (1, Severity::Error, false),
            "{stage:?}"
        );
        // The base cannot be read: nothing is the branch's, so nothing is lenient.
        let unknown = judge_entries(&both, None, stage);
        assert_eq!(
            (unknown.len(), unknown[0].severity),
            (1, Severity::Error),
            "{stage:?}"
        );
    }
}

#[test]
fn an_approved_entry_is_no_finding_and_a_register_that_is_not_a_register_is_a_problem() {
    let ok = parse_gates(&register(&[entry("report_only", "C1", Some("\"user\""))])).unwrap();
    assert!(judge_entries(&ok, None, Stage::Landing).is_empty());
    assert!(
        parse_gates("= nope")
            .unwrap_err()
            .to_string()
            .contains("not valid TOML")
    );
    assert!(
        parse_gates("report_only = 3")
            .unwrap_err()
            .to_string()
            .contains("[[report_only]] is not an array of tables")
    );
    assert!(
        parse_gates("report_only = [3]")
            .unwrap_err()
            .to_string()
            .contains("not a table")
    );
    assert_eq!(parse_gates("version = 1\n").unwrap(), Default::default());
    assert_eq!(
        ENTRY_TABLES
            .iter()
            .filter(|(_, n)| *n == Approver::User)
            .count(),
        1,
        "only the ratchet needs the user"
    );
}
