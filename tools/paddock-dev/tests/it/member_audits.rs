//! The member audits (`member-audits` of the push gate and so of the member gate of `./dev merge check --gates`): the nextest profile
//! that declares them is read with a TOML parser (the format's own reader, ER-42), and its list is checked against the modules of this
//! test binary, so a typo or a renamed audit makes the list select less without anyone noticing (ER-33). The gate's own wiring and its
//! failure verdicts are tested in `src/pre_push/tests.rs`; `--no-tests=fail` turns an empty selection into a failure there.
//!
//! No nested `cargo nextest` runs here: it waits on the build-directory lock the outer run holds (measured 2026-11-17, a 120 s hang).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_dev::pre_push::gates::AUDIT_PROFILE;
use std::collections::BTreeSet;
use std::path::Path;

/// Audit modules that land with their own branches (feat/local-sccache: `dev_bin`, `cargo_target_lint`;
/// feat/golden-checked-when-made: `parity_corpus`). The profile names them already; until they exist they select nothing.
const NOT_LANDED: &[&str] = &["dev_bin", "cargo_target_lint", "parity_corpus"];

fn here(file: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap()
}

/// The profile's `default-filter`, read through the TOML parser.
fn filter() -> String {
    let doc: toml::Table = here(".config/nextest.toml").parse().unwrap();
    doc["profile"][AUDIT_PROFILE]["default-filter"]
        .as_str()
        .unwrap_or_else(|| panic!("profile {AUDIT_PROFILE} has no default-filter in .config/nextest.toml"))
        .to_string()
}

/// The modules of this test binary (`mod x;` lines of tests/it/main.rs).
fn modules() -> BTreeSet<String> {
    here("tests/it/main.rs")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("mod ")?.strip_suffix(';').map(str::to_string))
        .collect()
}

/// The module names of the first alternation in the filter: `test(/^(a|b|c)::/)`.
fn named_modules(filter: &str) -> Vec<String> {
    let start = filter.find("test(/^(").expect("the filter has a module alternation") + "test(/^(".len();
    let end = filter[start..].find(")::/)").expect("the alternation is closed") + start;
    filter[start..end].split('|').map(str::to_string).collect()
}

#[test]
fn the_profile_names_the_repository_wide_audits_and_every_name_is_a_module_or_waits_for_its_branch() {
    let f = filter();
    let named = named_modules(&f);
    // Positive control (ER-33): the audits that exist on every base are in the list; an empty or shrunk list fails here.
    for audit in ["tree", "golden", "check_allow_list"] {
        assert!(
            named.iter().any(|n| n == audit),
            "audit `{audit}` is missing from the {AUDIT_PROFILE} filter: {named:?}"
        );
    }
    let have = modules();
    for n in &named {
        assert!(
            have.contains(n) || NOT_LANDED.contains(&n.as_str()),
            "the {AUDIT_PROFILE} filter names `{n}`, which is no module of tests/it/main.rs (a typo or a renamed audit selects nothing)"
        );
    }
    // The control that the list tells tests apart: plain unit-style modules are not audits.
    for plain in ["slots", "locks", "worktree", "member_audits"] {
        assert!(!named.iter().any(|n| n == plain), "`{plain}` is not an audit");
    }
    assert!(
        f.contains("!test(/^golden::live_range_plans_match_affected_mjs"),
        "the over-120-s history replay stays out of the member audits: {f}"
    );
    assert!(f.contains("binary(it)"), "the audits live in the `it` binary: {f}");
}
