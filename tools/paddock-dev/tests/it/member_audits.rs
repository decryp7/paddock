//! The member audits (`member-audits` of the push gate, run only in the member gate of `./dev merge check --gates`): the nextest profile
//! that declares them is read with a TOML parser (the format's own reader, ER-42), and every clause of its filter is matched against the
//! names of this binary's own tests (`--list`, no nested nextest: that waits on the build-directory lock the outer run holds, a 120 s
//! hang measured 2026-11-17). A clause that selects nothing (a renamed audit, a typo) fails here, and so does an exclusion that no longer
//! excludes anything (ER-33). The gate's wiring and verdicts are tested in `src/pre_push/tests.rs`; `--no-tests=fail` turns an empty
//! selection into a failure there.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::pre_push::gates::AUDIT_PROFILE;
use regex::Regex;
use std::path::Path;

/// The profile's `default-filter`, read through the TOML parser.
fn filter() -> String {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".config/nextest.toml")).unwrap();
    let doc: toml::Table = text.parse().unwrap();
    doc["profile"][AUDIT_PROFILE]["default-filter"]
        .as_str()
        .unwrap_or_else(|| panic!("profile {AUDIT_PROFILE} has no default-filter in .config/nextest.toml"))
        .to_string()
}

/// The `test(/regex/)` clauses of a filter, each with whether it is negated (`!test(...)`).
fn clauses(filter: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut rest = filter;
    while let Some(at) = rest.find("test(/") {
        let negated = rest[..at].ends_with('!');
        let body = &rest[at + "test(/".len()..];
        let end = body.find("/)").expect("a test(/regex/) clause is closed");
        out.push((negated, body[..end].to_string()));
        rest = &body[end + 2..];
    }
    out
}

/// Every alternative of a `^(a|b|c)::` module group on its own, so one missing module shows by name; other clauses whole.
fn parts(regex: &str) -> Vec<String> {
    let Some(open) = regex.strip_prefix("^(") else {
        return vec![regex.to_string()];
    };
    let Some((group, tail)) = open.split_once(")::") else {
        return vec![regex.to_string()];
    };
    if group.contains(['(', ')', '[']) {
        return vec![regex.to_string()];
    }
    group.split('|').map(|m| format!("^{m}::{tail}")).collect()
}

