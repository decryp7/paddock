//! Cleanup requests (ADR 0005). Queries (`CleanupCategories`, `ScanCleanup`, `Quarantine`, `DiskUsage`) change nothing;
//! `RunCleanup` and `RestoreQuarantine` are the only ones that move files, and only for items of a scan the user saw
//! and confirmed. The work itself is in [`crate::cleanup`].

use crate::agent::{lock, now, Shared};
use dyno_core::activity::{ActivityKind, UndoRecord};
use dyno_core::cleanup::DeleteMode;
use dyno_core::protocol::{AgentError, ErrorCode, Reply, Request};
use std::sync::Arc;

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

/// The most items one run may name (a scan lists at most 1000 per category).
const MAX_ITEMS: usize = 20_000;

/// Answers a cleanup request.
pub fn handle(shared: &Arc<Shared>, request: Request) -> Result<Reply, AgentError> {
    match request {
        Request::CleanupCategories => {
            let scheduled = lock(&shared.core).settings.cleanup.categories.clone();
            Ok(Reply::Categories(shared.cleanup.categories(&scheduled)))
        }
        Request::ScanCleanup { categories } => {
            let age = lock(&shared.core).settings.cleanup.dev_project_age_days;
            shared
                .cleanup
                .scan(&categories, age, now())
                .map(|s| Reply::Scan(Box::new(s)))
                .map_err(bad)
        }
        Request::RunCleanup { scan_id, item_ids, mode } => run(shared, &scan_id, &item_ids, mode).map(Reply::Outcome),
        Request::Quarantine => Ok(Reply::Quarantines(shared.cleanup.quarantines(now()))),
        Request::RestoreQuarantine { id } => restore(shared, &id).map(|()| Reply::Ok),
        Request::DiskUsage { path } => shared
            .cleanup
            .disk_usage(path.as_deref())
            .map(|n| Reply::SizeTree(Box::new(n)))
            .map_err(bad),
        other => Err(bad(format!("{other:?} is not a cleanup request."))),
    }
}

fn run(
    shared: &Arc<Shared>,
    scan_id: &str,
    item_ids: &[String],
    mode: DeleteMode,
) -> Result<dyno_core::cleanup::CleanupOutcome, AgentError> {
    if item_ids.len() > MAX_ITEMS || scan_id.len() > 128 {
        return Err(bad("That request is too large."));
    }
    let t = now();
    let names = shared.cleanup.category_names(scan_id, item_ids);
    let out = {
        let mut locks = lock(&shared.ports.locks);
        shared.cleanup.run(scan_id, item_ids, mode, t, &mut **locks).map_err(bad)?
    };
    if out.deleted > 0 {
        let (where_to, undo) = match (&out.quarantine_id, mode) {
            (Some(q), _) => (
                "to the 7-day quarantine",
                UndoRecord::Quarantine {
                    id: q.clone(),
                    expires: dyno_core::activity::quarantine_expiry(t),
                },
            ),
            _ => ("to the Recycle Bin / Trash", UndoRecord::None),
        };
        let title = format!("Cleaned {} item{}", out.deleted, if out.deleted == 1 { "" } else { "s" });
        let detail = format!(
            "{} {where_to}",
            if names.is_empty() {
                "Selected items".to_string()
            } else {
                names.join(", ")
            }
        );
        let entry = lock(&shared.core).log(t, ActivityKind::Cleanup, title, detail, undo, Some(out.freed_bytes));
        if let Some(q) = &out.quarantine_id {
            shared.cleanup.link_activity(q, &entry.id);
        }
    }
    Ok(out)
}

/// Puts a quarantine back and marks the activity entry that made it undone. Items that cannot be put back stay in the
/// quarantine and are named in the error.
pub fn restore(shared: &Arc<Shared>, id: &str) -> Result<(), AgentError> {
    let r = shared
        .cleanup
        .restore(id, now())
        .map_err(|m| AgentError::new(ErrorCode::NotFound, m))?;
    if r.failed.is_empty() {
        if let Some(activity) = &r.activity {
            let _ = lock(&shared.core).store.mark_undone(activity);
        }
        return Ok(());
    }
    Err(bad(format!(
        "Put back {} item{}; {} could not be restored: {} ({})",
        r.restored,
        if r.restored == 1 { "" } else { "s" },
        r.failed.len(),
        r.failed[0].reason,
        r.failed[0].path
    )))
}
