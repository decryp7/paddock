//! The disk centre with the saved-maps cache (ADR 0029): saving at the end of a pass, offering saved maps at start, loading
//! them lazily, unloading under the memory bounds, Rescan as a replacement, a different disk, forgetting. Time is the cache's
//! fake clock; nothing sleeps.

use super::*;
use crate::cache::db::tests::Roomy;
use crate::cache::db::FreeSpace;
use crate::cache::testing::FakeClock;
use crate::cache::{CacheDeps, CacheSettings, SaveJob, SaveOutcome};

/// Starts the cache at `dir` and hands it to the rig's centre.
fn attach(r: &Rig, dir: &Path, free: Arc<dyn FreeSpace>, settings: CacheSettings) -> Cache {
    let cache = Cache::start(
        dir.join("cache.db"),
        settings,
        CacheDeps {
            clock: Arc::new(FakeClock::new(5_000)),
            free,
        },
    )
    .cache
    .expect("the cache starts");
    r.centre.attach_cache(Some(cache.clone()));
    cache
}

fn volume_target() -> DiskTarget {
    DiskTarget::Volume { id: "Macintosh HD".into() }
}

/// A fresh rig over the same fake disk, as after an agent restart; `old` is dropped first so its writer thread has ended.
fn restarted(old: Rig, fs: &Arc<FakeFs>, dir: &Path, identity: Option<&str>) -> (Rig, Cache) {
    drop(old);
    let mut v = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    v.identity = identity.map(str::to_string);
    let r = rig(fs.clone(), vec![v]);
    let cache = attach(&r, dir, Arc::new(Roomy), CacheSettings::default());
    (r, cache)
}

fn scanned_and_saved(fs: &Arc<FakeFs>, dir: &Path, identity: Option<&str>) -> Rig {
    let mut v = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    v.identity = identity.map(str::to_string);
    let r = rig(fs.clone(), vec![v]);
    let cache = attach(&r, dir, Arc::new(Roomy), CacheSettings::default());
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    cache.flush();
    r
}

#[test]
fn a_finished_scan_is_saved_when_its_pass_ends() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let r = rig(fs.clone(), vec![vol("Macintosh HD", &fs.root(), VolumeLocation::Internal)]);
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    assert!(cache.list().is_empty());
    let s = r.centre.start(volume_target()).unwrap();
    let done = wait_final(&r, s.scan);
    cache.flush();
    let rows = cache.list();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].params.target_key, "volume:Macintosh HD");
    assert_eq!(rows[0].params.scanned_at, done.scanned_at.unwrap().millis());
    assert_eq!(rows[0].params.volume_identity, None);
    assert!(rows[0].params.used_bytes_then.is_none(), "the fake lister gives no used space now");
    let stored: DiskScanStatus = serde_json::from_str(&rows[0].params.status).unwrap();
    assert_eq!(stored.state, ScanState::Done);
    assert_eq!(stored.progress.bytes, done.progress.bytes);
}

#[test]
fn a_cancelled_scan_is_saved_and_says_it_is_partial() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let gate = Gate::new();
    let g = gate.clone();
    fs.on_read(move |p| {
        if p.ends_with("a") {
            g.pass();
        }
    });
    let d = tempfile::tempdir().unwrap();
    let mut v = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    v.volume.rotational = Some(true);
    let r = rig(fs.clone(), vec![v]);
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let s = r.centre.start(volume_target()).unwrap();
    gate.wait_arrived(1);
    r.centre.cancel(s.scan).unwrap();
    final_status(&r, s.scan);
    cache.flush();
    gate.open();
    let rows = cache.list();
    assert_eq!(rows.len(), 1, "a first scan that was cancelled is kept, as partial");
    let stored: DiskScanStatus = serde_json::from_str(&rows[0].params.status).unwrap();
    assert_eq!(stored.state, ScanState::Cancelled);
    let live = lock(&r.centre.scans)[0].walk().unwrap();
    live.wait();
}