/// The names of the tests of this very binary (the `it` binary), as nextest names them (`module::test`).
fn names() -> Vec<String> {
    let exe = std::env::current_exe().unwrap();
    let out = paddock_proc::command(exe).args(["--list", "--format", "terse"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.strip_suffix(": test").map(str::to_string))
        .collect()
}

#[test]
fn every_clause_of_the_profile_selects_at_least_one_test_and_the_exclusion_excludes_one() {
    let f = filter();
    assert!(f.contains("binary(it)"), "the audits live in the `it` binary: {f}");
    let all = names();
    assert!(all.len() > 100, "the list of this binary's tests looks empty: {}", all.len());
    let found = clauses(&f);
    assert!(found.len() >= 8, "the filter was not parsed: {found:?}");
    for (negated, regex) in &found {
        for part in parts(regex) {
            let re = Regex::new(&part).unwrap_or_else(|e| panic!("`{part}` is not a regex: {e}"));
            let n = all.iter().filter(|name| re.is_match(name)).count();
            assert!(
                n > 0,
                "the {AUDIT_PROFILE} clause `{part}`{} matches none of the {} tests of this binary: it was renamed or removed (.config/nextest.toml)",
                if *negated { " (an exclusion)" } else { "" },
                all.len()
            );
        }
    }
    // The control that the list tells tests apart: the audits the review named are selected, plain unit-style modules are not.
    let selected = |name: &str| {
        let ok = found
            .iter()
            .filter(|(neg, _)| !neg)
            .any(|(_, r)| parts(r).iter().any(|p| Regex::new(p).unwrap().is_match(name)));
        let out = found
            .iter()
            .filter(|(neg, _)| *neg)
            .any(|(_, r)| Regex::new(r).unwrap().is_match(name));
        ok && !out
    };
    for audit in [
        "dev_bin::a_spawned_binary_has_the_wrapper_off",
        "cargo_target_lint::only_the_listed_files_set_the_build_folder_as_a_variable",
        "inputs::inputs_txt_matches_cargo_metadata",
    ] {
        assert!(
            all.iter().any(|n| n == audit) && selected(audit),
            "`{audit}` is not selected by the {AUDIT_PROFILE} profile"
        );
    }
    assert!(!selected(
        "golden::live_range_plans_match_affected_mjs_on_this_repositorys_first_parent_history"
    ));
    // A rename cannot silently drop a live or working-tree audit: every check module's `live_` and `this_working_tree_` test is selected.
    let audit_name = Regex::new(r"^check(_[a-z0-9]+)?::(live_|this_working_tree_)").unwrap();
    let check_audits: Vec<&String> = all.iter().filter(|n| audit_name.is_match(n)).collect();
    assert!(
        check_audits.len() >= 10,
        "the check modules' live and working-tree tests were not found: {check_audits:?}"
    );
    let dropped: Vec<&&String> = check_audits.iter().filter(|n| !selected(n.as_str())).collect();
    assert!(
        dropped.is_empty(),
        "these live or working-tree check tests are not selected by the {AUDIT_PROFILE} profile: {dropped:?}"
    );
    for plain in ["slots::", "locks::", "worktree::", "member_audits::"] {
        assert!(
            !all.iter().filter(|n| n.starts_with(plain)).any(|n| selected(n)),
            "`{plain}` is not an audit but is selected"
        );
    }
}

/// The `test(/regex/)` clauses of every override of `profile` that sets a `slow-timeout`.
fn budget_clauses(profile: &str) -> Vec<String> {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".config/nextest.toml")).unwrap();
    let doc: toml::Table = text.parse().unwrap();
    doc["profile"][profile]["overrides"]
        .as_array()
        .unwrap_or_else(|| panic!("profile {profile} has no overrides in .config/nextest.toml"))
        .iter()
        .filter(|o| o.get("slow-timeout").is_some())
        .filter_map(|o| o.get("filter").and_then(|f| f.as_str()))
        .flat_map(|f| clauses(f).into_iter().filter(|(neg, _)| !neg).map(|(_, r)| r))
        .collect()
}

/// Every check module's `live_` and `this_working_tree_` test reads the whole tree, and has the measured budget of that class in the
/// ci and the default profile: a new module's test is not left at the plain 60 s by its name (check_c122's live test was, and was stopped
/// at 60 s under load, 2026-11-18). The control: a plain unit-style test is not given that budget.
#[test]
fn every_live_or_working_tree_check_test_has_the_whole_tree_budget() {
    let all = names();
    let class = Regex::new(r"^check(_[a-z0-9]+)?::(live_|this_working_tree_)").unwrap();
    let members: Vec<&String> = all.iter().filter(|n| class.is_match(n)).collect();
    assert!(members.len() >= 10, "the class was not found: {members:?}");
    for profile in ["ci", "default"] {
        let budget: Vec<Regex> = budget_clauses(profile).iter().map(|r| Regex::new(r).unwrap()).collect();
        assert!(!budget.is_empty(), "no override of profile {profile} sets a slow-timeout");
        let covered = |n: &str| budget.iter().any(|r| r.is_match(n));
        let left: Vec<&&String> = members.iter().filter(|n| !covered(n)).collect();
        assert!(
            left.is_empty(),
            "profile {profile}: these whole-tree check tests have no whole-tree budget (.config/nextest.toml): {left:?}"
        );
        assert!(
            !covered("slots::a_held_slot_is_not_taken_twice"),
            "profile {profile} gives every test the budget"
        );
    }
}
