//! The OS-removal register (docs/os-removals.toml): its records and the verdict on each removal key.
use paddock_approvals::{Approver, Verdict, judge_os_removals, parse_os_removals};
use std::collections::BTreeSet;

const C: &str = "crates/paddock.ci.json";

fn register(by: &str, entries: &str) -> String {
    format!(
        "version = 1\n[[approved]]\ncontract = \"{C}\"\nby = \"{by}\"\ndate = \"2026-11-27\"\nwhy = \"w\"\nentries = [{entries}]\n"
    )
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn verdicts(text: &str, current: &[&str], added: &[&str]) -> Vec<(String, Verdict)> {
    judge_os_removals(
        &parse_os_removals(text).unwrap(),
        C,
        &set(current),
        &set(added),
    )
    .into_iter()
    .map(|v| (v.key, v.verdict))
    .collect()
}

#[test]
fn an_added_removal_the_user_approved_is_approved() {
    let v = verdicts(&register("user", "\"skip a\""), &["skip a"], &["skip a"]);
    assert_eq!(v, [("skip a".to_string(), Verdict::Approved)]);
}

#[test]
fn an_added_removal_nobody_approved_is_missing_and_another_contracts_approval_clears_nothing() {
    let text = register("user", "\"skip a\"").replace(C, "apps/x/paddock.ci.json");
    let v = verdicts(&text, &["skip a"], &["skip a"]);
    assert_eq!(v.len(), 1);
    let Verdict::Missing { line_to_add } = &v[0].1 else {
        panic!("{v:?}")
    };
    assert!(
        line_to_add.contains(C)
            && line_to_add.contains("entries = [\"skip a\"]")
            && line_to_add.contains("by = \"user\""),
        "{line_to_add}"
    );
}

#[test]
fn an_approval_of_the_coordinator_is_a_mismatch_for_an_os_removal() {
    let v = verdicts(
        &register("coordinator", "\"skip a\""),
        &["skip a"],
        &["skip a"],
    );
    assert!(
        matches!(&v[0].1, Verdict::Mismatch { why } if why.contains("needs the user")),
        "{v:?}"
    );
}

#[test]
fn an_approval_of_an_entry_the_merge_base_already_has_or_the_contract_lost_is_stale() {
    // The merge base has "skip a" (not added) and the contract still lists it; "skip gone" is in no contract any more.
    let v = verdicts(
        &register("user", "\"skip a\", \"skip gone\""),
        &["skip a"],
        &[],
    );
    assert_eq!(v.len(), 2);
    assert!(
        matches!(&v[0], (k, Verdict::Stale { why }) if k == "skip a" && why.contains("merge base already has it")),
        "{v:?}"
    );
    assert!(
        matches!(&v[1], (k, Verdict::Stale { why }) if k == "skip gone" && why.contains("has no such entry")),
        "{v:?}"
    );
}

#[test]
fn the_register_reads_its_records_and_lists_every_approved_key_for_its_own_shrink_only_check() {
    let r = parse_os_removals(&register("user", "\"skip a\", \"only macos b\"")).unwrap();
    assert_eq!(r.records[0].by, Approver::User);
    assert_eq!(r.records[0].contract, C);
    assert_eq!(
        r.keys(),
        [format!("{C} skip a"), format!("{C} only macos b")]
    );
    assert!(
        parse_os_removals("version = 1\n")
            .unwrap()
            .records
            .is_empty()
    );
}

#[test]
fn a_register_that_cannot_be_read_is_a_problem_naming_the_record() {
    let bad = |text: &str| parse_os_removals(text).unwrap_err().to_string();
    assert!(bad("= nope").contains("not valid TOML"));
    assert!(bad("version = 1\nextra = 1\n").contains("unknown key \"extra\""));
    assert!(bad("approved = 3").contains("approved is not an array of tables"));
    assert!(bad("approved = [3]").contains("approved #1 is not a table"));
    assert!(
        bad(&register("user", "\"a\"").replace("why = \"w\"\n", ""))
            .contains("approved #1 has no why")
    );
    assert!(bad(&register("agent", "\"a\"")).contains("neither the coordinator nor the user"));
    assert!(bad(&register("user", "1")).contains("not a string"));
    assert!(
        bad(&register("user", "\"a\"").replace("entries = [\"a\"]", "entries = 1"))
            .contains("has no entries")
    );
    assert!(
        bad(&format!("{}other = 1\n", register("user", "\"a\""))).contains("unknown key \"other\"")
    );
}