#[test]
fn a_saved_map_is_offered_at_start_without_loading_its_tree_and_loads_on_the_first_level() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let first = scanned_and_saved(&fs, d.path(), None);
    let before = {
        let s = first.centre.scans();
        first.centre.children(s[0].scan, None, 0, 10, 0.0).unwrap()
    };
    let reads_before = fs.reads.load(Ordering::SeqCst);
    let (r, _cache) = restarted(first, &fs, d.path(), None);
    let offered = r.centre.scans();
    assert_eq!(offered.len(), 1);
    assert!(
        matches!(offered[0].origin, ScanOrigin::Saved { saved_at } if saved_at.millis() == 5_000),
        "{:?}",
        offered[0].origin
    );
    assert_eq!(offered[0].state, ScanState::Done);
    assert_eq!(offered[0].name, "Macintosh HD");
    let e = lock(&r.centre.scans)[0].clone();
    assert!(!e.loaded(), "offered without reading the tree");
    let page = r.centre.children(offered[0].scan, None, 0, 10, 0.0).unwrap();
    assert!(e.loaded(), "the first level asked for loads it");
    assert_eq!(page, before, "the same answer as the scan gave");
    assert_eq!(fs.reads.load(Ordering::SeqCst), reads_before, "a saved map reads no folder");
}

#[test]
fn a_saved_map_has_the_net_change_in_used_space_since_it_was_scanned() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let mut v = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    v.volume.capacity = Some(1000 * MB);
    v.volume.free = Some(400 * MB);
    let r = rig(fs.clone(), vec![v]);
    *lock(&r.lister.2) = Some((1000 * MB, 500 * MB));
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    cache.flush();
    assert_eq!(cache.list()[0].params.used_bytes_then, Some(500 * MB));
    let (r2, _c) = restarted(r, &fs, d.path(), None);
    assert_eq!(r2.centre.scans()[0].changed_bytes, None, "the volume list is not known yet");
    r2.centre.volumes();
    // The list says 600 MB used now: 100 MB more than then. A net figure.
    assert_eq!(r2.centre.scans()[0].changed_bytes, Some(100 * MB as i64));
}

/// Sixteen saved maps of folders, written straight to the cache.
fn sixteen(dir: &Path) -> Vec<String> {
    let cache = Cache::start(
        dir.join("cache.db"),
        CacheSettings::default(),
        CacheDeps {
            clock: Arc::new(FakeClock::new(100)),
            free: Arc::new(Roomy),
        },
    )
    .cache
    .unwrap();
    let mut paths = Vec::new();
    for n in 0..16u32 {
        let path = format!("/folder/{n}");
        let mut index = DiskIndex::new(format!("folder-{n}"), ScanLimits::default());
        let root = index.root();
        let sub = index.add_folder(root, "inner").unwrap();
        index.add_file(
            sub,
            "file",
            dyno_core::disk::index::Sizes::same(50 * MB + u64::from(n)),
            Timestamp(7),
        );
        index.finish_listing(sub);
        index.finish_listing(root);
        let status = DiskScanStatus {
            scan: 1,
            seq: 1,
            name: format!("folder-{n}"),
            target: DiskTarget::Folder { path: path.clone() },
            volume: None,
            state: ScanState::Done,
            phase: ScanPhase::ReadingFolders,
            progress: ScanProgress::default(),
            truncated: false,
            unreadable_first: vec![],
            needs_full_disk_access: false,
            items_per_second: None,
            eta_seconds: None,
            hidden_bytes: None,
            scanned_at: Some(Timestamp(1_000 + i64::from(n))),
            origin: ScanOrigin::Live,
            changed_bytes: None,
            replaces: None,
            not_saved: None,
        };
        let (tx, rx) = std::sync::mpsc::channel();
        cache.save(SaveJob {
            params: crate::cache::db::SaveParams {
                target_key: format!("folder:{path}"),
                target: serde_json::to_string(&DiskTarget::Folder { path: path.clone() }).unwrap(),
                volume_identity: None,
                status: serde_json::to_string(&status).unwrap(),
                limits: "{}".into(),
                scanned_at: 1_000 + i64::from(n),
                used_bytes_then: None,
            },
            index: Arc::new(index),
            scan: 1,
            kind: "folder",
            done: Box::new(move |o| {
                let _ = tx.send(o);
            }),
        });
        assert!(matches!(rx.recv().unwrap(), SaveOutcome::Saved { .. }));
        paths.push(path);
    }
    cache.flush();
    paths
}

