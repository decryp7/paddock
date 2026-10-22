//! The engine against a local mock server: first check baselines, a new release notifies, 304 keeps state, the SSRF
//! guard refuses loopback unless the watch opts in.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::source::SourceConfig;
use spotter_core::watch::WatchSpec;
use spotter_engine::clock::{Clock, ManualClock};
use spotter_engine::notifier::RecordingNotifier;
use spotter_engine::secrets::MemorySecrets;
use spotter_engine::{Engine, EngineConfig, Services};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn feed(versions: &[&str]) -> String {
    let items: String = versions
        .iter()
        .map(|v| format!("<item><title>App {v}</title><link>https://x/{v}</link></item>"))
        .collect();
    format!("<rss><channel>{items}</channel></rss>")
}

async fn server(body: Arc<std::sync::Mutex<String>>, hits: Arc<AtomicUsize>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let (body, hits) = (body.clone(), hits.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                hits.fetch_add(1, Ordering::SeqCst);
                let text = body.lock().unwrap().clone();
                let etag = format!("\"{}\"", text.len());
                let resp = if req.contains(&format!("if-none-match: {etag}")) {
                    format!("HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nConnection: close\r\n\r\n")
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                        text.len()
                    )
                };
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/feed.xml")
}

fn spec(url: &str, allow_local: bool) -> WatchSpec {
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
        allow_local,
        paused: false,
        tracking: Default::default(),
    }
}

fn engine(dir: &std::path::Path) -> (Engine, Arc<RecordingNotifier>, Arc<ManualClock>) {
    let notifier = Arc::new(RecordingNotifier::default());
    let clock = Arc::new(ManualClock::new(spotter_core::time::Timestamp(1_800_000_000_000)));
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = notifier.clone();
    svc.clock = clock.clone();
    let mut cfg = EngineConfig::new(dir, "test");
    let mut fetch = spotter_engine::fetch::FetchConfig::new("test");
    fetch.min_host_interval = std::time::Duration::ZERO;
    cfg.fetch = Some(fetch);
    (Engine::open(cfg, svc).unwrap(), notifier, clock)
}

#[tokio::test]
async fn baseline_then_new_release_then_not_modified() {
    let dir = tempfile::tempdir().unwrap();
    let body = Arc::new(std::sync::Mutex::new(feed(&["1.0.0"])));
    let hits = Arc::new(AtomicUsize::new(0));
    let url = server(body.clone(), hits.clone()).await;
    let (engine, notifier, _clock) = engine(dir.path());
    let saved = engine.save_watch(spec(&url, true), &BTreeMap::new()).unwrap();
    let r = engine.check_one(&saved.id).await.unwrap();
    assert!(
        r.new_versions.is_empty() && notifier.notifications().is_empty(),
        "first check is a baseline"
    );
    *body.lock().unwrap() = feed(&["1.1.0", "1.0.0"]);
    let r = engine.check_one(&saved.id).await.unwrap();
    assert_eq!(r.new_versions, ["1.1.0"]);
    assert_eq!(notifier.notifications().len(), 1);
    // Unchanged body: the server answers 304 and nothing new happens.
    let r = engine.check_one(&saved.id).await.unwrap();
    assert!(r.new_versions.is_empty() && !r.skipped);
    assert_eq!(notifier.notifications().len(), 1);
    let detail = engine.get_watch(&saved.id).unwrap();
    assert_eq!(detail.history.len(), 2);
    assert_eq!(engine.events(10, None).unwrap()[0].kind, "new-release");
}

#[tokio::test]
async fn loopback_is_refused_unless_the_watch_opts_in() {
    let dir = tempfile::tempdir().unwrap();
    let body = Arc::new(std::sync::Mutex::new(feed(&["1.0.0"])));
    let url = server(body, Arc::new(AtomicUsize::new(0))).await;
    let (engine, _, _) = engine(dir.path());
    let saved = engine.save_watch(spec(&url, false), &BTreeMap::new()).unwrap();
    let r = engine.check_one(&saved.id).await.unwrap();
    assert!(matches!(r.status, spotter_core::state::WatchStatus::Invalid(_)), "{:?}", r.status);
}

#[tokio::test]
async fn due_watches_follow_backoff_and_the_clock() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, clock) = engine(dir.path());
    let saved = engine
        .save_watch(spec("http://127.0.0.1:1/feed.xml", true), &BTreeMap::new())
        .unwrap();
    assert_eq!(engine.due_watches(clock.now()).len(), 1);
    let r = engine.check_one(&saved.id).await.unwrap();
    assert!(
        matches!(r.status, spotter_core::state::WatchStatus::Unreachable(_)),
        "{:?}",
        r.status
    );
    assert!(engine.due_watches(clock.now()).is_empty());
    clock.advance_secs(24 * 3600);
    assert_eq!(engine.due_watches(clock.now()).len(), 1);
}

#[tokio::test]
async fn downloads_and_failed_actions_reach_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = engine(dir.path());
    let hits = Arc::new(AtomicUsize::new(0));
    let url = server(Arc::new(std::sync::Mutex::new(feed(&["1.0.0"]))), hits).await;
    let saved = engine.save_watch(spec(&url, true), &BTreeMap::new()).unwrap();
    engine
        .record_download(&saved.id, "1.0.0", Ok(("/tmp/x/app.bin".into(), 42, true)))
        .unwrap();
    engine.record_download(&saved.id, "1.0.0", Err("disk full".into())).unwrap();
    engine
        .record_action_failure(&saved.id, "run-command", "the program could not be started")
        .unwrap();
    let kinds: Vec<String> = engine.events(10, None).unwrap().into_iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"download-finished".to_string()), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| *k == "action-failed").count(), 2, "{kinds:?}");
    assert!(engine.record_action_failure("nope!", "x", "y").is_err());
}

#[tokio::test]
async fn a_known_release_can_be_looked_up_for_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = engine(dir.path());
    let url = server(
        Arc::new(std::sync::Mutex::new(feed(&["1.0.0", "1.1.0"]))),
        Arc::new(AtomicUsize::new(0)),
    )
    .await;
    let saved = engine.save_watch(spec(&url, true), &BTreeMap::new()).unwrap();
    engine.check_now(Some(&saved.id)).await.unwrap();
    let r = engine.release_of(&saved.id, "1.1.0").unwrap();
    assert_eq!(r.location, "https://x/1.1.0");
    assert!(matches!(
        engine.release_of(&saved.id, "9.9.9"),
        Err(spotter_engine::EngineError::NotFound(_))
    ));
    assert!(matches!(
        engine.release_of("missing", "1.0.0"),
        Err(spotter_engine::EngineError::NotFound(_))
    ));
}
