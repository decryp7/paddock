//! The D12 gate on this tree (ADR 0117): the old planner's rule tables are still here, or five consecutive clean nightlies are
//! recorded in `docs/escape-register.toml`. Its control is in the unit tests of `escape::register` (a deleted table is refused).
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::repo_root;
use paddock_dev::escape::register;

#[test]
fn the_old_rule_tables_stay_until_five_clean_nightlies_are_recorded() {
    let root = repo_root();
    if let Err(e) = register::check_tree(&root) {
        panic!("::error::{e}");
    }
    // The register guards something real: every table it names exists on this tree today (a stale entry would pass the gate forever).
    let r = register::load(&root).unwrap();
    for t in &r.tables {
        let text = std::fs::read_to_string(root.join(&t.file)).unwrap_or_default();
        assert!(
            !text.is_empty() && t.contains.as_ref().is_none_or(|c| text.contains(c.as_str()))
                || register::consecutive_clean(&r) >= r.required,
            "{} no longer holds {:?} and the register has {} clean nightlies",
            t.file,
            t.contains,
            register::consecutive_clean(&r)
        );
    }
}
