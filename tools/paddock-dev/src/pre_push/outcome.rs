//! The push gate's rows as shared outcomes (ADR 0089 amendment 2026-11-13, WP-T3): each row becomes a `paddock_cli::Outcome` (a
//! pass or findings only through `measure`, with the commit and tree as evidence), the rows fold through the shared
//! `Verdict`, and the whole run gets one outcome of its own, which is what a consumer reads with `Outcome::from_wire` (merge check
//! `--gates`, WP-T5). A person's `--accept-not-measured` is an excuse on a not-measured item, named in the JSON, never a state.
//!
//! The rows keep their own type in [`super::exec`] (they carry the tail and the next step the table prints); this is the boundary
//! where they stop being a status a caller could have set by hand.

use super::exec::{Kind, Row, tool};
use crate::generators::run::working_tree;
use crate::plan_repo::git_text;
use paddock_cli::outcome::{LiveRepo, Sha, TreeId};
use paddock_cli::{CheckId, Finding, Outcome, Reason, Subject, UnixSeconds, Unmeasured, Verdict, measure};
use std::path::Path;

/// The repository as it is now, for `Evidence::adopt`.
pub struct GitRepo<'a>(pub &'a Path);

impl LiveRepo for GitRepo<'_> {
    fn tree_of(&self, sha: &Sha) -> Option<TreeId> {
        git_text(self.0, &["rev-parse", "--verify", "--quiet", &format!("{}^{{tree}}", sha.as_str())]).map(|t| TreeId::new(t.trim()))
    }
}

/// What a run of the gates at `root` is about: the commit and its tree when `sha` names one, else the working tree.
pub fn subject_of(root: &Path, sha: Option<&str>) -> Subject {
    match sha.and_then(|s| GitRepo(root).tree_of(&Sha::new(s)).map(|t| (s, t))) {
        Some((s, tree)) => Subject::Commit { sha: Sha::new(s), tree },
        None => working_tree(root),
    }
}

/// One row as an outcome for the shared [`Verdict`], with its next step in the text, and who excused it when a person's override did.
fn row_outcome(id: &str, row: &Row, subject: &Subject) -> (Outcome, Option<String>) {
    let with_next = |r: String| Reason::new(format!("{r}. Next: {}", row.next));
    let outcome = match &row.outcome {
        Outcome::Findings(_) => measure(
            CheckId::new(format!("pre-push:{id}")),
            subject.clone(),
            tool(),
            UnixSeconds::now(),
            || Ok(vec![Finding::failing(format!("{} Next step: {}", row.reason(), row.next))]),
        ),
        Outcome::NotMeasured(_) if row.excused_by.is_some() => Outcome::NotMeasured(Reason::new(format!(
            "{} (excused by a person's --accept-not-measured, logged)",
            row.reason()
        ))),
        Outcome::NotMeasured(_) => Outcome::NotMeasured(with_next(row.reason())),
        Outcome::CheckBroken(_) => Outcome::CheckBroken(with_next(row.reason())),
        other => other.clone(),
    };
    let by = if row.kind() == Kind::Accepted {
        row.excused_by.clone()
    } else {
        None
    };
    (outcome, by)
}

/// The rows folded by the shared [`Verdict`] (a scoped run: a gate the change does not reach has nothing to do), and the run's own
/// outcome: findings when a row failed, broken or not measured with the rows named, not measured when no row ran and passed, else a
/// pass for `subject`.
pub fn fold(rows: &[(String, Row)], subject: &Subject) -> (Verdict, Outcome) {
    let items: Vec<(String, Outcome, Option<String>)> = rows
        .iter()
        .map(|(id, r)| {
            let (o, by) = row_outcome(id, r, subject);
            (id.clone(), o, by)
        })
        .collect();
    let verdict = Verdict::scoped(items);
    let named = |pick: fn(&Row) -> bool| {
        rows.iter()
            .filter(|(_, r)| pick(r))
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    };
    let run = measure(CheckId::new("pre-push"), subject.clone(), tool(), UnixSeconds::now(), || {
        let broken = named(|r| r.kind() == Kind::Broken);
        if !broken.is_empty() {
            return Err(Unmeasured::Broken(Reason::new(format!("a check is broken: {broken}"))));
        }
        let failed = named(|r| r.kind() == Kind::Failed);
        if !failed.is_empty() {
            return Ok(vec![Finding::failing(format!("failed: {failed}"))]);
        }
        let unmeasured = named(|r| r.kind() == Kind::NotMeasured);
        if !unmeasured.is_empty() {
            return Err(Unmeasured::NotMeasured(Reason::new(format!("not measured: {unmeasured}"))));
        }
        if rows.is_empty() {
            return Err(Unmeasured::NotMeasured(Reason::new("no gate was planned or run")));
        }
        // A pass comes only from a gate that ran and passed (ADR 0089): a run of only skipped or excused rows proved nothing.
        if !rows.iter().any(|(_, r)| r.kind() == Kind::Passed) {
            return Err(Unmeasured::NotMeasured(Reason::new(
                "no gate ran and passed: every row was skipped (the change does not reach it) or excused by a person's --accept-not-measured",
            )));
        }
        // Accepted rows pass the push (the exit is unchanged) but are not proven: the shared verdict names them.
        Ok(Vec::new())
    });
    (verdict, run)
}

