//! The stage a check runs at, and what it makes of a verdict.

use crate::verdict::Verdict;

/// Where in a change's life a check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// A developer's push of a branch.
    BranchPush,
    /// The member gate: the branch, ready to join a train.
    MemberGate,
    /// Landing: the train (or main) that the integrator lands.
    Landing,
}

impl Stage {
    /// The name a flag or variable carries.
    pub fn name(self) -> &'static str {
        match self {
            Stage::BranchPush => "branch-push",
            Stage::MemberGate => "member-gate",
            Stage::Landing => "landing",
        }
    }

    /// The stage a name gives, if it names one.
    pub fn parse(name: &str) -> Option<Stage> {
        [Stage::BranchPush, Stage::MemberGate, Stage::Landing]
            .into_iter()
            .find(|s| s.name() == name.trim())
    }

    /// The stage of the ref a CI run is on (`github.ref`): a trial or train branch is the member gate, anything else (`main`, a tag, any
    /// other branch, an empty ref) is landing, the strictest. The one definition of the mapping; a caller reads the ref, never matches it.
    pub fn from_ref(git_ref: &str) -> Stage {
        let r = git_ref.trim();
        let is_member = ["refs/heads/trial/", "refs/heads/train/"]
            .iter()
            .any(|p| r.strip_prefix(p).is_some_and(|rest| !rest.is_empty()));
        if is_member {
            Stage::MemberGate
        } else {
            Stage::Landing
        }
    }

    /// The stage of a caller that may have given none: the strictest, so a caller that says nothing (CI) is never more lenient than
    /// landing. A name that is not a stage is an error for the caller to report, not a silent default.
    pub fn or_strictest(name: Option<&str>) -> Result<Stage, String> {
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            None => Ok(Stage::Landing),
            Some(n) => Stage::parse(n).ok_or_else(|| {
                format!("{n:?} is not a stage (branch-push, member-gate or landing)")
            }),
        }
    }
}

/// How a finding counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Printed, never fails.
    Warning,
    /// Fails the run.
    Error,
}

/// What `verdict` means at `stage`, or `None` when nothing is wrong.
///
/// - Missing: a warning at a branch push when the branch itself adds the entry (the author may still be waiting for the approval),
///   an error at the member gate and at landing, and an error at every stage for an entry the branch did not add (main already
///   carries it without the approval).
/// - Stale: a warning on a branch (the member gate included), a finding at landing, which the integrator clears.
/// - Mismatch: an error at every stage.
pub fn severity(stage: Stage, verdict: &Verdict, added_by_branch: bool) -> Option<Severity> {
    match verdict {
        Verdict::Approved => None,
        Verdict::Mismatch { .. } => Some(Severity::Error),
        Verdict::Missing { .. } if added_by_branch && stage == Stage::BranchPush => {
            Some(Severity::Warning)
        }
        Verdict::Missing { .. } => Some(Severity::Error),
        Verdict::Stale { .. } if stage == Stage::Landing => Some(Severity::Error),
        Verdict::Stale { .. } => Some(Severity::Warning),
    }
}
