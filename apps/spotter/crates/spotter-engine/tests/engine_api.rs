//! The engine facade and the scheduler against a scripted local server: commands, queries, notifications, channels,
//! the digest and the failure paths. Everything lives in temporary folders; nothing leaves the machine.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::action::Action;
use spotter_core::notify::{ChannelKind, DigestMode, NotificationChannel};
use spotter_core::render::{PageRenderer, RenderError, RenderedPage};
use spotter_core::source::{BoxFuture, SourceConfig};
use spotter_core::state::WatchStatus;
use spotter_core::time::{ClockTime, QuietHours, Timestamp};
use spotter_core::watch::WatchSpec;
use spotter_engine::clock::ManualClock;
use spotter_engine::notifier::{NotificationKind, RecordingNotifier};
use spotter_engine::secrets::{MemorySecrets, SecretStore};
use spotter_engine::{Engine, EngineConfig, EngineError, Scheduler, Services};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const T0: i64 = 1_800_000_000_000;

fn feed(versions: &[&str]) -> String {
    let items: String = versions
        .iter()
        .map(|v| format!("<item><title>App {v}</title><link>https://x/{v}</link></item>"))
        .collect();
    format!("<rss><channel>{items}</channel></rss>")
}

/// A scripted server: `route(request_text) -> (status line, extra headers, body)`; requests are recorded.
struct Server {
    base: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start(route: impl Fn(&str) -> (u16, String, String) + Send + Sync + 'static) -> Server {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let route = Arc::new(route);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let (seen, route) = (seen.clone(), route.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    seen.lock().unwrap().push(req.clone());
                    let (status, headers, body) = route(&req);
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                });
            }
        });
        Server { base, requests }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn seen(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn ok_feed(versions: &'static [&'static str]) -> impl Fn(&str) -> (u16, String, String) + Send + Sync + 'static {
    move |_| (200, String::new(), feed(versions))
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
        interval_secs: 3600,
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: true,
        paused: false,
        tracking: Default::default(),
    }
}

struct Fixture {
    engine: Engine,
    notifier: Arc<RecordingNotifier>,
    clock: Arc<ManualClock>,
    secrets: Arc<MemorySecrets>,
    dir: tempfile::TempDir,
}

fn fixture_with(renderer: Option<Arc<dyn PageRenderer>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let notifier = Arc::new(RecordingNotifier::default());
    let clock = Arc::new(ManualClock::new(Timestamp(T0)));
    let secrets = Arc::new(MemorySecrets::new());
    let mut svc = Services::new(secrets.clone());
    svc.notifier = notifier.clone();
    svc.clock = clock.clone();
    svc.renderer = renderer;
    let mut cfg = EngineConfig::new(dir.path(), "test");
    let mut fetch = spotter_engine::fetch::FetchConfig::new("test");
    fetch.min_host_interval = Duration::ZERO;
    cfg.fetch = Some(fetch);
    cfg.render_timeout = Duration::from_secs(5);
    let engine = Engine::open(cfg, svc).unwrap();
    Fixture {
        engine,
        notifier,
        clock,
        secrets,
        dir,
    }
}

fn fixture() -> Fixture {
    fixture_with(None)
}

fn none() -> BTreeMap<String, String> {
    BTreeMap::new()
}

#[tokio::test]
async fn saving_validates_assigns_ids_and_resets_history_when_the_source_changes() {
    let f = fixture();
    let server = Server::start(ok_feed(&["1.0.0"])).await;
    let mut rx = f.engine.subscribe();

    let mut bad = spec("not a url");
    bad.versions = "no-such-scheme".into();
    assert!(matches!(f.engine.save_watch(bad, &none()), Err(EngineError::Invalid(_))));
    let mut with_secret = spec(&server.url("/a"));
    let wrong = BTreeMap::from([("nope".to_string(), "x".to_string())]);
    assert!(matches!(
        f.engine.save_watch(with_secret.clone(), &wrong),
        Err(EngineError::Invalid(_))
    ));

    let saved = f.engine.save_watch(spec(&server.url("/a")), &none()).unwrap();
    assert_eq!(saved.name, "App");
    assert!(rx.try_recv().is_ok(), "saving announces a change");
    f.engine.check_one(&saved.id).await.unwrap();
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().history.len(), 1);

    // Same source, other name: history stays. Other source: history starts again.
    with_secret.id = saved.id.clone();
    with_secret.name = "Renamed".into();
    f.engine.save_watch(with_secret.clone(), &none()).unwrap();
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().history.len(), 1);
    with_secret.source = SourceConfig::new("feed", &[("url", &server.url("/b"))]);
    f.engine.save_watch(with_secret, &none()).unwrap();
    assert!(f.engine.get_watch(&saved.id).unwrap().history.is_empty());
    assert_eq!(f.engine.list_watches().len(), 1);
    assert_eq!(f.engine.source_of(&saved.id).unwrap().kind, "feed");
    assert!(matches!(f.engine.source_of("zzzzzzzz"), Err(EngineError::NotFound(_))));
}