#[test]
fn sixteen_saved_maps_are_all_listed_and_each_answers_in_turn_without_one_ever_being_removed() {
    let d = tempfile::tempdir().unwrap();
    sixteen(d.path());
    let fs = Arc::new(FakeFs::new());
    let r = rig(fs.clone(), vec![]);
    let _cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let listed = r.centre.scans();
    assert_eq!(listed.len(), 16, "every saved map is a kept scan");
    assert!(listed.iter().all(|s| matches!(s.origin, ScanOrigin::Saved { .. })));
    let loaded = |r: &Rig| lock(&r.centre.scans).iter().filter(|e| e.loaded()).count();
    assert_eq!(loaded(&r), 0, "none is in memory");
    for round in 0..2 {
        for s in &listed {
            let page = r
                .centre
                .children(s.scan, None, 0, 10, 0.0)
                .unwrap_or_else(|e| panic!("round {round}: {} {e:?}", s.name));
            assert_eq!(page.node.name, s.name);
            assert!(loaded(&r) <= MAX_KEPT, "a load unloads the oldest loaded one");
        }
    }
    assert_eq!(r.centre.scans().len(), 16, "nothing was removed, no id answers 'no longer kept'");
    assert_eq!(loaded(&r), MAX_KEPT);
}

#[test]
fn saved_maps_are_unloaded_under_the_node_budget() {
    let d = tempfile::tempdir().unwrap();
    sixteen(d.path());
    let fs = Arc::new(FakeFs::new());
    let r = rig(fs.clone(), vec![]);
    let _cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    // Each map holds 3 nodes; a budget of 7 holds two.
    r.centre.set_kept_nodes(7);
    let ids: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
    for id in &ids[..5] {
        r.centre.children(*id, None, 0, 10, 0.0).unwrap();
    }
    let loaded = lock(&r.centre.scans).iter().filter(|e| e.loaded()).count();
    assert_eq!(loaded, 2);
    assert_eq!(r.centre.scans().len(), 16);
}

/// A free-space port that parks the cache's writer thread at the start of a save.
struct Parked(Arc<Gate>);

impl FreeSpace for Parked {
    fn volume_of(&self, _: &Path) -> Option<(u64, u64)> {
        self.0.pass();
        Some((1 << 40, 1 << 39))
    }
}

#[test]
fn a_save_does_not_hold_the_index_so_a_query_on_the_same_scan_completes_meanwhile() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let r = rig(fs.clone(), vec![vol("Macintosh HD", &fs.root(), VolumeLocation::Internal)]);
    let gate = Gate::new();
    let cache = attach(&r, d.path(), Arc::new(Parked(gate.clone())), CacheSettings::default());
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    gate.wait_arrived(1);
    // The writer is parked inside the save, with the tree in its hands: the scan answers anyway.
    let page = r.centre.children(s.scan, None, 0, 10, 0.0).unwrap();
    assert_eq!(page.children.len(), 3);
    assert!(r.centre.rings(s.scan, None, 2, 0.0, 100).is_ok());
    let e = lock(&r.centre.scans)[0].clone();
    assert!(e.kept.pending(), "its save has not ended, so it is not unloaded for room");
    gate.open();
    cache.flush();
    assert_eq!(cache.list().len(), 1);
    assert!(!e.kept.pending());
    assert!(e.kept.row().is_some());
}

#[test]
fn a_finished_scan_with_a_saved_row_is_unloaded_not_removed_and_loads_again() {
    let fs = Arc::new(FakeFs::new());
    let mut volumes = Vec::new();
    for n in 0..=MAX_KEPT {
        let root = fs.dir(&fs.root(), format!("v{n}"));
        fs.file(&root, "big", 5 * MB + n as u64);
        volumes.push(vol(&format!("V{n}"), &root, VolumeLocation::Internal));
    }
    let d = tempfile::tempdir().unwrap();
    let r = rig(fs.clone(), volumes);
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let mut scans = Vec::new();
    for n in 0..=MAX_KEPT {
        let s = r.centre.start(DiskTarget::Volume { id: format!("V{n}") }).unwrap();
        wait_final(&r, s.scan);
        // Saved before the next scan makes room for itself.
        cache.flush();
        scans.push(s.scan);
    }
    let loaded = lock(&r.centre.scans).iter().filter(|e| e.loaded()).count();
    assert_eq!(
        loaded, MAX_KEPT,
        "one more than the memory bound: the least recently used went back to being saved"
    );
    assert_eq!(r.centre.scans().len(), MAX_KEPT + 1, "nothing was removed");
    let first = scans[0];
    let page = r.centre.children(first, None, 0, 10, 0.0).unwrap();
    assert_eq!(page.node.bytes, 5 * MB);
    let loaded = lock(&r.centre.scans).iter().filter(|e| e.loaded()).count();
    assert_eq!(loaded, MAX_KEPT, "loading it again unloaded another");
}

