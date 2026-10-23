//! Watched folders (WP-10, ADR 0019). Queries (`WatchedFolders`, `PreviewWatchedFolder`) move nothing; `SweepWatchedFolder`
//! and the background sweep are the only code that moves files, and only in a folder the person chose, set up, previewed and
//! turned on. Every sweep that moves something is one activity entry that can be undone (the quarantine).

use crate::agent::{lock, now, Shared};
use crate::cleanup::roots::Env;
use crate::watch::{preview, sweep, Change, SweepCtx};
use dyno_core::activity::{quarantine_expiry, ActivityKind, UndoRecord};
use dyno_core::protocol::{AgentError, ErrorCode, Message, Reply, Request, TimelineEvent, TimelineEventKind};
use dyno_core::watch::{keep_text, SweepSummary, WatchedFolder};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn bad(msg: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, msg)
}

fn overview(shared: &Shared) -> Reply {
    Reply::Watched(Box::new(shared.watch.overview()))
}

/// Answers a watched-folder request.
pub fn handle(shared: &Arc<Shared>, request: Request) -> Result<Reply, AgentError> {
    let env = Env::current();
    let home = env.home.to_string_lossy().into_owned();
    match request {
        Request::WatchedFolders => Ok(overview(shared)),
        Request::AddWatchedFolder {
            path,
            template,
            keep_for_secs,
            recursive,
        } => {
            if path.len() > 4096 {
                return Err(bad("That path is too long."));
            }
            // Compare real paths: the data folder may be reached through a link (macOS temp folders are).
            let data_dir = std::fs::canonicalize(shared.dirs.root()).unwrap_or_else(|_| shared.dirs.root().to_path_buf());
            let data = data_dir.to_string_lossy().trim_start_matches(r"\\?\").to_string();
            shared
                .watch
                .add(&path, template.as_deref(), keep_for_secs, recursive, env.os(), &home, &data, now())
                .map_err(bad)?;
            Ok(overview(shared))
        }
        Request::SetWatchedFolder {
            id,
            keep_for_secs,
            enabled,
            recursive,
        } => {
            shared
                .watch
                .change(
                    &id,
                    Change {
                        keep_for_secs,
                        enabled,
                        recursive,
                    },
                )
                .map_err(bad)?;
            Ok(overview(shared))
        }
        Request::RemoveWatchedFolder { id } => {
            shared.watch.remove(&id).map_err(|m| AgentError::new(ErrorCode::NotFound, m))?;
            Ok(overview(shared))
        }
        Request::PreviewWatchedFolder { id } => preview(&shared.watch, &id, &home, now())
            .map(|p| Reply::WatchPreview(Box::new(p)))
            .map_err(bad),
        Request::SweepWatchedFolder { id } => {
            let folder = shared
                .watch
                .get(&id)
                .ok_or_else(|| AgentError::new(ErrorCode::NotFound, "Dyno no longer watches that folder."))?;
            if !folder.previewed {
                return Err(bad("Look at the preview first: it shows exactly what would be moved."));
            }
            sweep_one(shared, &folder, true).map_err(bad)?;
            Ok(overview(shared))
        }
        other => Err(bad(format!("{other:?} is not a watched-folder request."))),
    }
}

/// Sweeps one folder now (`trust_created`: read file creation times, as production does) and records the result: the
/// folder's last sweep, one undoable activity entry and a timeline
/// event when something moved.
pub fn sweep_one(shared: &Arc<Shared>, folder: &WatchedFolder, trust_created: bool) -> Result<(), String> {
    let t = now();
    let home = Env::current().home.to_string_lossy().into_owned();
    let batch_id = format!("{}-b", shared.watch.new_id(t));
    let result = {
        let mut locks = lock(&shared.ports.locks);
        sweep(
            folder,
            t,
            SweepCtx {
                home: &home,
                quarantine: shared.cleanup.quarantine(),
                locks: &mut **locks,
                batch_id,
                trust_created,
            },
        )
    };
    let summary = match result {
        Ok(s) => s,
        Err(why) => {
            // Remember the failure so a missing folder is retried on the usual schedule, not every round.
            shared.watch.record(
                &folder.id,
                SweepSummary {
                    at: t,
                    moved: 0,
                    bytes: 0,
                    failed: 1,
                    first_failure: Some(why.clone()),
                    quarantine_id: None,
                },
            );
            return Err(why);
        }
    };
    if let Some(q) = &summary.quarantine_id {
        let name = Path::new(&folder.path)
            .file_name()
            .map_or_else(|| folder.path.clone(), |n| n.to_string_lossy().into_owned());
        let title = format!("Moved {} old file{} out of {name}", summary.moved, if summary.moved == 1 { "" } else { "s" });
        let detail = format!(
            "Older than {} in the watched folder {}; held in the 7-day quarantine. Undo puts them back.",
            keep_text(folder.keep_for_secs),
            folder.path
        );
        let mut core = lock(&shared.core);
        let entry = core.log(
            t,
            ActivityKind::Cleanup,
            title.clone(),
            detail,
            UndoRecord::Quarantine {
                id: q.clone(),
                expires: quarantine_expiry(t),
            },
            Some(summary.bytes),
        );
        let _ = core.store.put_event(&TimelineEvent {
            at: t,
            kind: TimelineEventKind::Cleanup,
            label: title,
        });
        drop(core);
        shared.cleanup.link_activity(q, &entry.id);
        shared.broadcast(&Message::Activity(entry));
    }
    shared.watch.record(&folder.id, summary);
    Ok(())
}

/// How often the background loop looks for folders that are due.
const LOOK_EVERY: Duration = Duration::from_secs(30);

/// The thread body: sweep the folders that are due, until the agent stops.
pub fn run(shared: &Arc<Shared>) {
    let wait = |d: Duration| {
        let mut waited = Duration::ZERO;
        while waited < d && !shared.stopped() {
            std::thread::sleep(Duration::from_millis(250));
            waited += Duration::from_millis(250);
        }
    };
    wait(Duration::from_secs(20));
    while !shared.stopped() {
        for folder in shared.watch.due(now()) {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sweep_one(shared, &folder, true)));
            match r {
                Ok(Ok(())) => {}
                // A missing folder or a refusal is shown on the page the next time it is opened; try again next round.
                Ok(Err(_)) => {}
                Err(_) => eprintln!("dyno-agent: a watched-folder sweep failed unexpectedly"),
            }
            if shared.stopped() {
                return;
            }
        }
        wait(LOOK_EVERY);
    }
}