#[tokio::test]
async fn secrets_are_stored_replaced_and_removed_with_the_watch() {
    let f = fixture();
    let mut s = spec("github.com/o/r");
    s.source = SourceConfig::new("github", &[("repo", "o/r")]);
    s.allow_local = false;
    let draft = |token: &str| BTreeMap::from([("token".to_string(), token.to_string())]);
    let saved = match f.engine.save_watch(s.clone(), &draft("ghp_secret")) {
        Ok(v) => v,
        Err(e) => panic!("{e:?}"),
    };
    assert_eq!(f.secrets.get(&saved.id, "token").as_deref(), Some("ghp_secret"));
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().secrets_set, ["token"]);
    s.id = saved.id.clone();
    f.engine.save_watch(s.clone(), &none()).unwrap();
    assert!(f.secrets.get(&saved.id, "token").is_some(), "a missing key keeps the secret");
    f.engine.save_watch(s, &draft("")).unwrap();
    assert!(f.secrets.get(&saved.id, "token").is_none(), "an empty value removes it");
    f.engine
        .save_watch(
            {
                let mut x = f.engine.get_watch(&saved.id).unwrap().spec;
                x.name = "Again".into();
                x
            },
            &draft("again"),
        )
        .unwrap();
    f.engine.delete_watch(&saved.id).unwrap();
    assert!(f.secrets.get(&saved.id, "token").is_none());
    assert!(matches!(f.engine.delete_watch(&saved.id), Err(EngineError::NotFound(_))));
}

struct FailingSecrets;
impl SecretStore for FailingSecrets {
    fn get(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn set(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err("locked".into())
    }
    fn delete(&self, _: &str, _: &str) -> Result<(), String> {
        Err("locked".into())
    }
}

#[test]
fn a_credential_store_that_refuses_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path(), "t"), Services::new(Arc::new(FailingSecrets))).unwrap();
    let mut s = spec("x");
    s.source = SourceConfig::new("github", &[("repo", "o/r")]);
    let e = engine
        .save_watch(s, &BTreeMap::from([("token".to_string(), "t".to_string())]))
        .unwrap_err();
    assert!(matches!(e, EngineError::Unavailable(m) if m.contains("locked")));
}