#[test]
fn rescan_keeps_the_saved_map_until_the_new_pass_is_done_and_then_replaces_it() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let first = scanned_and_saved(&fs, d.path(), None);
    let (r, cache) = restarted(first, &fs, d.path(), None);
    let old = r.centre.scans()[0].clone();
    let old_row = cache.list()[0].clone();
    let gate = Gate::new();
    let g = gate.clone();
    fs.on_read(move |p| {
        if p.ends_with("a") {
            g.pass();
        }
    });
    let mut v = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    v.volume.rotational = Some(true);
    *lock(&r.lister.0) = vec![v];
    r.centre.volumes();
    let started = r.centre.start_replacing(volume_target(), Some(old.scan)).unwrap();
    assert_eq!(started.replaces, Some(old.scan));
    gate.wait_arrived(1);
    let now = r.centre.scans();
    assert_eq!(now.len(), 2, "the saved map and the growing one");
    assert!(
        r.centre.children(old.scan, None, 0, 10, 0.0).is_ok(),
        "the saved map is readable while the new pass runs"
    );
    gate.open();
    wait_final(&r, started.scan);
    cache.flush();
    let after = r.centre.scans();
    assert_eq!(after.len(), 1, "the new scan took its place");
    assert_eq!(after[0].scan, started.scan);
    let rows = cache.list();
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0].id, old_row.id, "the row was replaced");
    assert_eq!(
        r.centre.children(old.scan, None, 0, 10, 0.0).unwrap_err().code,
        ErrorCode::NotFound,
        "the replaced map is dropped, not just hidden: it cannot come back"
    );
    assert!(lock(&r.centre.scans).iter().all(|e| e.scan != old.scan));
}

#[test]
fn a_cancelled_rescan_keeps_the_saved_map_and_saves_nothing() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let first = scanned_and_saved(&fs, d.path(), None);
    let (r, cache) = restarted(first, &fs, d.path(), None);
    let old = r.centre.scans()[0].clone();
    let old_row = cache.list()[0].clone();
    let gate = Gate::new();
    let g = gate.clone();
    fs.on_read(move |p| {
        if p.ends_with("a") {
            g.pass();
        }
    });
    let mut v = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    v.volume.rotational = Some(true);
    *lock(&r.lister.0) = vec![v];
    r.centre.volumes();
    let started = r.centre.start_replacing(volume_target(), Some(old.scan)).unwrap();
    gate.wait_arrived(1);
    r.centre.cancel(started.scan).unwrap();
    assert_eq!(final_status(&r, started.scan).state, ScanState::Cancelled);
    cache.flush();
    gate.open();
    let rows = cache.list();
    assert_eq!(rows, vec![old_row], "the saved row is exactly as it was");
    let ids: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
    assert!(ids.contains(&old.scan), "the old map is still listed");
    assert!(r.centre.children(old.scan, None, 0, 10, 0.0).is_ok());
}

#[test]
fn a_map_of_a_different_disk_is_not_offered_and_comes_back_with_its_disk() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let first = scanned_and_saved(&fs, d.path(), Some("DISK-A"));
    let (r, cache) = restarted(first, &fs, d.path(), Some("DISK-B"));
    r.centre.volumes();
    assert!(r.centre.scans().is_empty(), "another disk is mounted under that id");
    let id = lock(&r.centre.scans)[0].scan;
    let e = r.centre.children(id, None, 0, 10, 0.0).unwrap_err();
    assert!(e.message.contains("different disk"), "{}", e.message);
    assert_eq!(cache.list().len(), 1, "it stays in the cache, in case the other disk returns");
    let mut a = vol("Macintosh HD", &fs.root(), VolumeLocation::Internal);
    a.identity = Some("DISK-A".into());
    *lock(&r.lister.0) = vec![a];
    r.centre.refresh();
    assert!(!r.centre.hidden(&lock(&r.centre.scans)[0].clone()));
    assert_eq!(r.centre.scans().len(), 1);
}

#[test]
fn forgetting_removes_the_row_and_the_entry() {
    let d = tempfile::tempdir().unwrap();
    sixteen(d.path());
    let fs = Arc::new(FakeFs::new());
    let r = rig(fs.clone(), vec![]);
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let ids: Vec<u64> = r.centre.scans().iter().map(|s| s.scan).collect();
    r.centre.forget_saved(ids[3]).unwrap();
    assert_eq!(r.centre.scans().len(), 15);
    assert_eq!(cache.list().len(), 15);
    assert_eq!(r.centre.children(ids[3], None, 0, 1, 0.0).unwrap_err().code, ErrorCode::NotFound);
    r.centre.forget_all_saved();
    assert!(r.centre.scans().is_empty());
    assert!(cache.list().is_empty());
}

