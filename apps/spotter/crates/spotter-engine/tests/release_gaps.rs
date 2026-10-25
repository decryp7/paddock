//! Last failure paths of the engine: a journal that refuses the record of a failed delivery, and servers that answer
//! "not modified" to a feed discovery or a source test.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

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
    server_with(body, "200 OK").await
}

async fn server_with(body: Arc<Mutex<String>>, status: &'static str) -> String {
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
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
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
        schedule: spotter_core::schedule::Schedule::every(60),
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

#[tokio::test]
async fn a_delivery_failure_the_journal_cannot_record_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let body = Arc::new(Mutex::new(feed(&["1.0.0"])));
    let url = server(body.clone()).await;
    let (engine, clock, _) = open(dir.path());
    let mut s = spec(&url);
    // Nothing listens on port 1: the webhook fails, and the failure is to be journalled.
    s.channels = vec![spotter_core::notify::NotificationChannel {
        kind: spotter_core::notify::ChannelKind::Webhook,
        target: "http://127.0.0.1:1/hook".into(),
    }];
    let id = engine.save_watch(s, &BTreeMap::new()).unwrap().id;
    engine.check_one(&id).await.unwrap();
    // Only the record of a failed action is refused, so everything before it works.
    let c = rusqlite::Connection::open(dir.path().join("spotter.db")).unwrap();
    c.execute_batch(
        "CREATE TRIGGER refuse_action_failures BEFORE INSERT ON events WHEN NEW.json LIKE '%action%failed%' \
         BEGIN SELECT RAISE(ABORT, 'journal full'); END;",
    )
    .unwrap();
    *body.lock().unwrap() = feed(&["1.1.0", "1.0.0"]);
    clock.advance_secs(7200);
    let e = engine.check_one(&id).await.unwrap_err();
    assert!(matches!(e, EngineError::Internal(_)), "{e:?}");
}

#[tokio::test]
async fn a_server_that_says_not_modified_has_no_releases_to_preview() {
    let dir = tempfile::tempdir().unwrap();
    let url = server_with(Arc::new(Mutex::new(String::new())), "304 Not Modified").await;
    let (engine, _, _) = open(dir.path());
    let preview = engine.test_source(&spec(&url), &BTreeMap::new()).await;
    assert!(preview.releases.is_empty(), "{preview:?}");
}