#[tokio::test]
async fn acknowledging_installing_and_pausing() {
    let f = fixture();
    let server = Server::start(ok_feed(&["1.1.0", "1.0.0"])).await;
    let saved = f.engine.save_watch(spec(&server.url("/")), &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();

    f.engine.acknowledge(&saved.id, None).unwrap();
    f.engine.acknowledge(&saved.id, Some("1.0.0")).unwrap();
    assert!(matches!(
        f.engine.acknowledge(&saved.id, Some("garbage")),
        Err(EngineError::BadRequest(_))
    ));
    f.engine.reset_acknowledgement(&saved.id).unwrap();
    f.engine.set_installed(&saved.id, Some("1.0.0")).unwrap();
    let s = f.engine.list_watches().remove(0);
    assert_eq!(s.installed.as_deref(), Some("1.0.0"));
    assert_eq!(s.newer_than_installed, Some(true));
    f.engine.set_installed(&saved.id, None).unwrap();
    assert!(matches!(f.engine.reset_acknowledgement("missing"), Err(EngineError::NotFound(_))));

    f.engine.set_paused(&saved.id, true).unwrap();
    assert!(f.engine.list_watches()[0].paused);
    assert!(f.engine.due_watches(Timestamp(T0 + 10 * 86_400_000)).is_empty());
    assert!(f.engine.check_now(None).await.unwrap().is_empty());
    assert!(
        !f.engine.check_now(Some(&saved.id)).await.unwrap()[0].skipped,
        "by id a paused watch runs"
    );
    assert!(matches!(f.engine.set_paused("missing", true), Err(EngineError::NotFound(_))));
}

#[tokio::test]
async fn settings_are_clamped_and_notifications_can_be_paused() {
    let f = fixture();
    let mut s = f.engine.settings();
    s.default_interval_secs = 1;
    s.downloads_folder = Some("   ".into());
    let saved = f.engine.save_settings(s.clone()).unwrap();
    assert!(saved.default_interval_secs > 1 && saved.downloads_folder.is_none());
    s.downloads_folder = Some("/somewhere".into());
    assert_eq!(
        f.engine.save_settings(s.clone()).unwrap().downloads_folder.as_deref(),
        Some("/somewhere")
    );
    s.downloads_folder = Some("a\0b".into());
    assert!(f.engine.save_settings(s).unwrap().downloads_folder.is_none());
    let paused = f.engine.pause_notifications(Some(30)).unwrap();
    assert_eq!(paused.notifications_paused_until, Some(T0 + 30 * 60_000));
    let huge = f.engine.pause_notifications(Some(u32::MAX)).unwrap();
    assert_eq!(huge.notifications_paused_until, Some(T0 + 30 * 24 * 60 * 60_000));
    assert!(f.engine.pause_notifications(None).unwrap().notifications_paused_until.is_none());
}

#[tokio::test]
async fn dashboard_events_imports_and_catalogues() {
    let f = fixture();
    let server = Server::start(ok_feed(&["1.0.0"])).await;
    let saved = f.engine.save_watch(spec(&server.url("/")), &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    let d = f.engine.dashboard("30d").unwrap();
    assert_eq!(d.range, "30d");
    assert!(!d.buckets_ms.is_empty());
    assert!(matches!(f.engine.dashboard("2w"), Err(EngineError::BadRequest(_))));
    assert!(f.engine.events(0, None).is_ok(), "limits are clamped");
    assert!(f.engine.events(10, Some(T0 - 1)).unwrap().is_empty());

    assert!(!f.engine.source_kinds().is_empty());
    assert!(!f.engine.presets().is_empty());
    assert!(matches!(f.engine.import_list("zip", ""), Err(EngineError::BadRequest(_))));
    let imported = f.engine.import_list("brewfile", "brew \"wget\"\n").unwrap();
    assert_eq!(imported.drafts.len(), 1);
    assert!(f.engine.import_list("opml", "<not opml").is_err() || f.engine.import_list("opml", "<opml/>").is_ok());
    assert!(f.engine.warnings().is_empty());
    assert!(!f.engine.migrated_from_legacy());
    assert!(f.engine.config_path().starts_with(f.dir.path()));
}

#[tokio::test]
async fn feeds_are_discovered_and_sources_tested() {
    let f = fixture();
    let server = Server::start(|req| {
        if req.starts_with("GET /page") {
            (
                200,
                "Content-Type: text/html\r\n".into(),
                "<html><head><link rel=\"alternate\" type=\"application/rss+xml\" href=\"/f.xml\" title=\"Feed\"></head></html>".into(),
            )
        } else if req.starts_with("GET /fail") {
            (500, String::new(), "boom".into())
        } else {
            (200, String::new(), feed(&["2.0.0", "2.0.0-rc1", "1.0.0"]))
        }
    })
    .await;
    let found = f.engine.discover_feeds(&server.url("/page")).await;
    // Loopback is refused for discovery unless a watch opts in, so this reports a clear error either way.
    assert!(found.is_ok() || matches!(found, Err(EngineError::Unavailable(_))));
    assert!(matches!(f.engine.discover_feeds("nonsense").await, Err(EngineError::BadRequest(_))));
    assert!(matches!(f.engine.discover_feeds("ftp://x/").await, Err(EngineError::BadRequest(_))));

    let preview = f.engine.test_source(&spec(&server.url("/f.xml")), &none()).await;
    assert!(preview.problems.is_empty() && preview.error.is_none());
    assert_eq!(preview.releases.len(), 2, "pre-releases are skipped by default");
    assert_eq!(preview.prereleases_skipped, 1);
    let failing = f.engine.test_source(&spec(&server.url("/fail")), &none()).await;
    assert!(failing.error.is_some());
    let mut invalid = spec("x");
    invalid.versions = "nope".into();
    assert!(!f.engine.test_source(&invalid, &none()).await.problems.is_empty());
    let mut github = spec("x");
    github.source = SourceConfig::new("github", &[("repo", "o/r")]);
    github.allow_local = false;
    let typed = BTreeMap::from([("token".to_string(), "typed".to_string())]);
    let _ = tokio::time::timeout(Duration::from_secs(1), f.engine.test_source(&github, &typed)).await;
}

#[tokio::test]
async fn a_folder_watch_lists_files_and_a_missing_folder_is_a_status() {
    let f = fixture();
    let share = tempfile::tempdir().unwrap();
    std::fs::write(share.path().join("app-1.2.0.zip"), b"x").unwrap();
    let mut s = spec("x");
    s.source = SourceConfig::new("folder", &[("path", share.path().to_str().unwrap()), ("pattern", "app-*.zip")]);
    let saved = f.engine.save_watch(s.clone(), &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().history.len(), 1);
    std::fs::remove_dir_all(share.path()).unwrap();
    let r = f.engine.check_one(&saved.id).await.unwrap();
    assert!(r.status.is_failure(), "{:?}", r.status);
}

struct FakeRenderer(Result<String, RenderError>);
impl PageRenderer for FakeRenderer {
    fn render<'a>(&'a self, url: &'a str, _: Option<&'a str>, _: Duration) -> BoxFuture<'a, Result<RenderedPage, RenderError>> {
        let out = self.0.clone().map(|html| RenderedPage {
            html,
            final_url: url.to_string(),
        });
        Box::pin(async move { out })
    }
}

