//! The cleanup centre: the categories, the recent scans (kept in memory only; a run must use one younger than an
//! hour), the quarantine and the Recycle Bin port. Scans and queries have no side effects on the user's files; only
//! [`CleanupCentre::run`], [`CleanupCentre::restore`] and the quarantine expiry move or remove anything.

use super::quarantine::{Quarantine, Restored};
use super::roots::Env;
use super::run::{run, RunCtx};
use super::scan::{scan_category, ScanCtx};
use super::trash::Trasher;
use super::walk::{owner, Budget};
use super::{packs, usage};
use crate::paths::Dirs;
use dyno_core::cleanup::{CategorySpec, Os};
use dyno_core::cleanup::{CleanupCategory, CleanupOutcome, CleanupScan, DeleteMode, FailedItem, QuarantineEntry, SizeNode};
use dyno_core::ports::FileLocks;
use dyno_core::time::Timestamp;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Scans kept for a run.
const KEEP_SCANS: usize = 4;
/// The most categories one scan covers.
const MAX_CATEGORIES: usize = 100;
/// How long all categories of one scan may take together.
pub const SCAN_SECS: u64 = 40;

struct State {
    scans: VecDeque<CleanupScan>,
    counter: u64,
}

/// See the module docs.
pub struct CleanupCentre {
    specs: Vec<CategorySpec>,
    env: Env,
    quarantine: Quarantine,
    trash: Box<dyn Trasher>,
    state: Mutex<State>,
}

