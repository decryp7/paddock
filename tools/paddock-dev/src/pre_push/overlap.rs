//! The overlap of a member and main (`./dev merge sync-member`, the `overlap-tests` row).
//!
//! The failure it prevents (ER-21, three times: the retry layer's removed `backoff_ms` field, the sccache warm-label test without a
//! port, the test cache's `Candidate.blind` field): a member is green alone and merges with main without a textual conflict, but
//! breaks on the combined tree, and only the train's run or a compile of the sync tip showed it. What both sides changed is where
//! that happens, so the tests of exactly those packages run on the sync tip.
//!
//! The overlap is the planner's own reach (ER-22): the packages the member's change reaches (`plan["packages"]`, the plan of
//! `--base`..HEAD) that the change main made since the member forked (`--overlap-since`..`--base`, planned the same way) reaches too. A
//! plan that says `all` reaches every package, so the other side's list is the overlap; both saying `all` cannot be narrowed and is
//! not measured, never a pass (ER-33).

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// What the `overlap-tests` row is told about the overlap.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Overlap {
    /// `--overlap-since` was not given (a developer's push): the row skips with that reason.
    #[default]
    NotAsked,
    /// The packages of each Cargo workspace that both sides reach; empty: the sides touched disjoint packages.
    Packages(BTreeMap<String, BTreeSet<String>>),
    /// The overlap could not be computed: the row is not measured, never a pass.
    Unknown(String),
}

/// The packages `plan` reaches in each workspace (`plan["packages"]`), and whether it reaches everything.
fn reached(plan: &Value) -> (BTreeMap<String, BTreeSet<String>>, bool) {
    let by_ws = plan["packages"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(ws, names)| {
                    let set = names
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
                    (ws.clone(), set.unwrap_or_default())
                })
                .collect()
        })
        .unwrap_or_default();
    (by_ws, plan["all"].as_bool().unwrap_or(false))
}

/// The overlap of the member's plan and main's plan.
pub fn of_plans(member: &Value, main: &Value) -> Overlap {
    let ((m, m_all), (t, t_all)) = (reached(member), reached(main));
    let both = match (m_all, t_all) {
        (true, true) => {
            return Overlap::Unknown(
                "the member and main each change something that reaches every package, so no package list narrows it".into(),
            );
        }
        (true, false) => t,
        (false, true) => m,
        (false, false) => m
            .into_iter()
            .map(|(ws, names)| {
                let common: BTreeSet<String> = t.get(&ws).map(|o| names.intersection(o).cloned().collect()).unwrap_or_default();
                (ws, common)
            })
            .collect(),
    };
    Overlap::Packages(both.into_iter().filter(|(_, names)| !names.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(all: bool, packages: Value) -> Value {
        json!({"all": all, "packages": packages})
    }

    fn names(o: &Overlap, ws: &str) -> Vec<String> {
        match o {
            Overlap::Packages(m) => m.get(ws).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
            other => panic!("not a package overlap: {other:?}"),
        }
    }

    /// The fixture the brief names: member and main both change package X (and each one more of its own): only X overlaps.
    #[test]
    fn a_package_both_sides_changed_is_the_overlap() {
        let member = plan(false, json!({"crates": ["paddock-proc", "paddock-a"]}));
        let main = plan(false, json!({"crates": ["paddock-proc", "paddock-b"]}));
        assert_eq!(names(&of_plans(&member, &main), "crates"), ["paddock-proc"]);
    }

    /// The other fixture: the sides touched disjoint packages (and disjoint workspaces): nothing overlaps.
    #[test]
    fn disjoint_packages_and_workspaces_have_no_overlap() {
        let member = plan(false, json!({"crates": ["paddock-a"], "apps/dyno": ["dyno-app"]}));
        let main = plan(false, json!({"crates": ["paddock-b"], "apps/pitwall": ["pitwall-app"]}));
        assert_eq!(of_plans(&member, &main), Overlap::Packages(BTreeMap::new()));
    }

    #[test]
    fn a_side_that_reaches_everything_overlaps_with_the_others_packages() {
        let all = plan(true, json!({}));
        let some = plan(false, json!({"crates": ["paddock-a"]}));
        assert_eq!(names(&of_plans(&all, &some), "crates"), ["paddock-a"]);
        assert_eq!(names(&of_plans(&some, &all), "crates"), ["paddock-a"]);
    }

    /// Positive control (ER-33): when both sides say `all` the overlap cannot be told, and the answer is `Unknown` (a not-measured
    /// row), not an empty overlap that would skip the tests and read as a pass.
    #[test]
    fn both_sides_reaching_everything_is_unknown_not_empty() {
        let all = plan(true, json!({}));
        assert!(matches!(of_plans(&all, &all), Overlap::Unknown(_)));
    }

    #[test]
    fn a_plan_without_a_package_list_overlaps_nothing() {
        let bare = json!({});
        assert_eq!(of_plans(&bare, &bare), Overlap::Packages(BTreeMap::new()));
    }
}