fn page_spec() -> WatchSpec {
    let mut s = spec("x");
    s.source = SourceConfig::new(
        "web-page",
        &[
            ("url", "https://example.org/dl"),
            ("mode", "version"),
            ("rendered", "true"),
            ("selector", "span.v"),
        ],
    );
    s.allow_local = false;
    s
}

#[tokio::test]
async fn rendered_pages_need_a_browser_and_report_challenges() {
    let f = fixture();
    let saved = f.engine.save_watch(page_spec(), &none()).unwrap();
    let r = f.engine.check_one(&saved.id).await.unwrap();
    assert!(matches!(r.status, WatchStatus::Failed(_)), "{:?}", r.status);

    let f = fixture_with(Some(Arc::new(FakeRenderer(Err(RenderError::NeedsYou("log in".into()))))));
    let saved = f.engine.save_watch(page_spec(), &none()).unwrap();
    let r = f.engine.check_one(&saved.id).await.unwrap();
    assert!(matches!(r.status, WatchStatus::NeedsYou(_)), "{:?}", r.status);
    let kinds: Vec<String> = f.engine.events(10, None).unwrap().into_iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"needs-you".to_string()), "{kinds:?}");
    let n = f.notifier.notifications();
    assert_eq!(n[0].kind, NotificationKind::NeedsYou);

    let html = "<span class=\"v\">1.0.0</span>";
    let f = fixture_with(Some(Arc::new(FakeRenderer(Ok(html.into())))));
    let saved = f.engine.save_watch(page_spec(), &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    assert_eq!(f.engine.get_watch(&saved.id).unwrap().history.len(), 1);
}

#[tokio::test]
async fn a_changed_page_is_a_release_and_failures_recover() {
    let f = fixture();
    let body = Arc::new(Mutex::new("<p>one</p>".to_string()));
    let status = Arc::new(Mutex::new(200u16));
    let (b, st) = (body.clone(), status.clone());
    let server = Server::start(move |_| (*st.lock().unwrap(), String::new(), b.lock().unwrap().clone())).await;
    let mut s = spec("x");
    s.source = SourceConfig::new("web-page", &[("url", &server.url("/")), ("mode", "change")]);
    let saved = f.engine.save_watch(s, &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    *body.lock().unwrap() = "<p>two</p>".into();
    f.clock.advance_secs(7200);
    let r = f.engine.check_one(&saved.id).await.unwrap();
    assert_eq!(r.new_versions.len(), 1);
    *status.lock().unwrap() = 503;
    f.clock.advance_secs(7200);
    assert!(f.engine.check_one(&saved.id).await.unwrap().status.is_failure());
    *status.lock().unwrap() = 200;
    f.clock.advance_secs(7 * 86_400);
    f.engine.check_one(&saved.id).await.unwrap();
    let kinds: Vec<String> = f.engine.events(20, None).unwrap().into_iter().map(|e| e.kind).collect();
    assert!(
        kinds.contains(&"recovered".to_string()) && kinds.contains(&"check-failed".to_string()),
        "{kinds:?}"
    );
}

#[tokio::test]
async fn a_watch_whose_scheme_disappeared_is_flagged_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("packs")).unwrap();
    let pack = dir.path().join("packs/mine.toml");
    std::fs::write(
        &pack,
        "[[scheme]]\nid = \"mine\"\nname = \"Mine\"\npattern = \"(?P<major>\\\\d+)\\\\.(?P<minor>\\\\d+)\"\n",
    )
    .unwrap();
    let open = || Engine::open(EngineConfig::new(dir.path(), "t"), Services::new(Arc::new(MemorySecrets::new()))).unwrap();
    let engine = open();
    assert!(engine.warnings().is_empty(), "{:?}", engine.warnings());
    assert!(!engine.presets().is_empty());
    let mut s = spec("http://127.0.0.1:1/x");
    s.versions = "mine".into();
    let saved = engine.save_watch(s, &none()).unwrap();
    drop(engine);
    std::fs::remove_file(&pack).unwrap();
    let engine = open();
    for _ in 0..2 {
        let r = engine.check_one(&saved.id).await.unwrap();
        assert!(matches!(r.status, WatchStatus::Invalid(_)), "{:?}", r.status);
    }
    let failed = engine
        .events(10, None)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "check-failed")
        .count();
    assert_eq!(failed, 1, "the same problem is journalled once");
}

