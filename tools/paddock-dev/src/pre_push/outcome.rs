//! The push gate's rows as shared outcomes (ADR 0089 amendment 2026-11-13, WP-T3): each row becomes a `paddock_cli::Outcome` (a
//! pass or findings only through `measure`, with the commit and tree as evidence), the rows fold through the shared
//! `Verdict`, and the whole run gets one outcome of its own, which is what a consumer reads with `Outcome::from_wire` (merge check
//! `--gates`, WP-T5). A person's `--accept-not-measured` is an excuse on a not-measured item, named in the JSON, never a state.
//!
//! The rows keep their own type in [`super::exec`] (they carry the tail and the next step the table prints); this is the boundary
//! where they stop being a status a caller could have set by hand.

use super::exec::Verdict as Row;
use crate::generators::run::working_tree;
use crate::plan_repo::git_text;
use paddock_cli::outcome::{LiveRepo, Sha, TreeId};
use paddock_cli::{CheckId, Finding, Outcome, Reason, Subject, ToolVersion, UnixSeconds, Unmeasured, Verdict, measure};
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

/// The tool, with its version.
pub fn tool() -> ToolVersion {
    ToolVersion::new(concat!("paddock-dev pre-push ", env!("CARGO_PKG_VERSION")))
}

/// One row as an outcome, and who excused it when a person's override did. The child's exit is the probe inside `measure`.
fn row_outcome(id: &str, row: &Row, subject: &Subject, who: &str) -> (Outcome, Option<String>) {
    let check = || CheckId::new(format!("pre-push:{id}"));
    let made = |found: Vec<Finding>| measure(check(), subject.clone(), tool(), UnixSeconds::now(), || Ok(found));
    match row {
        Row::Passed { .. } => (made(Vec::new()), None),
        Row::Failed { cause, next, .. } => (made(vec![Finding::failing(format!("{cause} Next step: {next}"))]), None),
        Row::Skipped { reason } => (Outcome::NotApplicable(Reason::new(reason.clone())), None),
        Row::NotMeasured { reason, next } => (Outcome::NotMeasured(Reason::new(format!("{reason}. Next: {next}"))), None),
        Row::Accepted { reason } => (
            Outcome::NotMeasured(Reason::new(format!(
                "{reason} (excused by a person's --accept-not-measured, logged)"
            ))),
            Some(who.to_string()),
        ),
        Row::Broken { cause, next } => (Outcome::CheckBroken(Reason::new(format!("{cause}. Next: {next}"))), None),
    }
}

/// The rows folded by the shared [`Verdict`] (a scoped run: a gate the change does not reach has nothing to do), and the run's own
/// outcome: findings when a row failed, broken or not measured with the rows named, else a pass for `subject`.
pub fn fold(rows: &[(String, Row)], subject: &Subject, who: &str) -> (Verdict, Outcome) {
    let items: Vec<(String, Outcome, Option<String>)> = rows
        .iter()
        .map(|(id, r)| {
            let (o, by) = row_outcome(id, r, subject, who);
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
        let broken = named(|r| matches!(r, Row::Broken { .. }));
        if !broken.is_empty() {
            return Err(Unmeasured::Broken(Reason::new(format!("a check is broken: {broken}"))));
        }
        let failed = named(|r| matches!(r, Row::Failed { .. }));
        if !failed.is_empty() {
            return Ok(vec![Finding::failing(format!("failed: {failed}"))]);
        }
        let unmeasured = named(|r| matches!(r, Row::NotMeasured { .. }));
        if !unmeasured.is_empty() {
            return Err(Unmeasured::NotMeasured(Reason::new(format!("not measured: {unmeasured}"))));
        }
        if rows.is_empty() {
            return Err(Unmeasured::NotMeasured(Reason::new("no gate was planned or run")));
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
        Row::Passed { secs: 1.0 }
    }

    fn rows(list: Vec<(&str, Row)>) -> Vec<(String, Row)> {
        list.into_iter().map(|(i, r)| (i.to_string(), r)).collect()
    }

    #[test]
    fn all_passed_is_a_pass_with_the_commit_and_tree() {
        let (v, run) = fold(
            &rows(vec![("fmt", passed()), ("skip", Row::Skipped { reason: "no".into() })]),
            &subject(),
            "me",
        );
        assert_eq!(v.exit(), Exit::Ok);
        let j = run.json();
        assert_eq!(j["status"], "clean");
        assert_eq!(j["evidence"]["subject"]["sha"], "a1");
        assert_eq!(j["evidence"]["subject"]["tree"], "t1");
    }

    #[test]
    fn failing_beats_not_measured_and_broken_beats_both_as_in_the_table() {
        let nm = || Row::NotMeasured {
            reason: "r".into(),
            next: "n".into(),
        };
        let failed = || Row::Failed {
            cause: "c".into(),
            tail: String::new(),
            next: "n".into(),
            secs: 1.0,
        };
        let broken = || Row::Broken {
            cause: "c".into(),
            next: "n".into(),
        };
        let all = |list: Vec<(&str, Row)>| fold(&rows(list), &subject(), "me");
        assert_eq!(all(vec![("a", nm())]).0.exit(), Exit::NotMeasured);
        assert_eq!(all(vec![("a", nm()), ("b", failed())]).0.exit(), Exit::Findings);
        assert_eq!(all(vec![("a", failed()), ("b", broken())]).0.exit(), Exit::CheckBroken);
        assert_eq!(all(vec![("a", nm())]).1.json()["status"], "not-measured");
        assert_eq!(all(vec![("a", nm()), ("b", failed())]).1.json()["status"], "findings");
        assert_eq!(all(vec![("a", broken())]).1.json()["status"], "check-broken");
    }

    #[test]
    fn an_accepted_row_is_a_named_excuse_and_the_exit_matches_the_table() {
        let acc = Row::Accepted {
            reason: "no docker".into(),
        };
        let (v, _) = fold(&rows(vec![("clippy", acc), ("fmt", passed())]), &subject(), "alex");
        assert_eq!(v.exit(), Exit::Ok);
        assert_eq!(v.json()["outcomes"][0]["override"], "alex");
    }

    /// "Delete the check": a run with no row at all is not measured, never a pass.
    #[test]
    fn no_rows_is_not_measured() {
        let (v, run) = fold(&[], &subject(), "me");
        assert_eq!(v.exit(), Exit::NotMeasured);
        assert_eq!(run.json()["status"], "not-measured");
    }

    #[test]
    fn nothing_to_check_is_not_applicable_never_a_pass() {
        assert_eq!(nothing_to_check("origin/main").json()["status"], "not-applicable");
    }
}
