//! The deleter: removes only the items the user chose from a recent scan, each re-checked first, to the Recycle Bin /
//! Trash or the quarantine. Files in use are skipped with the name of the program holding them.

use super::guard::recheck;
use super::quarantine::Quarantine;
use super::roots::{folders, Env};
use super::trash::Trasher;
use dyno_core::cleanup::{chosen, CategorySpec, CleanupOutcome, CleanupScan, DeleteMode, FailedItem};
use dyno_core::ports::FileLocks;
use dyno_core::time::Timestamp;
use std::collections::HashSet;

/// What a run needs besides the scan.
pub struct RunCtx<'a> {
    /// The categories (to re-resolve each one's folders and rules).
    pub specs: &'a [CategorySpec],
    /// The folders of this OS.
    pub env: &'a Env,
    /// Now.
    pub now: Timestamp,
    /// Who holds a file open.
    pub locks: &'a mut dyn FileLocks,
    /// The OS Recycle Bin / Trash.
    pub trash: &'a dyn Trasher,
    /// The quarantine.
    pub quarantine: &'a Quarantine,
    /// A fresh id for the quarantine batch.
    pub batch_id: String,
}

fn holders(locks: &mut dyn FileLocks, path: &std::path::Path) -> Option<String> {
    let found = locks.lockers(path).ok()?;
    let mut names: Vec<String> = found.into_iter().map(|l| l.name).collect();
    names.sort();
    names.dedup();
    (!names.is_empty()).then(|| names.join(", "))
}

/// Deletes `item_ids` of `scan`. Fails as a whole (nothing touched) when the scan is stale or an id is not in it;
/// otherwise every item is handled on its own and failures are listed.
pub fn run(scan: &CleanupScan, item_ids: &[String], mode: DeleteMode, ctx: RunCtx<'_>) -> Result<CleanupOutcome, String> {
    let picked = chosen(scan, item_ids, ctx.now)?;
    let mut batch = match mode {
        DeleteMode::Quarantine => Some(ctx.quarantine.begin(&ctx.batch_id, ctx.now)?),
        DeleteMode::RecycleBin => None,
    };
    let mut failed: Vec<FailedItem> = Vec::new();
    let mut deleted = 0;
    let mut freed = 0u64;
    let mut done: HashSet<String> = HashSet::new();
    for (category, item) in picked {
        let fail = |failed: &mut Vec<FailedItem>, reason: String| {
            failed.push(FailedItem {
                path: item.path.clone(),
                reason,
            })
        };
        if !done.insert(item.path.clone()) {
            continue;
        }
        let Some(spec) = ctx.specs.iter().find(|s| s.id == category) else {
            fail(&mut failed, "Its category is no longer known; scan again.".into());
            continue;
        };
        let roots = folders(spec, ctx.env);
        let real = match recheck(item, spec, &roots, &ctx.env.home, ctx.now) {
            Ok(p) => p,
            Err(why) => {
                fail(&mut failed, why);
                continue;
            }
        };
        if !item.folder {
            if let Some(who) = holders(ctx.locks, &real) {
                fail(&mut failed, format!("In use by {who}. Close it and try again."));
                continue;
            }
        }
        let moved = match batch.as_mut() {
            Some(b) => b.stage(&real, item.bytes),
            None => ctx.trash.trash(&real),
        };
        match moved {
            Ok(()) => {
                deleted += 1;
                freed = freed.saturating_add(item.bytes);
            }
            Err(why) => fail(&mut failed, why),
        }
    }
    let quarantine_id = batch.and_then(|b| {
        let id = b.id().to_string();
        b.finish().map(|_| id)
    });
    Ok(CleanupOutcome {
        freed_bytes: freed,
        deleted,
        failed,
        quarantine_id,
    })
}

#[cfg(test)]
mod tests {
    use super::super::scan::{scan_category, ScanCtx};
    use super::super::testing::{backdate, now};
    use super::super::trash::FolderBin;
    use super::*;
    use dyno_core::cleanup::pack::{parse, Trust};
    use dyno_core::cleanup::{Os, SCAN_VALID_MS};
    use dyno_core::ports::PlatformError;
    use dyno_core::process::{Locker, ProcessRef};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    struct Locks(Vec<String>);

    impl FileLocks for Locks {
        fn lockers(&mut self, path: &Path) -> Result<Vec<Locker>, PlatformError> {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            Ok(self
                .0
                .iter()
                .filter(|l| name.starts_with(l.as_str()))
                .map(|l| Locker {
                    process: ProcessRef {
                        pid: 7,
                        started: Timestamp(1),
                    },
                    name: format!("{l}-app"),
                    kind: None,
                })
                .collect())
        }
    }

