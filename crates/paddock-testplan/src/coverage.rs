//! Traceability, computed once for both tools: requirement x case x last status.

use std::collections::BTreeMap;

use crate::ids::{AcceptanceRef, CaseId, ReqRef, RequirementId};
use crate::plan::Plan;
use crate::result::{RunResult, Status};

/// The last known outcome of one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseCell {
    /// The case.
    pub case: CaseId,
    /// Its last status (the worst over its iterations in the latest run that has it); `None` when it never ran.
    pub status: Option<Status>,
    /// The latest result is for an older version of the plan.
    pub stale: bool,
}

/// One acceptance criterion and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceRow {
    /// The criterion.
    pub acceptance: AcceptanceRef,
    /// Rolled-up status of the cases linked to it; `None` when none ran or none is linked.
    pub status: Option<Status>,
    /// Cases linked to it.
    pub cases: Vec<CaseId>,
}

/// One requirement with its cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequirementRow {
    /// The requirement.
    pub requirement: RequirementId,
    /// Cases linked to it, whole or by criterion, in plan order.
    pub cases: Vec<CaseCell>,
    /// Rolled-up status: the worst of the cases that ran; `None` when no linked case has run.
    pub status: Option<Status>,
    /// Its criteria.
    pub acceptance: Vec<AcceptanceRow>,
}

impl RequirementRow {
    /// No case is linked to the requirement at all.
    pub fn uncovered(&self) -> bool {
        self.cases.is_empty()
    }
}

/// The traceability matrix of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Matrix {
    /// One row per requirement, in plan order.
    pub rows: Vec<RequirementRow>,
}

/// Builds the matrix.
pub struct Coverage;

impl Coverage {
    /// `results` may hold any runs; those for another plan are ignored. For each case the latest run (by `started`, then by
    /// position) that has it decides, and its statuses are rolled up over iterations.
    pub fn from(plan: &Plan, results: &[RunResult]) -> Matrix {
        let hash = plan.hash();
        let mut runs: Vec<&RunResult> = results.iter().filter(|r| r.plan == plan.id).collect();
        runs.sort_by(|a, b| a.started.as_str().cmp(b.started.as_str()));
        let mut last: BTreeMap<&CaseId, (Status, bool)> = BTreeMap::new();
        for run in runs {
            let mut here: BTreeMap<&CaseId, Status> = BTreeMap::new();
            for c in &run.cases {
                here.entry(&c.case)
                    .and_modify(|s| *s = s.worst(c.status))
                    .or_insert(c.status);
            }
            for (case, status) in here {
                last.insert(case, (status, run.plan_hash != hash));
            }
        }
        let cell = |case: &CaseId| {
            let known = last.get(case);
            CaseCell {
                case: case.clone(),
                status: known.map(|k| k.0),
                stale: known.is_some_and(|k| k.1),
            }
        };
        let rows = plan
            .requirements
            .iter()
            .map(|req| {
                let linked = |wanted: Option<u32>| -> Vec<CaseId> {
                    plan.cases
                        .iter()
                        .filter(|c| {
                            c.requirements.iter().any(|r| match (r, wanted) {
                                (ReqRef::Requirement(id), None) => id == &req.id,
                                (ReqRef::Acceptance(a), None) => a.requirement() == &req.id,
                                (ReqRef::Acceptance(a), Some(n)) => {
                                    a.requirement() == &req.id && a.number() == n
                                }
                                (ReqRef::Requirement(_), Some(_)) => false,
                            })
                        })
                        .map(|c| c.id.clone())
                        .collect()
                };
                let roll = |ids: &[CaseId]| {
                    ids.iter()
                        .filter_map(|c| last.get(c).map(|k| k.0))
                        .reduce(Status::worst)
                };
                let cases = linked(None);
                let acceptance = (1..).zip(&req.acceptance).map(|(n, _)| {
                    let ids = linked(Some(n));
                    AcceptanceRow {
                        acceptance: AcceptanceRef::of(&req.id, n),
                        status: roll(&ids),
                        cases: ids,
                    }
                });
                RequirementRow {
                    requirement: req.id.clone(),
                    status: roll(&cases),
                    cases: cases.iter().map(cell).collect(),
                    acceptance: acceptance.collect(),
                }
            })
            .collect();
        Matrix { rows }
    }
}