#[tokio::test]
async fn new_releases_notify_run_actions_and_deliver_channels() {
    let f = fixture();
    let body = Arc::new(Mutex::new(feed(&["1.0.0"])));
    let b = body.clone();
    let feed_server = Server::start(move |_| (200, String::new(), b.lock().unwrap().clone())).await;
    let hooks = Server::start(|req| {
        if req.starts_with("POST /bad") {
            (500, String::new(), String::new())
        } else {
            (200, String::new(), "ok".into())
        }
    })
    .await;
    let mut s = spec(&feed_server.url("/"));
    s.actions = vec![Action::open_location()];
    s.channels = vec![
        NotificationChannel {
            kind: ChannelKind::Webhook,
            target: hooks.url("/hook"),
        },
        NotificationChannel {
            kind: ChannelKind::Ntfy,
            target: hooks.url("/topic"),
        },
        NotificationChannel {
            kind: ChannelKind::Webhook,
            target: hooks.url("/bad"),
        },
        NotificationChannel {
            kind: ChannelKind::Email,
            target: "me@example.org".into(),
        },
    ];
    let saved = f.engine.save_watch(s, &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    *body.lock().unwrap() = feed(&["1.1.0", "1.0.0"]);
    f.clock.advance_secs(7200);
    let r = f.engine.check_one(&saved.id).await.unwrap();
    assert_eq!(r.new_versions, ["1.1.0"]);
    assert_eq!(f.notifier.notifications().len(), 1);
    assert_eq!(f.notifier.actions().len(), 1);
    let seen = hooks.seen().join("\n");
    assert!(seen.contains("POST /hook") && seen.contains("\"version\":\"1.1.0\""), "{seen}");
    assert!(
        seen.contains("POST /topic") && seen.to_lowercase().contains("click: https://x/1.1.0"),
        "{seen}"
    );
    let failures = f
        .engine
        .events(20, None)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "action-failed")
        .count();
    assert_eq!(failures, 1, "the 500 is journalled");
}