/// The outcome of a run that found nothing to check (a person's run with nothing changed since the base): not applicable, never a
/// pass, so no consumer can read the exit 0 as "checked this tree".
pub fn nothing_to_check(base: &str) -> Outcome {
    Outcome::NotApplicable(Reason::new(format!("nothing changed since {base}, so there was nothing to check")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_cli::Exit;

    fn subject() -> Subject {
        Subject::Commit {
            sha: Sha::new("a1"),
            tree: TreeId::new("t1"),
        }
    }

    fn passed() -> Row {
        Row::passed("x", &subject(), 1.0)
    }

    fn rows(list: Vec<(&str, Row)>) -> Vec<(String, Row)> {
        list.into_iter().map(|(i, r)| (i.to_string(), r)).collect()
    }

    #[test]
    fn all_passed_is_a_pass_with_the_commit_and_tree() {
        let (v, run) = fold(&rows(vec![("fmt", passed()), ("skip", Row::skipped("no"))]), &subject());
        assert_eq!(v.exit(), Exit::Ok);
        let j = run.json();
        assert_eq!(j["status"], "clean");
        assert_eq!(j["evidence"]["subject"]["sha"], "a1");
        assert_eq!(j["evidence"]["subject"]["tree"], "t1");
    }

    #[test]
    fn failing_beats_not_measured_and_broken_beats_both_as_in_the_table() {
        let nm = || Row::not_measured("r", "n");
        let failed = || Row::failed("g", &subject(), "c".into(), String::new(), "n".into(), 1.0);
        let broken = || Row::broken("c", "n");
        let all = |list: Vec<(&str, Row)>| fold(&rows(list), &subject());
        assert_eq!(all(vec![("a", nm())]).0.exit(), Exit::NotMeasured);
        assert_eq!(all(vec![("a", nm()), ("b", failed())]).0.exit(), Exit::Findings);
        assert_eq!(all(vec![("a", failed()), ("b", broken())]).0.exit(), Exit::CheckBroken);
        assert_eq!(all(vec![("a", nm())]).1.json()["status"], "not-measured");
        assert_eq!(all(vec![("a", nm()), ("b", failed())]).1.json()["status"], "findings");
        assert_eq!(all(vec![("a", broken())]).1.json()["status"], "check-broken");
    }

    #[test]
    fn an_accepted_row_is_a_named_excuse_and_the_exit_matches_the_table() {
        let mut acc = Row::not_measured("no docker", "n");
        acc.excused_by = Some("alex".into());
        let (v, _) = fold(&rows(vec![("clippy", acc), ("fmt", passed())]), &subject());
        assert_eq!(v.exit(), Exit::Ok);
        assert_eq!(v.json()["outcomes"][0]["override"], "alex");
    }

    /// "Delete the check": a run with no row at all is not measured, never a pass.
    #[test]
    fn no_rows_is_not_measured() {
        let (v, run) = fold(&[], &subject());
        assert_eq!(v.exit(), Exit::NotMeasured);
        assert_eq!(run.json()["status"], "not-measured");
    }

    /// ADR 0089: a pass comes only from a check that ran and passed; a run of only skipped rows proved nothing.
    #[test]
    fn all_skipped_is_not_measured_never_a_pass() {
        let (_, run) = fold(&rows(vec![("a", Row::skipped("no")), ("b", Row::skipped("no"))]), &subject());
        assert_eq!(run.json()["status"], "not-measured");
    }

    /// The same for a run in which every row was excused by a person's override.
    #[test]
    fn all_excused_is_not_measured_never_a_pass() {
        let mut acc = Row::not_measured("no docker", "n");
        acc.excused_by = Some("alex".into());
        let mut acc2 = Row::not_measured("no vm", "n");
        acc2.excused_by = Some("alex".into());
        let (_, run) = fold(&rows(vec![("a", acc), ("b", acc2)]), &subject());
        assert_eq!(run.json()["status"], "not-measured");
        let mut acc3 = Row::not_measured("no vm", "n");
        acc3.excused_by = Some("alex".into());
        let (_, run) = fold(&rows(vec![("a", acc3), ("b", Row::skipped("no"))]), &subject());
        assert_eq!(run.json()["status"], "not-measured");
    }

    #[test]
    fn nothing_to_check_is_not_applicable_never_a_pass() {
        assert_eq!(nothing_to_check("origin/main").json()["status"], "not-applicable");
    }

    /// "Delete the check": a run in which no gate was planned or ran is exit 6, never 0.
    #[test]
    fn a_run_with_no_gate_is_not_measured_never_a_pass() {
        let (v, run) = fold(&[], &subject());
        assert_eq!(v.exit(), Exit::NotMeasured);
        assert_eq!(run.json()["status"], "not-measured");
    }
}
