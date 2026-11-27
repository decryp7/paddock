//! The user's approval of growth of a shrink-only list (ER-46), read by every list's growth check: a thin adapter over
//! [`paddock_approvals`] (the one reader, verdict and stage policy; ER-22). C155 first carried it, then C53, C139 and the allow lists
//! each had only a warning or an unlandable error.
//!
//! The approval is one `[[register_growth]]` record in docs/gates.toml (the register of gates), made by the user and never by an agent;
//! it names the `count` and the SHA-256 digest of exactly the added keys ([`paddock_approvals::growth_line`]). The added keys come
//! from [`super::baseline::Growth::Added`]'s `fresh`; a broken register approves nothing. What a missing approval counts as depends on
//! the run's [`Stage`]: a warning at a push, an error at the member gate and landing ([`paddock_approvals::severity`]).

use super::Ctx;
use super::finding::{Finding, Severity as FindingSeverity};
use paddock_approvals::{Severity, Verdict, judge_growth, parse_gates, severity};

/// The register of the user's approvals of growth: docs/gates.toml.
pub const REGISTER: &str = "docs/gates.toml";

/// What a growth check does with its finding.
#[derive(Debug, PartialEq, Eq)]
pub enum Standing {
    /// The user approved exactly these additions: no finding.
    Approved,
    /// The finding stands. `detail` (empty, or a parenthesis starting with a space) goes on its message, `ask` (a sentence) on its fix,
    /// `severity` says whether it is a warning or an error at this run's stage.
    Stands {
        /// Why a present entry does not match, or empty when there is none.
        detail: String,
        /// The exact line the user adds.
        ask: String,
        /// How it counts at the stage of the run.
        severity: Severity,
    },
}

/// The growth records of the register of this run's tree; a register that cannot be read approves nothing.
fn records(ctx: &Ctx) -> Vec<paddock_approvals::GrowthRecord> {
    ctx.read(REGISTER)
        .and_then(|t| parse_gates(&t).ok())
        .map(|g| g.growth)
        .unwrap_or_default()
}

/// What `check`'s growth finding does given the additions (`fresh` of the growth, their exact keys) and the register of this run's tree.
pub fn standing(ctx: &Ctx, check: &str, added: &[String]) -> Standing {
    let verdict = judge_growth(&records(ctx), check, added);
    // The additions are the branch's by construction: the growth is measured against the target branch.
    let Some(severity) = severity(ctx.stage(), &verdict, true) else {
        return Standing::Approved;
    };
    match verdict {
        Verdict::Missing { line_to_add } => Standing::Stands {
            detail: String::new(),
            ask: format!("The user records it in {REGISTER}: {line_to_add} (ER-46)."),
            severity,
        },
        Verdict::Mismatch { why } => Standing::Stands {
            detail: format!(" ({REGISTER} has a [[register_growth]] entry for {check} that does not match: {why})"),
            ask: format!(
                "The user records it in {REGISTER}: {} (ER-46).",
                paddock_approvals::growth_line(check, added)
            ),
            severity,
        },
        Verdict::Stale { why } => Standing::Stands {
            detail: format!(" ({why})"),
            ask: format!("Remove the [[register_growth]] entry for {check} from {REGISTER}."),
            severity,
        },
        Verdict::Approved => Standing::Approved,
    }
}

/// The finding for a `[[register_growth]]` entry of `check` that approves additions the target branch already has (the check adds
/// nothing now): a warning on a branch, an error at landing, which the integrator clears. `None` when there is no such entry.
pub fn stale_finding(ctx: &Ctx, check: &str, rule: u32) -> Option<Finding> {
    let Standing::Stands { detail, ask, severity } = standing(ctx, check, &[]) else {
        return None;
    };
    Some(at_severity(
        Finding::error(check, rule, REGISTER, 1, format!("stale growth approval{detail}"), ask),
        severity,
    ))
}

/// `finding` as a warning when the stage makes it one, else unchanged.
pub fn at_severity(mut finding: Finding, severity: Severity) -> Finding {
    if severity == Severity::Warning {
        finding.severity = FindingSeverity::Warn;
    }
    finding
}

/// The variable through which a caller says where the run is ([`paddock_approvals::Stage`]): the push gate and the member gate set it
/// for their rows; a caller that sets nothing (CI) gets the stage of its ref ([`STAGE_FROM_REF_ENV`]), else the strictest, landing.
pub const STAGE_ENV: &str = "PADDOCK_APPROVAL_STAGE";

/// The variable through which CI says the ref it runs on (ci.yml declares `${{ github.ref }}` once); the ref-to-stage mapping is
/// [`paddock_approvals::Stage::from_ref`], the one definition.
pub const STAGE_FROM_REF_ENV: &str = "PADDOCK_APPROVAL_STAGE_FROM_REF";

/// The stage of this run from the environment `get` reads: an explicit [`STAGE_ENV`] wins (a name that is no stage is an error to
/// report, never a default), else the stage of the [`STAGE_FROM_REF_ENV`] ref, else landing.
pub fn stage_from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<paddock_approvals::Stage, String> {
    let explicit = get(STAGE_ENV).filter(|n| !n.trim().is_empty());
    if explicit.is_none()
        && let Some(r) = get(STAGE_FROM_REF_ENV)
    {
        return Ok(paddock_approvals::Stage::from_ref(&r));
    }
    paddock_approvals::Stage::or_strictest(explicit.as_deref()).map_err(|e| format!("{STAGE_ENV}: {e}"))
}
