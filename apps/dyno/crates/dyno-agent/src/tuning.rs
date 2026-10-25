//! Tuning: lists the catalogue, applies one change on a click and reverts it. Nothing changes until `apply` runs;
//! every change is a typed [`HelperAction`] shown as a preview first, recorded in the activity log with its inverse
//! ([`UndoRecord::Tuning`]) and verified by reading the setting back. Before a revert the setting is read again: if
//! something else changed it since, the revert is refused unless the user insists. Changes that need administrator
//! rights go through the privileged helper (ADR 0016), installed on the user's click; where it cannot be installed they
//! are listed with the reason and are not applicable.

use crate::agent::{lock, now, Shared};
use dyno_core::activity::{ActivityEntry, ActivityKind, UndoRecord};
use dyno_core::helper::HelperAction;
use dyno_core::metrics::PowerSource;
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{AgentError, ErrorCode, Message};
use dyno_core::tuning::catalog::{catalogue, mark_blocked};
use dyno_core::tuning::{applied_entry, find, Plan, TuningSuggestion};

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

fn from_platform(e: PlatformError) -> AgentError {
    let code = match &e {
        PlatformError::NotFound(_) => ErrorCode::NotFound,
        PlatformError::AccessDenied(_) => ErrorCode::AccessDenied,
        PlatformError::Unsupported(_) => ErrorCode::Unsupported,
        PlatformError::Timeout(_) => ErrorCode::Timeout,
        PlatformError::Failed(_) => ErrorCode::Internal,
    };
    AgentError::new(code, e.to_string())
}

/// A change Dyno applied that is still in force (not undone).
struct Applied {
    entry_id: String,
    id: String,
    apply: HelperAction,
    revert: HelperAction,
    after: String,
}

/// The tuning changes Dyno applied and the user has not reverted, newest first, one per suggestion id.
fn applied(shared: &Shared) -> Vec<Applied> {
    let rows = lock(&shared.core).store.activity(500).unwrap_or_default();
    let mut out: Vec<Applied> = Vec::new();
    for (entry, record, undone) in rows {
        if let (false, UndoRecord::Tuning { id, apply, revert, after }) = (undone, record) {
            if !out.iter().any(|a| a.id == id) {
                out.push(Applied {
                    entry_id: entry.id,
                    id,
                    apply,
                    revert,
                    after,
                });
            }
        }
    }
    out
}

fn power_source(shared: &Shared) -> Option<PowerSource> {
    lock(&shared.core).recent.back().and_then(|s| s.battery.value().map(|b| b.source))
}

/// The suggestions that fit this machine now (not yet applied ones), with what is blocked marked. The health report's
/// fixes link to these.
pub fn plans(shared: &Shared) -> Vec<Plan> {
    let facts = crate::handlers::facts(shared);
    let done: Vec<String> = applied(shared).into_iter().map(|a| a.id).collect();
    let power = power_source(shared);
    let mut system = lock(&shared.ports.tuning);
    let host = system.host();
    let mut plans = catalogue(host, &facts, power, &done, &mut **system);
    drop(system);
    // A change that needs administrator rights is applicable when the helper is installed or can be installed on the
    // user's click (the first click then asks first); it is blocked only where the helper cannot exist.
    let helper_possible = lock(&shared.ports.helper).can_install().is_ok();
    mark_blocked(&mut plans, host, helper_possible);
    plans
}

/// The catalogue as the page shows it: what could be applied, plus what Dyno applied (with Revert).
pub fn list(shared: &Shared) -> Result<Vec<TuningSuggestion>, AgentError> {
    let mut out: Vec<TuningSuggestion> = plans(shared).into_iter().map(|p| p.suggestion).filter(|s| !s.applied).collect();
    for a in applied(shared) {
        let mut s = applied_entry(&a.id, &a.revert);
        s.preview = a.apply.preview();
        s.changed_outside = lock(&shared.ports.tuning).read(&a.apply).is_ok_and(|now| now != a.after);
        out.push(s);
    }
    Ok(out)
}

/// Applies the suggestion `id` on the user's click.
pub fn apply(shared: &Shared, id: &str) -> Result<ActivityEntry, AgentError> {
    if applied(shared).iter().any(|a| a.id == id) {
        return Err(bad("Dyno already applied that; revert it first."));
    }
    let plans = plans(shared);
    let plan =
        find(&plans, id).ok_or_else(|| AgentError::new(ErrorCode::NotFound, "Dyno has no such suggestion for this computer right now."))?;
    if let Some(reason) = &plan.suggestion.blocked {
        return Err(AgentError::new(ErrorCode::AccessDenied, reason.clone()));
    }
    plan.apply.validate().map_err(bad)?;
    plan.revert.validate().map_err(bad)?;
    let expected = plan.apply.setting().1;
    let after = {
        let mut system = lock(&shared.ports.tuning);
        system.apply(&plan.apply).map_err(from_platform)?;
        let after = system.read(&plan.apply).unwrap_or_else(|_| expected.clone());
        if after != expected {
            // The system accepted the call but the setting did not change: put back what we can and say so.
            let _ = system.apply(&plan.revert);
            return Err(AgentError::new(
                ErrorCode::Internal,
                format!(
                    "{} did not take effect (the setting reads {after}); nothing was changed.",
                    plan.suggestion.title
                ),
            ));
        }
        after
    };
    let t = now();
    let entry = lock(&shared.core).log(
        t,
        ActivityKind::Tuning,
        plan.suggestion.title.clone(),
        plan.apply.preview().join("; "),
        UndoRecord::Tuning {
            id: id.into(),
            apply: plan.apply.clone(),
            revert: plan.revert.clone(),
            after,
        },
        None,
    );
    shared.broadcast(&Message::Activity(entry.clone()));
    Ok(entry)
}

/// Puts the previous setting back for the change in `record` (a [`UndoRecord::Tuning`]). Unless `force`, it first
/// checks that the setting still reads as Dyno left it. Does not touch the log.
pub fn undo_record(shared: &Shared, record: &UndoRecord, force: bool) -> Result<(), AgentError> {
    let UndoRecord::Tuning { apply, revert, after, .. } = record else {
        return Err(bad("That is not a tuning change."));
    };
    revert.validate().map_err(bad)?;
    let mut system = lock(&shared.ports.tuning);
    if !force {
        let now = system.read(apply).map_err(from_platform)?;
        if now != *after {
            return Err(bad(format!(
                "Something else changed this setting since Dyno applied it (it now reads {now}, Dyno left {after}). \
                 Revert anyway to put the earlier setting back."
            )));
        }
    }
    system.apply(revert).map_err(from_platform)
}

/// Reverts the suggestion `id` that Dyno applied and records that in the activity log.
pub fn revert(shared: &Shared, id: &str, force: bool) -> Result<ActivityEntry, AgentError> {
    let a = applied(shared)
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "Dyno has not applied that, so there is nothing to revert."))?;
    let record = UndoRecord::Tuning {
        id: a.id.clone(),
        apply: a.apply.clone(),
        revert: a.revert.clone(),
        after: a.after.clone(),
    };
    undo_record(shared, &record, force)?;
    let t = now();
    let mut core = lock(&shared.core);
    let _ = core.store.mark_undone(&a.entry_id);
    let title = applied_entry(&a.id, &a.revert).title.replace(" (applied by Dyno)", "");
    let entry = core.log(
        t,
        ActivityKind::Tuning,
        format!("Reverted: {title}"),
        a.revert.preview().join("; "),
        UndoRecord::None,
        None,
    );
    drop(core);
    shared.broadcast(&Message::Activity(entry.clone()));
    Ok(entry)
}