#[test]
fn a_map_too_big_for_the_limit_says_so_in_its_status() {
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let r = rig(fs.clone(), vec![vol("Macintosh HD", &fs.root(), VolumeLocation::Internal)]);
    let cache = attach(
        &r,
        d.path(),
        Arc::new(Roomy),
        CacheSettings {
            enabled: true,
            max_bytes: 100,
        },
    );
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    cache.flush();
    // The note arrives as a later event of the same scan, sent by the writer before it answers the flush.
    let told = r
        .events
        .all()
        .into_iter()
        .find(|e| e.not_saved.is_some())
        .expect("an event says why it was not kept");
    assert_eq!(told.not_saved, Some(NotSaved::TooBig));
    assert_eq!(told.state, ScanState::Done);
    assert!(cache.list().is_empty());
    assert_eq!(r.centre.scans()[0].not_saved, Some(NotSaved::TooBig));
}

#[test]
fn a_saved_map_cannot_be_retried_only_scanned_again() {
    let d = tempfile::tempdir().unwrap();
    sixteen(d.path());
    let fs = Arc::new(FakeFs::new());
    let r = rig(fs.clone(), vec![]);
    let _cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let id = r.centre.scans()[0].scan;
    assert_eq!(r.centre.retry(id).unwrap_err().code, ErrorCode::BadRequest);
}

#[test]
fn a_damaged_saved_map_answers_scan_again_and_is_dropped_alone() {
    let d = tempfile::tempdir().unwrap();
    sixteen(d.path());
    {
        let raw = rusqlite::Connection::open(d.path().join("cache.db")).unwrap();
        raw.execute(
            "UPDATE disk_chunks SET data = X'01FFFF' WHERE saved = (SELECT id FROM disk_saved ORDER BY id LIMIT 1)",
            [],
        )
        .unwrap();
    }
    let fs = Arc::new(FakeFs::new());
    let r = rig(fs.clone(), vec![]);
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    let all = r.centre.scans();
    let mut bad = 0;
    for s in &all {
        if let Err(e) = r.centre.children(s.scan, None, 0, 10, 0.0) {
            assert!(e.message.contains("Scan again"), "{}", e.message);
            bad += 1;
        }
    }
    assert_eq!(bad, 1);
    cache.flush();
    assert_eq!(cache.list().len(), 15);
    assert_eq!(r.centre.scans().len(), 15);
}

#[test]
fn switching_the_setting_off_forgets_every_saved_map_and_on_again_saves_new_ones() {
    use dyno_core::disk::settings::SavedMapsSettings;
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let first = scanned_and_saved(&fs, d.path(), None);
    let (r, cache) = restarted(first, &fs, d.path(), None);
    assert_eq!(r.centre.scans().len(), 1);
    r.centre.apply_saved_maps(SavedMapsSettings {
        enabled: false,
        ..SavedMapsSettings::default()
    });
    assert!(r.centre.scans().is_empty(), "the saved map left the list");
    assert!(cache.list().is_empty(), "and the cache");
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    cache.flush();
    assert!(cache.list().is_empty(), "nothing is saved while it is off");
    r.centre.apply_saved_maps(SavedMapsSettings::default());
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    cache.flush();
    assert_eq!(cache.list().len(), 1);
}

#[test]
fn the_size_limit_setting_reaches_the_cache() {
    use dyno_core::disk::settings::SavedMapsSettings;
    let fs = Arc::new(FakeFs::new());
    tree(&fs, &fs.root());
    let d = tempfile::tempdir().unwrap();
    let r = rig(fs.clone(), vec![vol("Macintosh HD", &fs.root(), VolumeLocation::Internal)]);
    let cache = attach(&r, d.path(), Arc::new(Roomy), CacheSettings::default());
    // The smallest limit the setting allows is 64 MB: a small tree fits it, the limit is the setting's, not the default's.
    r.centre.apply_saved_maps(SavedMapsSettings {
        max_mb: 0,
        ..SavedMapsSettings::default()
    });
    let s = r.centre.start(volume_target()).unwrap();
    wait_final(&r, s.scan);
    cache.flush();
    assert_eq!(cache.list().len(), 1);
}
