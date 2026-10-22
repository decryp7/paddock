//! Schedules in the engine: the three modes drive `next_check`, a machine that slept gets one catch-up check, daylight
//! saving follows the machine's zone, an edited schedule counts from the last check, and the preview is a query.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::calendar::Weekday;
use spotter_core::schedule::Schedule;
use spotter_core::source::SourceConfig;
use spotter_core::time::{ClockTime, Timestamp};
use spotter_core::watch::WatchSpec;
use spotter_engine::clock::{Clock, ManualClock, SequenceRandom};
use spotter_engine::notifier::RecordingNotifier;
use spotter_engine::secrets::MemorySecrets;
use spotter_engine::{Engine, EngineConfig, EngineError, Scheduler, Services};
use std::collections::BTreeMap;
use std::sync::Arc;

fn ts(text: &str) -> Timestamp {
    Timestamp::parse(text).unwrap()
}

fn t(h: u8, m: u8) -> ClockTime {
    ClockTime::new(h, m).unwrap()
}

/// A watch on a port nobody listens on: every check fails fast, which is all the schedule tests need.
fn spec(schedule: Schedule) -> WatchSpec {
    WatchSpec {
        id: String::new(),
        name: "App".into(),
        source: SourceConfig::new("feed", &[("url", "http://127.0.0.1:1/feed.xml")]),
        versions: "semver".into(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        schedule,
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: true,
        paused: false,
        tracking: Default::default(),
    }
}

fn engine(dir: &std::path::Path, now: &str, zone: Option<&str>) -> (Engine, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(ts(now)));
    clock.set_local_zone_name(zone);
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = Arc::new(RecordingNotifier::default());
    svc.clock = clock.clone();
    // The same jitter every time: 0.5.
    svc.random = Arc::new(SequenceRandom::default());
    let mut cfg = EngineConfig::new(dir, "test");
    let mut fetch = spotter_engine::fetch::FetchConfig::new("test");
    fetch.min_host_interval = std::time::Duration::ZERO;
    cfg.fetch = Some(fetch);
    (Engine::open(cfg, svc).unwrap(), clock)
}

fn next_of(engine: &Engine, id: &str) -> Option<Timestamp> {
    engine
        .list_watches()
        .into_iter()
        .find(|w| w.id == id)
        .and_then(|w| w.next_check_ms)
        .map(Timestamp)
}

#[tokio::test]
async fn an_at_times_watch_is_next_checked_at_its_time_plus_jitter() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, clock) = engine(dir.path(), "2026-10-22T08:00:00Z", None);
    let weekdays = Schedule::Times {
        days: Weekday::WEEKDAYS.to_vec(),
        times: vec![t(9, 0), t(14, 0)],
        zone: Some("UTC".into()),
    };
    let saved = engine.save_watch(spec(weekdays), &BTreeMap::new()).unwrap();
    assert_eq!(saved.schedule_text, "Weekdays at 09:00 and 14:00 (UTC)");
    engine.check_one(&saved.id).await.unwrap();
    // Jitter 0.5 of 30 s on top of the nominal 09:00 (the failure backoff of two minutes is shorter than the wait).
    assert_eq!(next_of(&engine, &saved.id), Some(ts("2026-10-22T09:00:15Z")));
    clock.set(ts("2026-10-22T08:59:00Z"));
    assert!(engine.due_watches(clock.now()).is_empty());
    clock.set(ts("2026-10-22T09:00:15Z"));
    assert_eq!(engine.due_watches(clock.now()), std::slice::from_ref(&saved.id));
}

#[tokio::test]
async fn a_machine_that_slept_runs_one_catch_up_check_not_a_burst() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, clock) = engine(dir.path(), "2026-10-22T08:00:00Z", None);
    let hourly = Schedule::Cron {
        expr: "0 * * * *".into(),
        zone: Some("UTC".into()),
    };
    let saved = engine.save_watch(spec(hourly), &BTreeMap::new()).unwrap();
    let scheduler = Scheduler::new(engine.clone());
    assert_eq!(scheduler.tick().await.len(), 1, "never checked: due at once");
    assert_eq!(next_of(&engine, &saved.id), Some(ts("2026-10-22T09:00:15Z")));
    // The lid is closed for three days: seventy-two runs are missed.
    clock.set(ts("2026-10-25T10:30:00Z"));
    assert_eq!(scheduler.tick().await.len(), 1, "one catch-up check on wake");
    let next = next_of(&engine, &saved.id).unwrap();
    assert!(next > clock.now(), "the missed runs are not replayed");
    assert_eq!(next, ts("2026-10-25T11:00:15Z"));
    assert!(scheduler.tick().await.is_empty(), "and nothing more until the next run");
}

#[tokio::test]
async fn daylight_saving_follows_the_machines_zone() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _clock) = engine(dir.path(), "2026-03-28T12:00:00Z", Some("Europe/Berlin"));
    let daily = Schedule::Times {
        days: Weekday::ALL.to_vec(),
        times: vec![t(2, 30)],
        zone: None,
    };
    let saved = engine.save_watch(spec(daily), &BTreeMap::new()).unwrap();
    engine.check_one(&saved.id).await.unwrap();
    // Sunday 2026-03-29: 02:30 does not exist in Berlin; the run happens at 03:30 CEST (01:30 UTC).
    assert_eq!(next_of(&engine, &saved.id), Some(ts("2026-03-29T01:30:15Z")));

    let (fall, _clock) = engine_at(dir.path().join("fall"), "2026-10-24T12:00:00Z", Some("Europe/Berlin"));
    let daily = Schedule::Times {
        days: Weekday::ALL.to_vec(),
        times: vec![t(2, 30)],
        zone: None,
    };
    let saved = fall.save_watch(spec(daily), &BTreeMap::new()).unwrap();
    fall.check_one(&saved.id).await.unwrap();
    // 02:30 happens twice on 2026-10-25; the first (CEST, 00:30 UTC) counts.
    assert_eq!(next_of(&fall, &saved.id), Some(ts("2026-10-25T00:30:15Z")));
}