fn guard(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl CleanupCentre {
    /// Loads the categories (built-in plus the user's packs) and expires old quarantines. Returns messages for the
    /// packs that were ignored.
    pub fn load(dirs: &Dirs, env: Env, trash: Box<dyn Trasher>, now: Timestamp) -> (CleanupCentre, Vec<String>) {
        let (specs, notes) = packs::load(&dirs.packs());
        let quarantine = Quarantine::new(dirs.quarantine());
        quarantine.expire(now);
        let centre = CleanupCentre {
            specs,
            env,
            quarantine,
            trash,
            state: Mutex::new(State {
                scans: VecDeque::new(),
                counter: 0,
            }),
        };
        (centre, notes)
    }

    /// The categories that apply to this OS.
    pub fn categories(&self, scheduled: &[String]) -> Vec<CleanupCategory> {
        let os: Os = self.env.os();
        self.specs
            .iter()
            .filter(|s| s.applies_to(os))
            .map(|s| s.view(scheduled.iter().any(|x| x == &s.id)))
            .collect()
    }

    /// Scans the categories. Deletes nothing. The scan is kept so a run can refer to it.
    pub fn scan(&self, ids: &[String], dev_age_days: u32, now: Timestamp) -> Result<CleanupScan, String> {
        if ids.is_empty() {
            return Err("Choose what to scan.".into());
        }
        if ids.len() > MAX_CATEGORIES {
            return Err("Too many categories in one scan.".into());
        }
        let os = self.env.os();
        let mut chosen: Vec<&CategorySpec> = Vec::new();
        for id in ids {
            let spec = self
                .specs
                .iter()
                .find(|s| &s.id == id && s.applies_to(os))
                .ok_or_else(|| format!("Dyno has no cleanup category `{id}` for this system."))?;
            if !chosen.iter().any(|c| c.id == spec.id) {
                chosen.push(spec);
            }
        }
        let scan_id = {
            let mut st = guard(&self.state);
            st.counter += 1;
            format!("scan-{}-{}", now.millis(), st.counter)
        };
        let ctx = ScanCtx {
            env: &self.env,
            now,
            dev_age_days: dev_age_days.clamp(7, 365),
            deadline: Instant::now() + Duration::from_secs(SCAN_SECS),
            owner: std::fs::metadata(&self.env.home).ok().and_then(|m| owner(&m)),
        };
        let results: Vec<_> = chosen.iter().map(|s| scan_category(s, &ctx, &scan_id)).collect();
        let scan = CleanupScan {
            id: scan_id,
            at: now,
            total_bytes: results.iter().map(|r| r.bytes).sum(),
            results,
        };
        let mut st = guard(&self.state);
        st.scans.push_back(scan.clone());
        while st.scans.len() > KEEP_SCANS {
            st.scans.pop_front();
        }
        Ok(scan)
    }

    /// Deletes the chosen items of a recent scan (see [`super::run::run`]). The scan is used up whatever the outcome.
    pub fn run(
        &self,
        scan_id: &str,
        item_ids: &[String],
        mode: DeleteMode,
        now: Timestamp,
        locks: &mut dyn FileLocks,
    ) -> Result<CleanupOutcome, String> {
        let scan = guard(&self.state)
            .scans
            .iter()
            .find(|s| s.id == scan_id)
            .cloned()
            .ok_or("That scan is no longer available; scan again.")?;
        let batch_id = {
            let mut st = guard(&self.state);
            st.counter += 1;
            format!("q-{}-{}", now.millis(), st.counter)
        };
        let out = run(
            &scan,
            item_ids,
            mode,
            RunCtx {
                specs: &self.specs,
                env: &self.env,
                now,
                locks,
                trash: self.trash.as_ref(),
                quarantine: &self.quarantine,
                batch_id,
            },
        );
        // A stale scan is refused without being used up, so the user sees why and scans again.
        if out.is_ok() {
            guard(&self.state).scans.retain(|s| s.id != scan_id);
        }
        out
    }

    /// Moves files that something else already admitted and re-checked (the file finder) to the Recycle Bin / Trash or the
    /// quarantine. Nothing is checked here: the caller owns the admission, this owns only the recoverable move.
    pub fn dispose(&self, items: &[(PathBuf, u64)], mode: DeleteMode, now: Timestamp) -> CleanupOutcome {
        let batch_id = {
            let mut st = guard(&self.state);
            st.counter += 1;
            format!("q-{}-{}", now.millis(), st.counter)
        };
        let mut batch = match mode {
            DeleteMode::Quarantine => match self.quarantine.begin(&batch_id, now) {
                Ok(b) => Some(b),
                Err(why) => {
                    return CleanupOutcome {
                        freed_bytes: 0,
                        deleted: 0,
                        failed: items
                            .iter()
                            .map(|(p, _)| FailedItem {
                                path: p.to_string_lossy().into_owned(),
                                reason: why.clone(),
                            })
                            .collect(),
                        quarantine_id: None,
                    }
                }
            },
            DeleteMode::RecycleBin => None,
        };
        let (mut deleted, mut freed, mut failed) = (0usize, 0u64, Vec::new());
        for (path, bytes) in items {
            let moved = match batch.as_mut() {
                Some(b) => b.stage(path, *bytes),
                None => self.trash.trash(path),
            };
            match moved {
                Ok(()) => {
                    deleted += 1;
                    freed = freed.saturating_add(*bytes);
                }
                Err(reason) => failed.push(FailedItem {
                    path: path.to_string_lossy().into_owned(),
                    reason,
                }),
            }
        }
        let quarantine_id = batch.and_then(|b| {
            let id = b.id().to_string();
            b.finish().map(|_| id)
        });
        CleanupOutcome {
            freed_bytes: freed,
            deleted,
            failed,
            quarantine_id,
        }
    }

    /// The names of the categories the ids in a scan belong to (for the activity log).
    pub fn category_names(&self, scan_id: &str, item_ids: &[String]) -> Vec<String> {
        let st = guard(&self.state);
        let Some(scan) = st.scans.iter().find(|s| s.id == scan_id) else {
            return vec![];
        };
        let mut names: Vec<String> = scan
            .results
            .iter()
            .filter(|r| r.items.iter().any(|i| item_ids.contains(&i.id)))
            .filter_map(|r| self.specs.iter().find(|s| s.id == r.category).map(|s| s.name.clone()))
            .collect();
        names.dedup();
        names
    }

    /// The quarantines waiting (expired ones are deleted first).
    pub fn quarantines(&self, now: Timestamp) -> Vec<QuarantineEntry> {
        self.quarantine.expire(now);
        self.quarantine.list()
    }

    /// Records which activity entry describes a quarantine.
    pub fn link_activity(&self, quarantine_id: &str, activity_id: &str) {
        self.quarantine.link_activity(quarantine_id, activity_id);
    }

    /// Puts a quarantine back.
    pub fn restore(&self, id: &str, now: Timestamp) -> Result<Restored, String> {
        self.quarantine.expire(now);
        self.quarantine.restore(id)
    }

    /// The size tree of `path` (the system drive when `None`).
    pub fn disk_usage(&self, path: Option<&str>) -> Result<SizeNode, String> {
        let root = match path {
            Some(p) => usage::check(p)?,
            None => usage::default_root(),
        };
        Ok(usage::size_tree(&root, Budget::new(usage::MAX_ENTRIES, usage::MAX_SECS)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{backdate, now};
    use super::super::trash::FolderBin;
    use super::*;
    use dyno_core::ports::PlatformError;
    use dyno_core::process::Locker;
    use std::path::{Path, PathBuf};

    struct NoLocks;

    impl FileLocks for NoLocks {
        fn lockers(&mut self, _: &Path) -> Result<Vec<Locker>, PlatformError> {
            Ok(vec![])
        }
    }

    struct Fx {
        _t: tempfile::TempDir,
        home: PathBuf,
        centre: CleanupCentre,
        notes: Vec<String>,
    }

    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(t.path()).unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(data.join("packs")).unwrap();
        std::fs::write(
            data.join("packs/mine.toml"),
            "format = 1\n[[category]]\nid = \"mine\"\nname = \"Mine\"\ndescription = \"d\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [\"{home}/cachey\"]\n",
        )
        .unwrap();
        std::fs::write(data.join("packs/broken.toml"), "format = ").unwrap();
        let env = Env {
            os: Some(Os::Linux),
            home: home.clone(),
            temp: home.join("tmp"),
            ..Default::default()
        };
        let (centre, notes) = CleanupCentre::load(&Dirs::at(&data), env, Box::new(FolderBin(root.join("bin"))), now());
        Fx {
            _t: t,
            home,
            centre,
            notes,
        }
    }

    #[test]
    fn categories_come_from_packs_for_this_os_and_a_bad_pack_is_reported() {
        let f = fx();
        assert_eq!(f.notes.len(), 1);
        assert!(f.notes[0].contains("broken.toml"));
        let cats = f.centre.categories(&["mine".into(), "downloadInstallers".into()]);
        assert!(cats.iter().any(|c| c.id == "mine" && c.scheduled));
        assert!(
            !cats.iter().any(|c| c.id == "systemTemp" || c.id == "safariCache"),
            "Windows/macOS-only ones are hidden on Linux"
        );
        assert!(cats.iter().any(|c| c.id == "appCaches"));
        assert!(!cats.iter().find(|c| c.id == "downloadInstallers").unwrap().scheduled);
    }

    #[test]
    fn scan_then_run_then_restore_end_to_end() {
        let f = fx();
        let cache = f.home.join("cachey");
        std::fs::create_dir_all(&cache).unwrap();
        for n in ["one", "two"] {
            std::fs::write(cache.join(n), n).unwrap();
            backdate(&cache.join(n), 2);
        }
        assert!(f.centre.scan(&[], 30, now()).is_err());
        assert!(f
            .centre
            .scan(&["nope".into()], 30, now())
            .unwrap_err()
            .contains("no cleanup category"));
        assert!(f.centre.scan(&["systemTemp".into()], 30, now()).is_err(), "not on this OS");
        let too_many: Vec<String> = (0..101).map(|i| i.to_string()).collect();
        assert!(f.centre.scan(&too_many, 30, now()).is_err());
        let scan = f.centre.scan(&["mine".into(), "mine".into()], 30, now()).unwrap();
        assert_eq!(scan.results.len(), 1, "duplicates collapse");
        assert_eq!(scan.total_bytes, 6);
        assert!(cache.join("one").exists(), "a scan deletes nothing");
        let pick = vec![scan.results[0].items[0].id.clone()];
        assert_eq!(f.centre.category_names(&scan.id, &pick), vec!["Mine".to_string()]);
        let out = f.centre.run(&scan.id, &pick, DeleteMode::Quarantine, now(), &mut NoLocks).unwrap();
        assert_eq!(out.deleted, 1);
        let q = out.quarantine_id.unwrap();
        assert_eq!(f.centre.quarantines(now())[0].id, q);
        // The scan is used up.
        assert!(f
            .centre
            .run(&scan.id, &pick, DeleteMode::Quarantine, now(), &mut NoLocks)
            .unwrap_err()
            .contains("no longer available"));
        f.centre.link_activity(&q, "act-1");
        let back = f.centre.restore(&q, now()).unwrap();
        assert_eq!((back.restored, back.activity.as_deref()), (1, Some("act-1")));
        assert!(cache.join("one").exists() && cache.join("two").exists());
        assert!(f.centre.restore(&q, now()).is_err());
    }

    #[test]
    fn a_stale_scan_is_refused_but_kept_and_old_scans_are_dropped() {
        let f = fx();
        let cache = f.home.join("cachey");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("x"), "x").unwrap();
        backdate(&cache.join("x"), 2);
        let t0 = now();
        let scan = f.centre.scan(&["mine".into()], 30, t0).unwrap();
        let pick = vec![scan.results[0].items[0].id.clone()];
        let late = t0.plus(dyno_core::cleanup::SCAN_VALID_MS + 1);
        assert!(f
            .centre
            .run(&scan.id, &pick, DeleteMode::Quarantine, late, &mut NoLocks)
            .unwrap_err()
            .contains("hour"));
        assert!(cache.join("x").exists());
        for i in 0..KEEP_SCANS + 1 {
            f.centre.scan(&["mine".into()], 30, t0.plus(i64::try_from(i).unwrap())).unwrap();
        }
        assert!(f
            .centre
            .run(&scan.id, &pick, DeleteMode::Quarantine, t0, &mut NoLocks)
            .unwrap_err()
            .contains("no longer available"));
        assert!(f.centre.category_names("gone", &pick).is_empty());
    }

    #[test]
    fn disk_usage_validates_its_path() {
        let f = fx();
        std::fs::write(f.home.join("file"), vec![0u8; 10]).unwrap();
        let tree = f.centre.disk_usage(Some(&f.home.to_string_lossy())).unwrap();
        assert_eq!(tree.bytes, 10, "{tree:?}");
        assert!(f.centre.disk_usage(Some("relative")).is_err());
    }
}
