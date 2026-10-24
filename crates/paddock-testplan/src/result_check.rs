//! Checks of a [`RunResult`] that its types cannot make: limits, hashes, ids of a run, scores.

use crate::error::PlanError;
use crate::ids::clip;
use crate::plan::{MAX_STEPS, MAX_STRING};
use crate::result::{EvidenceRef, MAX_CASE_RESULTS, RunResult};

fn field(name: &str, msg: &str) -> PlanError {
    PlanError::BadField {
        field: name.to_owned(),
        msg: msg.to_owned(),
    }
}

fn text(name: &str, value: &str) -> Result<(), PlanError> {
    if value.len() > MAX_STRING {
        Err(PlanError::Limit {
            what: format!("`{name}`"),
            max: MAX_STRING,
        })
    } else {
        Ok(())
    }
}

fn evidence(e: &EvidenceRef) -> Result<(), PlanError> {
    let hex = e.sha256.len() == 64
        && e.sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if hex {
        Ok(())
    } else {
        Err(field(
            &format!("sha256 `{}`", clip(&e.sha256)),
            "expected 64 lowercase hex digits",
        ))
    }
}

fn is_hash(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Checks a result built in code or parsed: limits, the plan hash and evidence hashes, run id, step scores.
/// [`parse_result`](crate::parse_result) calls it.
pub fn validate_result(r: &RunResult) -> Result<(), PlanError> {
    if r.cases.len() > MAX_CASE_RESULTS {
        return Err(PlanError::Limit {
            what: "case results".to_owned(),
            max: MAX_CASE_RESULTS,
        });
    }
    if !is_hash(&r.plan_hash) {
        return Err(field(
            &format!("plan_hash `{}`", clip(&r.plan_hash)),
            "expected 64 lowercase hex digits",
        ));
    }
    crate::ids::RequirementId::new(&r.run_id.replace('#', "-")).map_err(|_| {
        field(
            &format!("run_id `{}`", clip(&r.run_id)),
            "expected 1 to 64 characters from A-Z a-z 0-9 . _ : / # -",
        )
    })?;
    text("env os", &r.env.os)?;
    for v in [
        &r.env.theme,
        &r.env.language,
        &r.env.resolution,
        &r.env.app_build,
        &r.env.sighter_version,
    ]
    .into_iter()
    .flatten()
    {
        text("env", v)?;
    }
    for c in &r.cases {
        if c.steps.len() > MAX_STEPS {
            return Err(PlanError::Limit {
                what: format!("step results in case `{}`", c.case),
                max: MAX_STEPS,
            });
        }
        if let Some(reason) = &c.reason {
            text("reason", reason)?;
        }
        c.evidence.iter().try_for_each(evidence)?;
        for s in &c.steps {
            s.evidence.iter().try_for_each(evidence)?;
            if s.score.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
                return Err(field("score", "must be a number from 0 to 1"));
            }
        }
    }
    Ok(())
}
