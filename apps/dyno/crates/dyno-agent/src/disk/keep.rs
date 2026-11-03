//! Keeping finished scans in the saved-maps cache (ADR 0029): what a pass that ended hands to the cache writer, and what the
//! centre remembers about the result. Nothing here reads a folder; the paths of a scan reach the cache only inside the saved
//! target, never a log line.

use super::engine::{End, Walk};
use super::volumes::VolumeLister;
use crate::cache::db::SaveParams;
use crate::cache::{Cache, SaveJob, SaveOutcome};
use dyno_core::disk::scan::{DiskScanStatus, DiskTarget, NotSaved};
use std::sync::{Arc, Mutex, MutexGuard};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the cache holds for one scan, shared by the scan's entry and its status hooks.
#[derive(Debug, Default)]
pub struct Kept {
    state: Mutex<KeptState>,
}

#[derive(Debug, Default)]
struct KeptState {
    /// The cache row holding this scan's map, once saved.
    row: Option<i64>,
    /// Saves handed to the cache writer that have not ended: the scan's tree is still being read by one.
    pending: usize,
    /// The volume's identity when the pass ended (where the OS gives one).
    identity: Option<String>,
    /// The volume's used bytes when the pass ended (whole-volume scans).
    used_then: Option<u64>,
}

impl Kept {
    /// A scan offered from the cache.
    pub fn saved(row: i64, identity: Option<String>, used_then: Option<u64>) -> Kept {
        Kept {
            state: Mutex::new(KeptState {
                row: Some(row),
                pending: 0,
                identity,
                used_then,
            }),
        }
    }

    /// A scan that starts now, on a volume with this identity.
    pub fn live(identity: Option<String>) -> Kept {
        Kept {
            state: Mutex::new(KeptState {
                identity,
                ..KeptState::default()
            }),
        }
    }

    /// The cache row, when saved.
    pub fn row(&self) -> Option<i64> {
        lock(&self.state).row
    }

    /// Forgets the row (it was removed from the cache).
    pub fn clear_row(&self) {
        lock(&self.state).row = None;
    }

    /// Whether a save of this scan's tree is queued or running.
    pub fn pending(&self) -> bool {
        lock(&self.state).pending > 0
    }

    /// The volume identity recorded with the scan.
    pub fn identity(&self) -> Option<String> {
        lock(&self.state).identity.clone()
    }

    /// The volume's used bytes when the scan ended.
    pub fn used_then(&self) -> Option<u64> {
        lock(&self.state).used_then
    }

    fn begin_save(&self, used_then: Option<u64>) {
        let mut s = lock(&self.state);
        s.pending += 1;
        s.used_then = used_then;
    }

    fn end_save(&self, saved: Option<i64>) {
        let mut s = lock(&self.state);
        s.pending = s.pending.saturating_sub(1);
        if saved.is_some() {
            s.row = saved;
        }
    }
}

/// Hands finished passes to the cache and tells the centre which rows the cache removed.
pub struct Keeper {
    cache: Mutex<Option<Cache>>,
    lister: Arc<dyn VolumeLister>,
    /// Rows the cache removed (replaced or evicted) since the centre last looked.
    removed: Arc<Mutex<Vec<i64>>>,
}

/// What a scan's status hooks give the keeper about the pass that ended.
pub struct PassEnd<'a> {
    /// The scan's id (for the log).
    pub scan: u64,
    /// The scan's target.
    pub target: &'a DiskTarget,
    /// The volume as listed when the scan started, for its used space now.
    pub listed: Option<&'a super::volumes::ListedVolume>,
    /// The scan this one replaces, if a Rescan.
    pub replaces: Option<u64>,
    /// The scan's state shared with the entry.
    pub kept: &'a Arc<Kept>,
    /// The status at the end of the pass.
    pub status: DiskScanStatus,
    /// Told the not-saved reason (or `None` when kept), to say it in a new status event.
    pub told: Box<dyn Fn(Option<NotSaved>) + Send + Sync>,
}

/// The key a target is saved under: `volume:<id>` or `folder:<path>` (user data, never logged).
pub fn target_key(t: &DiskTarget) -> String {
    match t {
        DiskTarget::Volume { id } => format!("volume:{id}"),
        DiskTarget::Folder { path } => format!("folder:{path}"),
    }
}

fn kind_of(t: &DiskTarget) -> &'static str {
    match t {
        DiskTarget::Volume { .. } => "volume",
        DiskTarget::Folder { .. } => "folder",
    }
}

fn limits_json(l: dyno_core::disk::index::ScanLimits) -> String {
    serde_json::json!({ "maxNodes": l.max_nodes, "notableBytes": l.notable_bytes, "maxHardLinks": l.max_hard_links }).to_string()
}

impl Keeper {
    /// A keeper with no cache yet.
    pub fn new(lister: Arc<dyn VolumeLister>) -> Keeper {
        Keeper {
            cache: Mutex::new(None),
            lister,
            removed: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Gives the keeper the cache (or takes it away).
    pub fn set_cache(&self, cache: Option<Cache>) {
        *lock(&self.cache) = cache;
    }

    /// The cache, when there is one.
    pub fn cache(&self) -> Option<Cache> {
        lock(&self.cache).clone()
    }

    /// The rows removed since the last call.
    pub fn take_removed(&self) -> Vec<i64> {
        std::mem::take(&mut *lock(&self.removed))
    }

    /// Saves the pass that just ended: the done scan, or (marked as partial by its state) a cancelled or failed one. A Rescan
    /// that did not finish saves nothing, so the map it replaces stays.
    pub fn save_pass(&self, pass: PassEnd<'_>, walk: &Walk, end: &End) {
        let Some(cache) = self.cache() else { return };
        if pass.replaces.is_some() && *end != End::Done {
            return;
        }
        let used_then = pass
            .listed
            .filter(|l| l.volume.whole_volume)
            .and_then(|l| self.lister.capacity_now(l))
            .map(|(total, free)| total.saturating_sub(free));
        let index = walk.shared_index();
        let params = SaveParams {
            target_key: target_key(pass.target),
            target: serde_json::to_string(pass.target).unwrap_or_default(),
            volume_identity: pass.kept.identity(),
            status: serde_json::to_string(&pass.status).unwrap_or_default(),
            limits: limits_json(index.limits()),
            scanned_at: pass.status.scanned_at.map_or(0, |t| t.millis()),
            used_bytes_then: used_then,
        };
        let kept = pass.kept.clone();
        let told = pass.told;
        kept.begin_save(used_then);
        let removed_into = self.removed.clone();
        cache.save(SaveJob {
            params,
            index,
            scan: pass.scan,
            kind: kind_of(pass.target),
            done: Box::new(move |outcome| {
                match outcome {
                    SaveOutcome::Saved { id, removed } => {
                        kept.end_save(Some(id));
                        lock(&removed_into).extend(removed);
                        told(None);
                    }
                    SaveOutcome::TooBig => {
                        kept.end_save(None);
                        told(Some(NotSaved::TooBig));
                    }
                    SaveOutcome::NoSpace => {
                        kept.end_save(None);
                        told(Some(NotSaved::NoSpace));
                    }
                    SaveOutcome::Failed => {
                        kept.end_save(None);
                        told(Some(NotSaved::Failed));
                    }
                    // Replaced by a newer save of the same target, or saving is off: nothing to say.
                    SaveOutcome::Superseded | SaveOutcome::Disabled => kept.end_save(None),
                }
            }),
        });
    }
}
