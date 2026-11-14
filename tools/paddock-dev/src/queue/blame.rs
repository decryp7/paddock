//! Which member of a train a red run blames (docs/plans/merge-queue.md Q3; ADR 0098 WP-1). One rule for the queue's tick and
//! for `./dev ci triage --members` on a train somebody built by hand: a failure is a member's when the member's change reaches
//! the projects the failure belongs to (the planner's answer for the member's own diff), or changes a file the failing gate's
//! log names. A member whose plan runs everything (`*`) is a suspect only for lack of a precise one. Pure: the callers read the
//! planner, git and the logs.

use super::ports::Failure;
use std::collections::BTreeSet;

/// What one member's change reaches.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    /// The projects of its plan, normalised (`*` for a plan that runs everything).
    pub projects: BTreeSet<String>,
    /// The files it changes (read only when a failure names files).
    pub files: Vec<String>,
}

/// The suspects of a red run.
#[derive(Debug, PartialEq, Eq)]
pub struct Suspects<'a> {
    /// Members with the failures they are suspected of, by index in the input.
    pub precise: Vec<(usize, Vec<&'a Failure>)>,
    /// Members whose plan runs everything and which no failure names precisely.
    pub wild: Vec<usize>,
}

/// Whether any failure names files, so the callers need each member's changed files.
pub fn needs_files(failures: &[&Failure]) -> bool {
    failures.iter().any(|f| !f.files.is_empty())
}

/// The suspects among `members` for `failures` (the failures that are not already failing on main).
pub fn suspects<'a>(members: &[Reach], failures: &[&'a Failure]) -> Suspects<'a> {
    let (mut precise, mut wild) = (Vec::new(), Vec::new());
    for (i, m) in members.iter().enumerate() {
        let by_file = |f: &Failure| f.files.iter().any(|x| m.files.contains(x));
        let hit: Vec<&Failure> = failures
            .iter()
            .copied()
            .filter(|f| by_file(f) || (!m.projects.contains("*") && f.projects.iter().any(|x| m.projects.contains(x))))
            .collect();
        if !hit.is_empty() {
            precise.push((i, hit));
        } else if m.projects.contains("*") {
            wild.push(i);
        }
    }
    Suspects { precise, wild }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::ports::Blame;

    fn set(p: &[&str]) -> BTreeSet<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    fn fail(what: &str, projects: &[&str], files: &[&str]) -> Failure {
        Failure {
            what: what.into(),
            projects: set(projects),
            files: files.iter().map(|s| s.to_string()).collect(),
            blame: Blame::New,
        }
    }

    fn reach(projects: &[&str], files: &[&str]) -> Reach {
        Reach {
            projects: set(projects),
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn a_failure_is_blamed_on_the_members_whose_plan_reaches_its_project() {
        // Two members that only fail together (train 8: a lint list of one, new code of the other): the failure is in the
        // project both reach, so both are suspects; the member that reaches nothing it names is not.
        let (a, b, c) = (
            reach(&["paddock-dev"], &[]),
            reach(&["paddock-dev", "pitwall"], &[]),
            reach(&["ui"], &[]),
        );
        let f = fail("paddock-dev safe_remove_lint", &["paddock-dev"], &[]);
        let s = suspects(&[a, b, c], &[&f]);
        assert_eq!(s.precise.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [0, 1]);
        assert!(s.wild.is_empty());
    }

    #[test]
    fn a_gate_job_is_blamed_through_the_files_its_log_names() {
        let a = reach(&["ui"], &["tools/heavy/inputs.txt"]);
        let b = reach(&["ui"], &["README.md"]);
        let f = fail("train gates", &["train gates"], &["tools/heavy/inputs.txt"]);
        let s = suspects(&[a, b], &[&f]);
        assert_eq!(s.precise.len(), 1);
        assert_eq!(s.precise[0].0, 0);
    }

    #[test]
    fn a_member_that_runs_everything_is_a_suspect_only_for_lack_of_a_precise_one() {
        let all = reach(&["*"], &[]);
        let f = fail("x", &["paddock-dev"], &[]);
        let alone = suspects(std::slice::from_ref(&all), &[&f]);
        assert!(alone.precise.is_empty() && alone.wild == [0]);
        // The wild member never matches a project by name: its `*` is not a project.
        let with = suspects(&[all, reach(&["paddock-dev"], &[])], &[&f]);
        assert_eq!(with.precise.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [1]);
        assert_eq!(with.wild, [0]);
    }

    #[test]
    fn needs_files_only_when_a_failure_names_one() {
        let (a, b) = (fail("a", &["x"], &[]), fail("b", &["x"], &["f"]));
        assert!(!needs_files(&[&a]) && needs_files(&[&a, &b]));
    }
}