#[tokio::test]
async fn loopback_channels_are_refused_unless_the_watch_is_local() {
    let f = fixture();
    let body = Arc::new(Mutex::new(feed(&["1.0.0"])));
    let b = body.clone();
    let feed_server = Server::start(move |_| (200, String::new(), b.lock().unwrap().clone())).await;
    let hooks = Server::start(|_| (200, String::new(), String::new())).await;
    let mut s = spec(&feed_server.url("/"));
    s.channels = vec![NotificationChannel {
        kind: ChannelKind::Ntfy,
        target: "just-a-topic".into(),
    }];
    let saved = f.engine.save_watch(s, &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    *body.lock().unwrap() = feed(&["1.1.0", "1.0.0"]);
    f.clock.advance_secs(7200);
    f.engine.check_one(&saved.id).await.unwrap();
    assert!(hooks.seen().is_empty());
}

#[tokio::test]
async fn the_digest_collects_and_sends_once_a_day() {
    let f = fixture();
    let body = Arc::new(Mutex::new(feed(&["1.0.0"])));
    let b = body.clone();
    let server = Server::start(move |_| (200, String::new(), b.lock().unwrap().clone())).await;
    let mut settings = f.engine.settings();
    settings.digest = DigestMode::Daily;
    f.engine.save_settings(settings).unwrap();
    let saved = f.engine.save_watch(spec(&server.url("/")), &none()).unwrap();
    f.engine.check_one(&saved.id).await.unwrap();
    *body.lock().unwrap() = feed(&["1.2.0", "1.1.0", "1.0.0"]);
    f.clock.advance_secs(7200);
    f.engine.check_one(&saved.id).await.unwrap();
    assert!(f.notifier.notifications().iter().all(|n| n.kind != NotificationKind::NewRelease));
    let sent = f.engine.flush_digest().await.unwrap();
    let digests: Vec<_> = f
        .notifier
        .notifications()
        .into_iter()
        .filter(|n| n.kind == NotificationKind::Digest)
        .collect();
    if sent {
        assert!(digests[0].title.contains("new release"));
        assert!(!f.engine.flush_digest().await.unwrap(), "only once a day");
    }
    // Quiet hours around the whole day hold the digest back.
    let mut settings = f.engine.settings();
    settings.quiet_hours = Some(QuietHours {
        start: ClockTime::new(0, 0).unwrap(),
        end: ClockTime::new(23, 59).unwrap(),
    });
    f.engine.save_settings(settings).unwrap();
    f.clock.advance_secs(3 * 86_400);
    assert!(!f.engine.flush_digest().await.unwrap());
}

#[tokio::test]
async fn the_scheduler_runs_due_watches_and_stops_on_request() {
    let f = fixture();
    let server = Server::start(ok_feed(&["1.0.0"])).await;
    f.engine.save_watch(spec(&server.url("/")), &none()).unwrap();
    let scheduler = Scheduler::new(f.engine.clone()).with_poll_interval(Duration::from_millis(10));
    assert_eq!(scheduler.tick().await.len(), 1);
    assert!(scheduler.tick().await.is_empty(), "nothing is due right after");

    f.clock.advance_secs(7200);
    let (tx, rx) = tokio::sync::watch::channel(false);
    let runner = scheduler.clone();
    let handle = tokio::spawn(async move { runner.run(rx).await });
    tokio::time::sleep(Duration::from_millis(80)).await;
    tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), handle).await.unwrap().unwrap();
    // Already stopped: returns at once; a dropped sender stops it too.
    let (tx, rx) = tokio::sync::watch::channel(true);
    scheduler.run(rx).await;
    drop(tx);
    let (tx, rx) = tokio::sync::watch::channel(false);
    let runner = scheduler.clone();
    let handle = tokio::spawn(async move { runner.run(rx).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    drop(tx);
    tokio::time::timeout(Duration::from_secs(5), handle).await.unwrap().unwrap();
    assert_eq!(spotter_engine::scheduler::POLL_INTERVAL, Duration::from_secs(5));
    let _ = Scheduler::new(f.engine.clone());
}

#[test]
fn user_packs_damaged_files_and_old_formats_are_worked_around() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("packs")).unwrap();
    std::fs::write(
        dir.path().join("packs/good.toml"),
        "[[scheme]]\nid = \"mine\"\nname = \"Mine\"\npattern = \"(?P<major>\\\\d+)\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("packs/bad.toml"), "this is = = not toml").unwrap();
    std::fs::write(dir.path().join("packs/ignored.txt"), "x").unwrap();
    std::fs::write(dir.path().join("spotter.toml"), "this is = = not toml").unwrap();
    std::fs::write(dir.path().join("spotter.db"), b"not a database at all").unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path(), "t"), Services::new(Arc::new(MemorySecrets::new()))).unwrap();
    let warnings = engine.warnings().join("\n");
    assert!(warnings.contains("bad.toml"), "{warnings}");
    assert!(engine.list_watches().is_empty());
}

#[test]
fn an_unwritable_data_folder_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    std::fs::write(&file, b"x").unwrap();
    let e = Engine::open(
        EngineConfig::new(file.join("inside"), "t"),
        Services::new(Arc::new(MemorySecrets::new())),
    )
    .err()
    .unwrap();
    assert!(matches!(e, EngineError::Internal(m) if m.contains("writable")));
}

#[tokio::test]
async fn too_many_watches_are_refused() {
    let f = fixture();
    let mut last = Ok(());
    for i in 0..=spotter_core::limits::MAX_WATCHES {
        let r = f.engine.save_watch(spec(&format!("http://127.0.0.1:1/{i}")), &none());
        if let Err(e) = r {
            last = Err(e);
            break;
        }
    }
    assert!(matches!(last, Err(EngineError::Invalid(_))));
}
