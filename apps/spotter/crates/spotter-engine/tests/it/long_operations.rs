//! Long operations through the engine: "n of m" for checking everything, a live count while a folder is listed,
//! and Cancel that is heard in the middle of a settle wait.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::job::{Cancel, Job, Phase, Progress, Unit};
use spotter_core::source::SourceConfig;
use spotter_core::time::Timestamp;
use spotter_core::watch::WatchSpec;
use spotter_engine::clock::ManualClock;
use spotter_engine::notifier::RecordingNotifier;
use spotter_engine::secrets::MemorySecrets;
use spotter_engine::{Engine, EngineConfig, EngineError, Services};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fixture {
    engine: Engine,
    _data: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = Arc::new(RecordingNotifier::default());
    svc.clock = Arc::new(ManualClock::new(Timestamp(1_800_000_000_000)));
    Fixture {
        engine: Engine::open(EngineConfig::new(data.path(), "test"), svc).unwrap(),
        _data: data,
    }
}

fn spec(name: &str, dir: &Path, settle: u32) -> WatchSpec {
    WatchSpec {
        id: String::new(),
        name: name.into(),
        source: SourceConfig::new(
            "folder",
            &[
                ("path", &dir.to_string_lossy()),
                ("settleSeconds", &settle.to_string()),
                ("extensions", "zip"),
            ],
        ),
        versions: "semver".into(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        schedule: spotter_core::schedule::Schedule::every(60),
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: false,
        paused: false,
        tracking: Default::default(),
    }
}

fn folder_with(files: usize) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for i in 0..files {
        std::fs::write(d.path().join(format!("app-1.{i}.0.zip")), "x").unwrap();
    }
    d
}

type Seen = Arc<Mutex<Vec<Progress>>>;

fn recording(interval: Duration) -> (Job, Seen, Cancel) {
    let seen: Seen = Arc::default();
    let cancel = Cancel::new();
    let sink = seen.clone();
    let job = Job::new(cancel.clone(), move |p| sink.lock().unwrap().push(p.clone())).with_interval(interval);
    (job, seen, cancel)
}

#[tokio::test]
async fn checking_everything_counts_n_of_m() {
    let f = fixture();
    let dirs: Vec<_> = (0..6).map(|_| folder_with(2)).collect();
    for (i, d) in dirs.iter().enumerate() {
        f.engine.save_watch(spec(&format!("w{i}"), d.path(), 0), &BTreeMap::new()).unwrap();
    }
    let (job, seen, _) = recording(Duration::ZERO);
    let reports = f.engine.check_now_job(None, &job).await.unwrap();
    assert_eq!(reports.len(), 6);
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().all(|p| p.phase == Phase::Checking && p.unit == Unit::Count),
        "only the one scale: {seen:?}"
    );
    assert_eq!((seen[0].done, seen[0].total), (0, Some(6)), "m is known from the start");
    assert!(seen.windows(2).all(|w| w[0].done <= w[1].done));
    assert_eq!((seen.last().unwrap().done, seen.last().unwrap().total), (6, Some(6)));
    assert!(seen.iter().any(|p| p.done == 3), "n of m moves one watch at a time");
}

#[tokio::test]
async fn cancelling_check_all_stops_the_waiting_checks_quickly() {
    let f = fixture();
    // Each folder has a new file, so each check would wait 30 s for it to settle.
    let dirs: Vec<_> = (0..10).map(|_| folder_with(1)).collect();
    for (i, d) in dirs.iter().enumerate() {
        f.engine.save_watch(spec(&format!("w{i}"), d.path(), 30), &BTreeMap::new()).unwrap();
    }
    let (job, _, cancel) = recording(Duration::ZERO);
    let began = Instant::now();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        cancel.cancel();
    });
    let e = f.engine.check_now_job(None, &job).await.unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
    // Nothing is left marked as running.
    assert!(f.engine.list_watches().iter().all(|w| !w.checking));
}

#[tokio::test]
async fn a_single_check_shows_the_growing_count_and_is_not_marked_failed_when_cancelled() {
    let f = fixture();
    let d = folder_with(40);
    let saved = f.engine.save_watch(spec("one", d.path(), 30), &BTreeMap::new()).unwrap();
    let (job, seen, cancel) = recording(Duration::ZERO);
    let watcher = {
        let seen = seen.clone();
        tokio::spawn(async move {
            loop {
                if seen.lock().unwrap().iter().any(|p| p.phase == Phase::Settling) {
                    cancel.cancel();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };
    let began = Instant::now();
    let e = f.engine.check_one_job(&saved.id, &job).await.unwrap_err();
    watcher.await.unwrap();
    assert_eq!(e, EngineError::Cancelled);
    assert!(began.elapsed() < Duration::from_secs(10));
    let seen = seen.lock().unwrap();
    let found = seen.iter().rfind(|p| p.phase == Phase::Finding).unwrap();
    assert_eq!((found.done, found.total), (40, Some(40)));
    let after = f.engine.list_watches().into_iter().find(|w| w.id == saved.id).unwrap();
    assert!(!after.checking);
    assert_eq!(after.last_check_ms, None, "a cancelled check records nothing");
}

#[tokio::test]
async fn the_wizard_test_counts_entries_and_cancel_comes_back_as_a_message() {
    let f = fixture();
    let d = folder_with(30);
    let (job, seen, _) = recording(Duration::ZERO);
    let preview = f.engine.test_source_job(&spec("t", d.path(), 0), &BTreeMap::new(), &job).await;
    assert!(preview.error.is_none(), "{:?}", preview.error);
    assert!(seen.lock().unwrap().iter().any(|p| p.phase == Phase::Finding && p.done == 30));

    let (job, _, cancel) = recording(Duration::ZERO);
    cancel.cancel();
    let preview = f.engine.test_source_job(&spec("t", d.path(), 0), &BTreeMap::new(), &job).await;
    assert!(preview.error.is_some(), "a cancelled look says so instead of showing releases");
    assert!(preview.releases.is_empty());
}
