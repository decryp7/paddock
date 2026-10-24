//! What changed between two versions of a plan, for Sighter's re-import: links (recordings, results) are kept by case id
//! and by step, so only genuinely new or edited steps lose theirs.

use crate::ids::{CaseId, RequirementId};
use crate::plan::{Case, Plan};
use crate::step::Step;

/// How an entry differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Only in the new plan.
    Added,
    /// Only in the old plan.
    Removed,
    /// In both, with different content.
    Changed,
}

/// What happened to one step. Indexes are zero-based positions in the old and new case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepChange {
    /// The same step in both: its links stay.
    Kept {
        /// Position in the old case.
        old: usize,
        /// Position in the new case.
        new: usize,
    },
    /// A step with no counterpart in the old case.
    Added {
        /// Position in the new case.
        new: usize,
        /// Canonical text of the step.
        text: String,
    },
    /// A step with no counterpart in the new case.
    Removed {
        /// Position in the old case.
        old: usize,
        /// Canonical text of the step.
        text: String,
    },
    /// An old step replaced in place by an edited one: its links need a look.
    Changed {
        /// Position in the old case.
        old: usize,
        /// Position in the new case.
        new: usize,
        /// Canonical text before.
        from: String,
        /// Canonical text after.
        to: String,
    },
}

/// The difference of one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseDiff {
    /// The case.
    pub case: CaseId,
    /// Added, removed, or changed.
    pub kind: ChangeKind,
    /// Title, requirement links, priority, tags or data table differ.
    pub meta_changed: bool,
    /// Step by step, in order (every step of an added or removed case).
    pub steps: Vec<StepChange>,
}

/// The difference of two plans.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanDiff {
    /// Requirements added, removed or edited, in plan order (old first for removals).
    pub requirements: Vec<(RequirementId, ChangeKind)>,
    /// Cases that differ.
    pub cases: Vec<CaseDiff>,
    /// Cases that are identical, whose links all stay.
    pub unchanged: Vec<CaseId>,
}

impl PlanDiff {
    /// Nothing differs in requirements or cases.
    pub fn is_empty(&self) -> bool {
        self.requirements.is_empty() && self.cases.is_empty()
    }
}

/// Compares `old` with `new`.
pub fn diff(old: &Plan, new: &Plan) -> PlanDiff {
    let mut out = PlanDiff::default();
    for r in &new.requirements {
        match old.requirements.iter().find(|o| o.id == r.id) {
            None => out.requirements.push((r.id.clone(), ChangeKind::Added)),
            Some(o) if o != r => out.requirements.push((r.id.clone(), ChangeKind::Changed)),
            Some(_) => {}
        }
    }
    out.requirements.extend(
        old.requirements
            .iter()
            .filter(|o| !new.requirements.iter().any(|r| r.id == o.id))
            .map(|o| (o.id.clone(), ChangeKind::Removed)),
    );
    for c in &new.cases {
        match old.case(&c.id) {
            None => out.cases.push(CaseDiff {
                case: c.id.clone(),
                kind: ChangeKind::Added,
                meta_changed: true,
                steps: c
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(new, s)| StepChange::Added {
                        new,
                        text: s.to_string(),
                    })
                    .collect(),
            }),
            Some(o) => {
                let steps = diff_steps(&o.steps, &c.steps);
                let meta_changed = meta(o) != meta(c);
                if meta_changed || steps.iter().any(|s| !matches!(s, StepChange::Kept { .. })) {
                    out.cases.push(CaseDiff {
                        case: c.id.clone(),
                        kind: ChangeKind::Changed,
                        meta_changed,
                        steps,
                    });
                } else {
                    out.unchanged.push(c.id.clone());
                }
            }
        }
    }
    for o in old.cases.iter().filter(|o| new.case(&o.id).is_none()) {
        out.cases.push(CaseDiff {
            case: o.id.clone(),
            kind: ChangeKind::Removed,
            meta_changed: true,
            steps: o
                .steps
                .iter()
                .enumerate()
                .map(|(old, s)| StepChange::Removed {
                    old,
                    text: s.to_string(),
                })
                .collect(),
        });
    }
    out
}

fn meta(c: &Case) -> impl PartialEq + '_ {
    (&c.title, &c.requirements, c.priority, &c.tags, &c.data)
}

/// Aligns two step lists on their longest common subsequence; the gaps between matches pair up as edits.
fn diff_steps(old: &[Step], new: &[Step]) -> Vec<StepChange> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let gap = |out: &mut Vec<StepChange>, from: (usize, usize), to: (usize, usize)| {
        let (removed, added) = (to.0 - from.0, to.1 - from.1);
        for k in 0..removed.max(added) {
            let (o, nw) = (from.0 + k, from.1 + k);
            out.push(match (k < removed, k < added) {
                (true, true) => StepChange::Changed {
                    old: o,
                    new: nw,
                    from: old[o].to_string(),
                    to: new[nw].to_string(),
                },
                (true, false) => StepChange::Removed {
                    old: o,
                    text: old[o].to_string(),
                },
                _ => StepChange::Added {
                    new: nw,
                    text: new[nw].to_string(),
                },
            });
        }
    };
    let (mut gi, mut gj) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            gap(&mut out, (gi, gj), (i, j));
            out.push(StepChange::Kept { old: i, new: j });
            i += 1;
            j += 1;
            (gi, gj) = (i, j);
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    gap(&mut out, (gi, gj), (n, m));
    out
}