    struct Fx {
        _t: tempfile::TempDir,
        home: PathBuf,
        cache: PathBuf,
        env: Env,
        specs: Vec<CategorySpec>,
        q: Quarantine,
        bin: FolderBin,
    }

    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(t.path()).unwrap().join("home");
        let cache = home.join("cachey");
        std::fs::create_dir_all(&cache).unwrap();
        let env = Env {
            os: Some(Os::Linux),
            home: home.clone(),
            temp: home.join("tmp"),
            ..Default::default()
        };
        let specs = parse(
            "format = 1\n[[category]]\nid = \"t\"\nname = \"T\"\ndescription = \"d\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [\"{home}/cachey\"]\n",
            Trust::User,
        )
        .unwrap();
        let q = Quarantine::new(t.path().join("q"));
        let bin = FolderBin(t.path().join("bin"));
        Fx {
            _t: t,
            home,
            cache,
            env,
            specs,
            q,
            bin,
        }
    }

    fn file(f: &Fx, name: &str) -> PathBuf {
        let p = f.cache.join(name);
        std::fs::write(&p, name.as_bytes()).unwrap();
        backdate(&p, 3);
        p
    }

    fn scan(f: &Fx, at: Timestamp) -> CleanupScan {
        let ctx = ScanCtx {
            env: &f.env,
            now: at,
            dev_age_days: 30,
            deadline: Instant::now() + Duration::from_secs(20),
            owner: None,
        };
        let r = scan_category(&f.specs[0], &ctx, "s");
        let total = r.bytes;
        CleanupScan {
            id: "s".into(),
            at,
            results: vec![r],
            total_bytes: total,
        }
    }

    fn ids(s: &CleanupScan) -> Vec<String> {
        s.results[0].items.iter().map(|i| i.id.clone()).collect()
    }

    fn go(f: &Fx, s: &CleanupScan, pick: &[String], mode: DeleteMode, at: Timestamp, locked: &[&str]) -> Result<CleanupOutcome, String> {
        let mut locks = Locks(locked.iter().map(|s| s.to_string()).collect());
        run(
            s,
            pick,
            mode,
            RunCtx {
                specs: &f.specs,
                env: &f.env,
                now: at,
                locks: &mut locks,
                trash: &f.bin,
                quarantine: &f.q,
                batch_id: "q-test".into(),
            },
        )
    }

    #[test]
    fn a_scan_alone_deletes_nothing() {
        let f = fx();
        file(&f, "a");
        file(&f, "b");
        let s = scan(&f, now());
        assert_eq!(s.results[0].item_count, 2);
        assert!(f.cache.join("a").exists() && f.cache.join("b").exists());
    }

    #[test]
    fn chosen_items_go_to_the_quarantine_and_come_back() {
        let f = fx();
        file(&f, "a");
        file(&f, "b");
        file(&f, "keep");
        let s = scan(&f, now());
        let pick: Vec<String> = s.results[0]
            .items
            .iter()
            .filter(|i| !i.path.ends_with("keep"))
            .map(|i| i.id.clone())
            .collect();
        let out = go(&f, &s, &pick, DeleteMode::Quarantine, now(), &[]).unwrap();
        assert_eq!((out.deleted, out.failed.len(), out.freed_bytes), (2, 0, 2));
        assert_eq!(out.quarantine_id.as_deref(), Some("q-test"));
        assert!(
            !f.cache.join("a").exists() && f.cache.join("keep").exists(),
            "only what was chosen goes"
        );
        assert_eq!(f.q.list()[0].item_count, 2);
        let back = f.q.restore("q-test").unwrap();
        assert_eq!(back.restored, 2);
        assert_eq!(std::fs::read(f.cache.join("a")).unwrap(), b"a");
    }

    #[test]
    fn the_recycle_bin_mode_uses_the_trash_port() {
        let f = fx();
        file(&f, "a");
        let s = scan(&f, now());
        let out = go(&f, &s, &ids(&s), DeleteMode::RecycleBin, now(), &[]).unwrap();
        assert_eq!((out.deleted, out.quarantine_id), (1, None));
        assert!(f.bin.0.join("a").exists() && !f.cache.join("a").exists());
    }

    #[test]
    fn a_stale_scan_is_refused_and_nothing_is_touched() {
        let f = fx();
        file(&f, "a");
        let at = now();
        let s = scan(&f, at);
        let err = go(&f, &s, &ids(&s), DeleteMode::Quarantine, at.plus(SCAN_VALID_MS + 1), &[]).unwrap_err();
        assert!(err.contains("more than an hour"), "{err}");
        assert!(f.cache.join("a").exists());
        assert!(f.q.list().is_empty());
        assert!(go(&f, &s, &[], DeleteMode::Quarantine, at, &[]).is_err());
        assert!(go(&f, &s, &["not-in-scan".into()], DeleteMode::Quarantine, at, &[])
            .unwrap_err()
            .contains("not part"));
    }

    #[test]
    fn a_locked_file_is_skipped_with_the_name_of_who_holds_it() {
        let f = fx();
        file(&f, "busy.db");
        file(&f, "idle");
        let s = scan(&f, now());
        let out = go(&f, &s, &ids(&s), DeleteMode::Quarantine, now(), &["busy"]).unwrap();
        assert_eq!(out.deleted, 1);
        assert_eq!(out.failed.len(), 1);
        assert!(out.failed[0].reason.contains("In use by busy-app"), "{:?}", out.failed);
        assert!(f.cache.join("busy.db").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_item_swapped_for_a_link_after_the_scan_is_refused_and_the_target_survives() {
        let f = fx();
        let p = file(&f, "swap");
        let s = scan(&f, now());
        let target = f.home.join("precious.txt");
        std::fs::write(&target, b"keep").unwrap();
        std::fs::remove_file(&p).unwrap();
        std::os::unix::fs::symlink(&target, &p).unwrap();
        let out = go(&f, &s, &ids(&s), DeleteMode::Quarantine, now(), &[]).unwrap();
        assert_eq!(out.deleted, 0);
        assert!(out.failed[0].reason.contains("link"), "{:?}", out.failed);
        assert!(target.exists() && p.symlink_metadata().is_ok());
        assert_eq!(out.quarantine_id, None, "an empty batch leaves nothing");
    }

    #[test]
    fn an_item_that_changed_or_vanished_after_the_scan_is_refused() {
        let f = fx();
        let a = file(&f, "a");
        let b = file(&f, "b");
        let s = scan(&f, now());
        std::fs::write(&a, b"changed since").unwrap();
        std::fs::remove_file(&b).unwrap();
        let out = go(&f, &s, &ids(&s), DeleteMode::Quarantine, now(), &[]).unwrap();
        assert_eq!(out.deleted, 0);
        let reasons: Vec<&str> = out.failed.iter().map(|x| x.reason.as_str()).collect();
        assert!(
            reasons.iter().any(|r| r.contains("changed after the scan")) && reasons.iter().any(|r| r.contains("no longer there")),
            "{reasons:?}"
        );
        assert!(a.exists());
    }

    #[test]
    fn a_forged_path_outside_the_roots_is_refused() {
        let f = fx();
        file(&f, "a");
        let mut s = scan(&f, now());
        let outside = f.home.join("outside.txt");
        std::fs::write(&outside, b"x").unwrap();
        backdate(&outside, 3);
        s.results[0].items[0].path = outside.to_string_lossy().into_owned();
        s.results[0].items[0].modified = Timestamp(i64::MAX);
        let out = go(&f, &s, &ids(&s), DeleteMode::Quarantine, now(), &[]).unwrap();
        assert_eq!(out.deleted, 0);
        assert!(out.failed[0].reason.contains("outside"), "{:?}", out.failed);
        assert!(outside.exists());
        // And a path with `..`.
        s.results[0].items[0].path = format!("{}/../outside.txt", f.cache.display());
        let out = go(&f, &s, &ids(&s), DeleteMode::Quarantine, now(), &[]).unwrap();
        assert!(out.failed[0].reason.contains("plain full path"));
        assert!(outside.exists());
    }

    #[test]
    fn an_unknown_category_and_duplicate_paths_are_handled() {
        let f = fx();
        file(&f, "a");
        let mut s = scan(&f, now());
        let dup = s.results[0].items[0].clone();
        let mut second = dup.clone();
        second.id = "dup".into();
        s.results[0].items.push(second);
        let out = go(&f, &s, &["s:t:0".into(), "dup".into()], DeleteMode::Quarantine, now(), &[]).unwrap();
        assert_eq!((out.deleted, out.failed.len()), (1, 0), "the same path is removed once");
        s.results[0].category = "gone".into();
        let out = go(&f, &s, &["s:t:0".into()], DeleteMode::Quarantine, now(), &[]).unwrap();
        assert!(out.failed[0].reason.contains("no longer known"));
        let _ = dup;
    }
}
