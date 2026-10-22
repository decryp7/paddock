//! A damaged history database or an unwritable configuration must come back as errors the user can act on, never
//! as a panic and never as a half-applied change that poisons later work. The failures are real: tables are dropped
//! under the running engine and the configuration file is replaced by a folder.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::notify::DigestMode;
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
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const EVENING: i64 = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000) + 20 * 3_600_000;

fn feed(versions: &[&str]) -> String {
    let items: String = versions
        .iter()
        .map(|v| format!("<item><title>App {v}</title><link>https://x/{v}</link></item>"))
        .collect();
    format!("<rss><channel>{items}</channel></rss>")
}

async fn server(body: Arc<Mutex<String>>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = s.read(&mut buf).await;
                let text = body.lock().unwrap().clone();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/feed.xml")
}

fn spec(url: &str) -> WatchSpec {
    WatchSpec {
        id: String::new(),
        name: "App".into(),
        source: SourceConfig::new("feed", &[("url", url)]),
        versions: "semver".into(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        interval_secs: 60,
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: true,
        paused: false,
        tracking: Default::default(),
    }
}

fn cfg(dir: &Path) -> EngineConfig {
    let mut cfg = EngineConfig::new(dir, "test");
    let mut fetch = spotter_engine::fetch::FetchConfig::new("test");
    fetch.min_host_interval = Duration::ZERO;
    cfg.fetch = Some(fetch);
    cfg
}

fn open(dir: &Path) -> (Engine, Arc<ManualClock>, Arc<RecordingNotifier>) {
    let notifier = Arc::new(RecordingNotifier::default());
    let clock = Arc::new(ManualClock::new(Timestamp(EVENING)));
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = notifier.clone();
    svc.clock = clock.clone();
    (Engine::open(cfg(dir), svc).unwrap(), clock, notifier)
}

/// Drops tables of the engine's database from a second connection.
fn drop_tables(dir: &Path, tables: &[&str]) {
    let c = rusqlite::Connection::open(dir.join("spotter.db")).unwrap();
    c.busy_timeout(Duration::from_secs(5)).unwrap();
    for t in tables {
        c.execute_batch(&format!("DROP TABLE {t};")).unwrap();
    }
}

/// Replaces the configuration file by a folder, so the next save cannot put its file in place.
fn block_config(engine: &Engine) {
    let p = engine.config_path();
    let _ = std::fs::remove_file(&p);
    std::fs::create_dir_all(p.join("blocker")).unwrap();
}

struct Setup {
    dir: tempfile::TempDir,
    engine: Engine,
    id: String,
    body: Arc<Mutex<String>>,
}

async fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let body = Arc::new(Mutex::new(feed(&["1.0.0"])));
    let url = server(body.clone()).await;
    let (engine, clock, _) = open(dir.path());
    let id = engine.save_watch(spec(&url), &BTreeMap::new()).unwrap().id;
    engine.check_one(&id).await.unwrap();
    clock.advance_secs(120);
    Setup { dir, engine, id, body }
}

fn is_internal(e: EngineError) -> bool {
    matches!(e, EngineError::Internal(_))
}

#[tokio::test]
async fn a_check_whose_bookkeeping_table_is_gone_fails_with_a_message() {
    for table in ["http_cache", "kv", "watch_state", "check_days", "events"] {
        let s = setup().await;
        drop_tables(s.dir.path(), &[table]);
        // A new release makes every bookkeeping step run (cache, page memory, state, counters, journal).
        *s.body.lock().unwrap() = feed(&["1.1.0", "1.0.0"]);
        let r = s.engine.check_one(&s.id).await;
        let e = r.unwrap_err();
        assert!(is_internal(e), "{table}");
        // The engine is still usable afterwards: reads that do not need the table work.
        assert!(!s.engine.list_watches().is_empty());
    }
}

#[tokio::test]
async fn commands_report_a_broken_history_database() {
    let s = setup().await;
    drop_tables(
        s.dir.path(),
        &[
            "events",
            "downloads",
            "check_days",
            "watch_state",
            "http_cache",
            "digest_queue",
            "kv",
        ],
    );
    assert!(s.engine.events(10, None).is_err());
    assert!(s.engine.dashboard("30d").is_err());
    assert!(s.engine.acknowledge(&s.id, None).is_err());
    assert!(s.engine.record_download(&s.id, "1.0.0", Ok(("/x".into(), 1, true))).is_err());
    assert!(s.engine.record_action_failure(&s.id, "open", "no").is_err());
    assert!(s.engine.set_installed(&s.id, Some("1.0.0")).is_err());
    assert!(s.engine.delete_watch(&s.id).is_err());
    let _ = s.engine.flush_digest().await;
}

#[tokio::test]
async fn a_digest_that_cannot_be_queued_or_taken_is_an_error() {
    let s = setup().await;
    let mut settings = s.engine.settings();
    settings.digest = DigestMode::Daily;
    s.engine.save_settings(settings).unwrap();
    drop_tables(s.dir.path(), &["digest_queue"]);
    *s.body.lock().unwrap() = feed(&["1.1.0", "1.0.0"]);
    assert!(
        s.engine.check_one(&s.id).await.is_err(),
        "the new release cannot be queued for the digest"
    );
    assert!(s.engine.flush_digest().await.is_err());
}

#[tokio::test]
async fn a_history_without_its_state_table_cannot_be_opened_and_says_so() {
    let s = setup().await;
    let dir = s.dir.path().to_path_buf();
    drop(s.engine);
    drop_tables(&dir, &["watch_state"]);
    let svc = Services::new(Arc::new(MemorySecrets::new()));
    let e = Engine::open(cfg(&dir), svc).err().expect("opening must fail");
    assert!(is_internal(e));
}

#[tokio::test]
async fn an_unwritable_configuration_is_reported_by_every_command_that_saves() {
    let s = setup().await;
    block_config(&s.engine);
    assert!(s.engine.set_paused(&s.id, true).is_err());
    assert!(s.engine.save_settings(s.engine.settings()).is_err());
    assert!(s.engine.pause_notifications(Some(5)).is_err());
    let mut edited = spec("http://127.0.0.1:1/other.xml");
    edited.id = s.id.clone();
    assert!(s.engine.save_watch(edited, &BTreeMap::new()).is_err());
    assert!(s.engine.save_watch(spec("http://127.0.0.1:1/new.xml"), &BTreeMap::new()).is_err());
    assert!(s.engine.delete_watch(&s.id).is_err());
}

#[tokio::test]
async fn a_configuration_that_cannot_be_saved_after_a_migration_stops_the_start() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("spotter.toml"), "path = \"/drops\"\nfile_regex = \".*\\\\.zip\"\n").unwrap();
    // The backup the migration keeps is a folder, and so is the temporary file name: the save cannot complete.
    std::fs::create_dir(dir.path().join("spotter.toml.tmp")).unwrap();
    let svc = Services::new(Arc::new(MemorySecrets::new()));
    let e = Engine::open(cfg(dir.path()), svc).err().expect("opening must fail");
    assert!(is_internal(e));
}