fn engine_at(dir: std::path::PathBuf, now: &str, zone: Option<&str>) -> (Engine, Arc<ManualClock>) {
    std::fs::create_dir_all(&dir).unwrap();
    engine(&dir, now, zone)
}

#[tokio::test]
async fn without_a_zone_name_the_machines_offset_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, clock) = engine(dir.path(), "2026-10-22T08:00:00Z", None);
    clock.set_utc_offset_minutes(120);
    let daily = Schedule::Times {
        days: Weekday::ALL.to_vec(),
        times: vec![t(12, 0)],
        zone: None,
    };
    let saved = engine.save_watch(spec(daily), &BTreeMap::new()).unwrap();
    engine.check_one(&saved.id).await.unwrap();
    assert_eq!(next_of(&engine, &saved.id), Some(ts("2026-10-22T10:00:15Z")));
}

#[tokio::test]
async fn an_expression_that_fires_too_often_is_clamped_to_the_source_minimum() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _clock) = engine(dir.path(), "2026-10-22T08:00:05Z", None);
    let every_second = Schedule::Cron {
        expr: "* * * * * *".into(),
        zone: Some("UTC".into()),
    };
    let saved = engine.save_watch(spec(every_second), &BTreeMap::new()).unwrap();
    engine.check_one(&saved.id).await.unwrap();
    let next = next_of(&engine, &saved.id).unwrap();
    // A site is checked at most once a minute, and a failed check backs off further (two minutes after one failure).
    assert!(next.0 - ts("2026-10-22T08:00:05Z").0 >= 120_000, "{next:?}");
}

#[tokio::test]
async fn changing_the_schedule_counts_from_the_last_check() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, clock) = engine(dir.path(), "2026-10-22T08:00:00Z", None);
    let saved = engine.save_watch(spec(Schedule::every(6 * 3600)), &BTreeMap::new()).unwrap();
    engine.check_one(&saved.id).await.unwrap();
    let before = next_of(&engine, &saved.id).unwrap();
    assert!(before > ts("2026-10-22T10:00:00Z"), "a long wait after a failed check");
    clock.advance_secs(3600);
    let mut edited = spec(Schedule::every(900));
    edited.id = saved.id.clone();
    let summary = engine.save_watch(edited, &BTreeMap::new()).unwrap();
    // 15 minutes after the check at 08:00, already past: due now, once.
    assert!(summary.next_check_ms.unwrap() <= clock.now().0, "{summary:?}");
    assert_eq!(engine.due_watches(clock.now()), std::slice::from_ref(&saved.id));
    // Saving the same schedule again changes nothing.
    let mut same = spec(Schedule::every(900));
    same.id = saved.id.clone();
    let before = next_of(&engine, &saved.id);
    engine.save_watch(same, &BTreeMap::new()).unwrap();
    assert_eq!(next_of(&engine, &saved.id), before);
}

#[tokio::test]
async fn invalid_schedules_are_refused_with_what_to_fix() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _) = engine(dir.path(), "2026-10-22T08:00:00Z", None);
    for (schedule, needle) in [
        (
            Schedule::Cron {
                expr: "* * *".into(),
                zone: None,
            },
            "five fields",
        ),
        (
            Schedule::Cron {
                expr: "0 9 * * *".into(),
                zone: Some("Mars/Base".into()),
            },
            "Unknown time zone",
        ),
        (
            Schedule::Times {
                days: vec![],
                times: vec![t(9, 0)],
                zone: None,
            },
            "at least one day",
        ),
    ] {
        match engine.save_watch(spec(schedule), &BTreeMap::new()) {
            Err(EngineError::Invalid(problems)) => assert!(problems.join(" ").contains(needle), "{problems:?}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(engine.list_watches().is_empty());
}

#[tokio::test]
async fn the_preview_is_a_query_with_the_source_minimum() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _) = engine(dir.path(), "2026-10-22T08:00:00Z", Some("Europe/Berlin"));
    let cron = Schedule::Cron {
        expr: "0 9 * * 1-5".into(),
        zone: None,
    };
    let p = engine.preview_schedule(&cron, "github").unwrap();
    assert_eq!(p.next_runs[0], ts("2026-10-23T07:00:00Z"));
    assert_eq!((p.next_runs.len(), p.zone.as_str(), p.warnings.len()), (5, "Europe/Berlin", 0));
    // A folder may be scanned more often than a site is asked.
    let fast = Schedule::every(15);
    assert!(engine.preview_schedule(&fast, "folder").unwrap().warnings.is_empty());
    assert!(engine.preview_schedule(&fast, "github").unwrap().warnings[0].contains("at most every minute"));
    assert!(matches!(
        engine.preview_schedule(
            &Schedule::Cron {
                expr: String::new(),
                zone: None
            },
            "github"
        ),
        Err(EngineError::Invalid(_))
    ));
    assert!(engine.list_watches().is_empty(), "nothing was saved");
}
