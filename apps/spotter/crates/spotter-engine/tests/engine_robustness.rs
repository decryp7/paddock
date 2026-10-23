//! Failure paths and long runs through the whole engine, against scripted local servers: a slow server, an oversize
//! body, a redirect loop, bad webhooks, a soak over simulated weeks, rendered pages with a fake renderer, DNS
//! rebinding, avatars (cached, expired, offline, hostile) and the e-mail channel. Temporary folders only; nothing leaves
//! the machine.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::avatar::ImageKind;
use spotter_core::notify::{ChannelKind, NotificationChannel, SmtpSecurity, SmtpSettings};
use spotter_core::render::{PageRenderer, RenderError, RenderedPage};
use spotter_core::source::{BoxFuture, SourceConfig, SourceError};
use spotter_core::state::WatchStatus;
use spotter_core::time::Timestamp;
use spotter_core::watch::WatchSpec;
use spotter_engine::avatar::{AvatarCache, AvatarSource, Entry, ABSENT_MS, FRESH_MS, MAX_ENTRIES};
use spotter_engine::clock::ManualClock;
use spotter_engine::fetch::{FetchConfig, FetchOptions, PoliteFetcher};
use spotter_engine::guard::GuardedResolver;
use spotter_engine::notifier::RecordingNotifier;
use spotter_engine::secrets::{MemorySecrets, SecretStore};
use spotter_engine::{Engine, EngineConfig, EngineError, Services};
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const T0: i64 = 1_800_000_000_000;

fn feed(versions: &[&str]) -> String {
    let items: String = versions
        .iter()
        .map(|v| format!("<item><title>App {v}</title><link>https://x/{v}</link></item>"))
        .collect();
    format!("<rss><channel>{items}</channel></rss>")
}

/// A scripted HTTP server: `route(path) -> (status, extra headers, body bytes)`, with an optional delay per path.
struct Server {
    base: String,
    hits: Arc<Mutex<Vec<String>>>,
}

type Route = dyn Fn(&str) -> (u16, String, Vec<u8>) + Send + Sync;

impl Server {
    async fn start(route: impl Fn(&str) -> (u16, String, Vec<u8>) + Send + Sync + 'static) -> Server {
        Server::start_slow(route, Duration::ZERO).await
    }

    async fn start_slow(route: impl Fn(&str) -> (u16, String, Vec<u8>) + Send + Sync + 'static, delay: Duration) -> Server {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let hits = Arc::new(Mutex::new(Vec::new()));
        let (seen, route): (_, Arc<Route>) = (hits.clone(), Arc::new(route));
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                let (seen, route) = (seen.clone(), route.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                    seen.lock().unwrap().push(path.clone());
                    tokio::time::sleep(delay).await;
                    let (status, headers, body) = route(&path);
                    let head = format!(
                        "HTTP/1.1 {status} X\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(&body).await;
                });
            }
        });
        Server { base, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn hits_of(&self, path: &str) -> usize {
        self.hits.lock().unwrap().iter().filter(|p| p.starts_with(path)).count()
    }
}

fn text(status: u16, body: impl Into<String>) -> (u16, String, Vec<u8>) {
    (status, String::new(), body.into().into_bytes())
}

fn feed_spec(url: &str) -> WatchSpec {
    WatchSpec {
        id: String::new(),
        name: "App".into(),
        source: SourceConfig::new("feed", &[("url", url)]),
        versions: "semver".into(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        schedule: spotter_core::schedule::Schedule::every(3600),
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

fn fixture(tweak: impl FnOnce(&mut FetchConfig), renderer: Option<Arc<dyn PageRenderer>>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let notifier = Arc::new(RecordingNotifier::default());
    let clock = Arc::new(ManualClock::new(Timestamp(T0)));
    let secrets = Arc::new(MemorySecrets::new());
    let mut svc = Services::new(secrets.clone());
    svc.notifier = notifier.clone();
    svc.clock = clock.clone();
    svc.renderer = renderer;
    let mut cfg = EngineConfig::new(dir.path(), "test");
    let mut fetch = FetchConfig::new("test");
    fetch.min_host_interval = Duration::ZERO;
    tweak(&mut fetch);
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

fn plain() -> Fixture {
    fixture(|_| {}, None)
}

fn none() -> BTreeMap<String, String> {
    BTreeMap::new()
}

fn kinds(f: &Fixture) -> Vec<String> {
    f.engine.events(200, None).unwrap().into_iter().map(|e| e.kind).collect()
}

// ---- fetching ----

#[tokio::test]
async fn a_slow_server_is_a_timeout_status_and_the_next_check_recovers() {
    let f = fixture(|c| c.request_timeout = Duration::from_millis(300), None);
    let slow = Server::start_slow(|_| text(200, feed(&["1.0.0"])), Duration::from_secs(3)).await;
    let id = f.engine.save_watch(feed_spec(&slow.url("/f")), &none()).unwrap().id;
    let started = std::time::Instant::now();
    let r = f.engine.check_one(&id).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "the timeout was honoured");
    match &r.status {
        WatchStatus::Unreachable(m) => assert!(m.contains("in time"), "{m}"),
        other => panic!("{other:?}"),
    }
    // A fast server at the same watch (the address changes the source, so use a second engine call on a new watch).
    let fast = Server::start(|_| text(200, feed(&["1.0.0"]))).await;
    let id2 = f.engine.save_watch(feed_spec(&fast.url("/f")), &none()).unwrap().id;
    assert!(matches!(f.engine.check_one(&id2).await.unwrap().status, WatchStatus::Ok));
    assert!(kinds(&f).contains(&"check-failed".to_string()));
}

#[tokio::test]
async fn an_oversize_body_fails_the_check_and_a_smaller_one_recovers() {
    let f = fixture(|c| c.max_body = 2048, None);
    let big = Arc::new(Mutex::new("x".repeat(100_000)));
    let b = big.clone();
    let server = Server::start(move |_| text(200, b.lock().unwrap().clone())).await;
    let id = f.engine.save_watch(feed_spec(&server.url("/f")), &none()).unwrap().id;
    let r = f.engine.check_one(&id).await.unwrap();
    assert!(r.status.is_failure(), "{:?}", r.status);
    assert!(f.engine.get_watch(&id).unwrap().history.is_empty());
    *big.lock().unwrap() = feed(&["1.0.0"]);
    f.clock.advance_secs(7 * 86_400);
    assert!(matches!(f.engine.check_one(&id).await.unwrap().status, WatchStatus::Ok));
    assert!(kinds(&f).contains(&"recovered".to_string()), "{:?}", kinds(&f));
}

#[tokio::test]
async fn a_redirect_loop_is_a_failed_check_not_a_hang() {
    let f = plain();
    let server = Server::start(|path| match path {
        "/a" => (302, "Location: /b\r\n".into(), Vec::new()),
        _ => (302, "Location: /a\r\n".into(), Vec::new()),
    })
    .await;
    let id = f.engine.save_watch(feed_spec(&server.url("/a")), &none()).unwrap().id;
    let r = f.engine.check_one(&id).await.unwrap();
    match r.status {
        WatchStatus::Failed(m) => assert!(m.contains("circle"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_name_that_changes_its_answer_to_this_computer_is_refused_at_connection_time() {
    // The first lookup (the guard's check) answers with a public address, the second (the connection) with loopback:
    // DNS rebinding. The connection must not be made.
    let server = Server::start(|_| text(200, "secret")).await;
    let port: u16 = server.base.rsplit(':').next().unwrap().parse().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let resolver = GuardedResolver::with_lookup(Arc::new(move |_name| {
        let n = c.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let ip: IpAddr = if n == 0 {
                "93.184.216.34".parse().unwrap()
            } else {
                "127.0.0.1".parse().unwrap()
            };
            Ok(vec![ip])
        })
    }));
    let mut cfg = FetchConfig::new("test");
    cfg.min_host_interval = Duration::ZERO;
    let fetcher = PoliteFetcher::with_resolver(cfg, resolver).unwrap();
    let request = spotter_core::source::HttpRequest::get(format!("http://rebind.test:{port}/x"));
    let e = fetcher.get(&request, &FetchOptions::default()).await.unwrap_err();
    assert!(matches!(e, SourceError::InvalidConfig(_)), "{e:?}");
    assert!(e.message().contains("local"), "{e}");
    assert!(calls.load(Ordering::SeqCst) >= 2, "the connection resolved the name itself");
    assert_eq!(server.hits_of("/x"), 0, "no request reached this computer");
}

#[tokio::test]
async fn a_local_watch_may_reach_this_computer_by_name() {
    let server = Server::start(|_| text(200, "hello")).await;
    let port: u16 = server.base.rsplit(':').next().unwrap().parse().unwrap();
    let resolver = GuardedResolver::with_lookup(Arc::new(|_| Box::pin(async { Ok(vec!["127.0.0.1".parse::<IpAddr>().unwrap()]) })));
    let mut cfg = FetchConfig::new("test");
    cfg.min_host_interval = Duration::ZERO;
    let fetcher = PoliteFetcher::with_resolver(cfg, resolver).unwrap();
    let request = spotter_core::source::HttpRequest::get(format!("http://localhost:{port}/x"));
    let local = FetchOptions {
        allow_local: true,
        ..FetchOptions::default()
    };
    assert!(fetcher.get(&request, &local).await.is_ok());
    let e = fetcher.get(&request, &FetchOptions::default()).await.unwrap_err();
    assert!(matches!(e, SourceError::InvalidConfig(_)), "{e:?}");
}

// ---- channels ----

#[tokio::test]
async fn bad_webhooks_are_journalled_and_the_good_ones_still_deliver() {
    let f = plain();
    let hook = Server::start(|path| if path == "/bad" { text(500, "no") } else { text(200, "ok") }).await;
    let versions = Arc::new(Mutex::new(vec!["1.0.0"]));
    let v = versions.clone();
    let feed_server = Server::start(move |_| text(200, feed(&v.lock().unwrap()))).await;
    let mut spec = feed_spec(&feed_server.url("/f"));
    spec.channels = vec![
        NotificationChannel {
            kind: ChannelKind::Webhook,
            target: hook.url("/bad"),
        },
        NotificationChannel {
            kind: ChannelKind::Webhook,
            target: "http://127.0.0.1:1/refused".into(),
        },
        NotificationChannel {
            kind: ChannelKind::Webhook,
            target: hook.url("/good"),
        },
    ];
    let id = f.engine.save_watch(spec, &none()).unwrap().id;
    f.engine.check_one(&id).await.unwrap();
    versions.lock().unwrap().push("1.1.0");
    f.clock.advance_secs(7200);
    let r = f.engine.check_one(&id).await.unwrap();
    assert_eq!(r.new_versions, vec!["1.1.0".to_string()]);
    assert_eq!(hook.hits_of("/good"), 1, "a failing channel does not stop the next one");
    let failed = f
        .engine
        .events(50, None)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "action-failed")
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert!(
        failed
            .iter()
            .all(|e| e.message.as_deref().is_some_and(|m| m.starts_with("Webhook channel:"))),
        "{failed:?}"
    );
    assert_eq!(f.notifier.notifications().len(), 1, "the OS notification is shown regardless");
}

// ---- a long run ----

#[tokio::test]
async fn a_soak_of_many_checks_keeps_history_events_and_notifications_bounded_and_exact() {
    let f = plain();
    let versions = Arc::new(Mutex::new(vec!["1.0.0".to_string()]));
    let v = versions.clone();
    let server = Server::start(move |_| {
        let all = v.lock().unwrap().clone();
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        text(200, feed(&refs))
    })
    .await;
    let id = f.engine.save_watch(feed_spec(&server.url("/f")), &none()).unwrap().id;
    f.engine.check_one(&id).await.unwrap();
    let mut released = 0;
    for step in 1..=400u32 {
        if step % 2 == 0 {
            released += 1;
            versions.lock().unwrap().insert(0, format!("1.{released}.0"));
        }
        f.clock.advance_secs(3600);
        let r = f.engine.check_one(&id).await.unwrap();
        assert!(matches!(r.status, WatchStatus::Ok), "step {step}: {:?}", r.status);
        assert_eq!(r.new_versions.len(), usize::from(step % 2 == 0), "step {step}");
    }
    let detail = f.engine.get_watch(&id).unwrap();
    assert!(
        detail.history.len() <= spotter_core::limits::MAX_HISTORY,
        "{}",
        detail.history.len()
    );
    assert_eq!(detail.history[0].version, format!("1.{released}.0"));
    assert_eq!(
        f.notifier.notifications().len(),
        released as usize,
        "one notification per release, none repeated"
    );
    assert!(detail.checks.len() <= spotter_core::limits::MAX_CHECK_LOG);
    assert!(f.engine.events(10_000, None).unwrap().len() <= spotter_core::journal::MAX_EVENTS);
    // The server saw exactly one request per check (no retries, no duplicates).
    assert_eq!(server.hits_of("/f"), 401);
}

// ---- rendered pages ----

struct LivePage(Arc<Mutex<Result<String, RenderError>>>);

impl PageRenderer for LivePage {
    fn render<'a>(&'a self, url: &'a str, _: Option<&'a str>, _: Duration) -> BoxFuture<'a, Result<RenderedPage, RenderError>> {
        let out = self.0.lock().unwrap().clone().map(|html| RenderedPage {
            html,
            final_url: url.to_string(),
        });
        Box::pin(async move { out })
    }
}

#[tokio::test]
async fn a_rendered_page_in_change_mode_reports_a_change_once_and_survives_a_render_failure() {
    let page = Arc::new(Mutex::new(Ok("<main><p>Price 10</p></main>".to_string())));
    let f = fixture(|_| {}, Some(Arc::new(LivePage(page.clone()))));
    let mut spec = feed_spec("x");
    spec.source = SourceConfig::new(
        "web-page",
        &[("url", "https://example.org/pricing"), ("mode", "change"), ("rendered", "true")],
    );
    spec.allow_local = false;
    let id = f.engine.save_watch(spec, &none()).unwrap().id;
    assert!(
        f.engine.check_one(&id).await.unwrap().new_versions.is_empty(),
        "the first check is the baseline"
    );
    f.clock.advance_secs(7200);
    assert!(
        f.engine.check_one(&id).await.unwrap().new_versions.is_empty(),
        "the same page is not a change"
    );

    *page.lock().unwrap() = Ok("<main><p>Price 12</p></main>".to_string());
    f.clock.advance_secs(7200);
    let r = f.engine.check_one(&id).await.unwrap();
    assert_eq!(r.new_versions.len(), 1, "{r:?}");
    f.clock.advance_secs(7200);
    assert!(
        f.engine.check_one(&id).await.unwrap().new_versions.is_empty(),
        "the change is reported once"
    );

    *page.lock().unwrap() = Err(RenderError::Failed("the page crashed".into()));
    f.clock.advance_secs(7200);
    assert!(f.engine.check_one(&id).await.unwrap().status.is_failure());
    *page.lock().unwrap() = Err(RenderError::Unavailable("no browser".into()));
    f.clock.advance_secs(7200);
    assert!(f.engine.check_one(&id).await.unwrap().status.is_failure());
    *page.lock().unwrap() = Ok("<main><p>Price 12</p></main>".to_string());
    f.clock.advance_secs(7 * 86_400);
    let r = f.engine.check_one(&id).await.unwrap();
    assert!(matches!(r.status, WatchStatus::Ok), "{:?}", r.status);
    assert!(r.new_versions.is_empty(), "coming back to the same page is not a new change");
    assert!(kinds(&f).contains(&"recovered".to_string()));
}

// ---- avatars ----

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}

fn image(bytes: Vec<u8>) -> (u16, String, Vec<u8>) {
    (200, "Content-Type: image/png\r\n".into(), bytes)
}

#[tokio::test]
async fn an_avatar_is_fetched_once_cached_on_disk_and_refreshed_after_a_week() {
    let f = plain();
    let icon = Arc::new(Mutex::new(png(32, 32)));
    let i = icon.clone();
    let server = Server::start(move |path| {
        if path == "/favicon.ico" {
            image(i.lock().unwrap().clone())
        } else {
            text(404, "")
        }
    })
    .await;
    let id = f.engine.save_watch(feed_spec(&server.url("/releases.atom")), &none()).unwrap().id;

    let first = f.engine.watch_avatar(&id).await.unwrap().unwrap();
    assert_eq!((first.source, first.mime), (AvatarSource::Remote, "image/png"));
    assert!(first.data_url().starts_with("data:image/png;base64,"));
    let again = f.engine.watch_avatar(&id).await.unwrap().unwrap();
    assert_eq!(again, first);
    assert_eq!(server.hits_of("/favicon.ico"), 1, "the second look used the cache");
    assert!(f.dir.path().join("cache/avatars").read_dir().unwrap().count() >= 2, "it is on disk");

    // A new engine on the same folder still has it: the cache survives a restart.
    drop(f.engine);
    let mut svc = Services::new(f.secrets.clone());
    svc.clock = f.clock.clone();
    let reopened = Engine::open(EngineConfig::new(f.dir.path(), "test"), svc).unwrap();
    assert_eq!(reopened.watch_avatar(&id).await.unwrap().unwrap(), first);
    assert_eq!(server.hits_of("/favicon.ico"), 1);

    *icon.lock().unwrap() = png(48, 48);
    f.clock.advance_secs((FRESH_MS / 1000) as u64 + 60);
    let refreshed = reopened.watch_avatar(&id).await.unwrap().unwrap();
    assert_ne!(refreshed.bytes, first.bytes);
    assert_eq!(server.hits_of("/favicon.ico"), 2);
}

#[tokio::test]
async fn offline_the_old_avatar_keeps_showing_and_with_none_the_bundled_mark_does() {
    let f = plain();
    let alive = Arc::new(AtomicUsize::new(1));
    let a = alive.clone();
    let server = Server::start(move |_| {
        if a.load(Ordering::SeqCst) == 1 {
            image(png(16, 16))
        } else {
            text(503, "down")
        }
    })
    .await;
    let id = f.engine.save_watch(feed_spec(&server.url("/f")), &none()).unwrap().id;
    let cached = f.engine.watch_avatar(&id).await.unwrap().unwrap();
    assert_eq!(cached.source, AvatarSource::Remote);

    // The site is down when the entry expires: the old picture stays, and is not asked for again at once.
    alive.store(0, Ordering::SeqCst);
    f.clock.advance_secs((FRESH_MS / 1000) as u64 + 60);
    let stale = f.engine.watch_avatar(&id).await.unwrap().unwrap();
    assert_eq!(stale, cached);
    let hits = server.hits_of("/favicon.ico");
    assert_eq!(f.engine.watch_avatar(&id).await.unwrap().unwrap(), cached);
    assert_eq!(server.hits_of("/favicon.ico"), hits, "an hour's grace after a failed refresh");

    // A watch whose site is not reachable at all and has no cache shows the bundled mark.
    let dead = f.engine.save_watch(feed_spec("http://127.0.0.1:1/f"), &none()).unwrap().id;
    let mark = f.engine.watch_avatar(&dead).await.unwrap().unwrap();
    assert_eq!((mark.source, mark.mime), (AvatarSource::Bundled, "image/svg+xml"));
}

#[tokio::test]
async fn hostile_or_unusable_pictures_are_never_shown_and_are_not_asked_for_again_at_once() {
    for (label, answer) in [
        (
            "an error page",
            (200u16, String::new(), b"<html><body>Not found</body></html>".to_vec()),
        ),
        (
            "a script in an svg",
            (
                200,
                String::new(),
                b"<svg xmlns='http://www.w3.org/2000/svg'><script>alert(1)</script></svg>".to_vec(),
            ),
        ),
        ("a huge image", (200, String::new(), png(9000, 9000))),
        ("a body over the cap", (200, String::new(), vec![0x89; 400_000])),
        ("not found", (404, String::new(), Vec::new())),
    ] {
        let f = plain();
        let a = answer.clone();
        let server = Server::start(move |_| a.clone()).await;
        let id = f.engine.save_watch(feed_spec(&server.url("/f")), &none()).unwrap().id;
        let got = f.engine.watch_avatar(&id).await.unwrap().unwrap();
        assert_eq!(got.source, AvatarSource::Bundled, "{label}");
        let hits = server.hits_of("/favicon.ico");
        assert_eq!(hits, 1, "{label}");
        f.engine.watch_avatar(&id).await.unwrap();
        assert_eq!(server.hits_of("/favicon.ico"), 1, "{label}: remembered as absent");
        f.clock.advance_secs((ABSENT_MS / 1000) as u64 + 60);
        f.engine.watch_avatar(&id).await.unwrap();
        assert_eq!(server.hits_of("/favicon.ico"), 2, "{label}: asked again after the absence expired");
    }
}

#[tokio::test]
async fn avatars_do_not_reach_local_addresses_for_a_watch_that_is_not_local() {
    let f = plain();
    let server = Server::start(|_| image(png(16, 16))).await;
    let mut spec = feed_spec(&server.url("/f"));
    spec.allow_local = false;
    // The watch itself would be refused at save? No: the address is only checked when fetching.
    let id = f.engine.save_watch(spec, &none()).unwrap().id;
    let got = f.engine.watch_avatar(&id).await.unwrap().unwrap();
    assert_eq!(got.source, AvatarSource::Bundled);
    assert_eq!(server.hits_of("/favicon.ico"), 0);
}

#[tokio::test]
async fn kinds_presets_and_unknown_ids_have_avatars_or_clear_errors() {
    let f = plain();
    for kind in ["folder", "github", "feed", "package", "container"] {
        let a = f.engine.kind_avatar(kind).unwrap();
        assert_eq!((a.source, a.mime), (AvatarSource::Bundled, "image/svg+xml"), "{kind}");
    }
    assert!(f.engine.kind_avatar("nope").is_none());
    assert!(matches!(f.engine.watch_avatar("missing").await, Err(EngineError::NotFound(_))));
    assert!(matches!(
        f.engine.preset_avatar("no-such-preset").await,
        Err(EngineError::NotFound(_))
    ));
    // A preset on a package registry has no site picture: it gets the mark for its kind, without any network.
    let mark = f.engine.preset_avatar("serde").await.unwrap().unwrap();
    assert_eq!((mark.source, mark.mime), (AvatarSource::Bundled, "image/svg+xml"));
}

#[test]
fn the_cache_ignores_damaged_files_and_trims_itself() {
    let dir = tempfile::tempdir().unwrap();
    let cache = AvatarCache::new(dir.path().join("a"));
    assert!(cache.load("nothing").is_none());
    let entry = Entry {
        fetched_ms: 5,
        image: Some((ImageKind::Png, png(8, 8))),
    };
    cache.store("k1", &entry);
    assert_eq!(cache.load("k1"), Some(entry.clone()));
    assert!(entry.fresh(5 + FRESH_MS - 1) && !entry.fresh(5 + FRESH_MS));
    assert!(!entry.fresh(0), "a clock that went backwards is not trusted");
    let absent = Entry {
        fetched_ms: 5,
        image: None,
    };
    cache.store("k2", &absent);
    assert_eq!(cache.load("k2"), Some(absent.clone()));
    assert!(absent.fresh(5 + ABSENT_MS - 1) && !absent.fresh(5 + ABSENT_MS));

    // Tampered image, wrong format, garbage metadata: all misses.
    std::fs::write(dir.path().join("a/k1.img"), b"<script>alert(1)</script>").unwrap();
    assert!(cache.load("k1").is_none());
    cache.store("k1", &entry);
    std::fs::write(dir.path().join("a/k1.img"), {
        let mut ico = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
        ico.extend_from_slice(&8u32.to_le_bytes());
        ico.extend_from_slice(&22u32.to_le_bytes());
        ico.extend_from_slice(&png(8, 8)[..8]);
        ico
    })
    .unwrap();
    assert!(cache.load("k1").is_none(), "the format on record no longer matches the bytes");
    std::fs::write(dir.path().join("a/k1.json"), b"{not json").unwrap();
    assert!(cache.load("k1").is_none());
    std::fs::remove_file(dir.path().join("a/k1.img")).unwrap();
    cache.store("k3", &entry);
    std::fs::remove_file(dir.path().join("a/k3.img")).unwrap();
    assert!(cache.load("k3").is_none(), "metadata without its image");

    // An absent answer replaces an image on disk.
    cache.store("k3", &entry);
    cache.store("k3", &absent);
    assert!(!dir.path().join("a/k3.img").exists());

    // A picture that cannot be written (its place is taken by a folder) is simply not cached, and leaves no temp file.
    std::fs::create_dir_all(dir.path().join("a/blocked.img")).unwrap();
    cache.store("blocked", &entry);
    assert!(cache.load("blocked").is_none());
    assert!(!dir.path().join("a/blocked.tmp").exists());

    // A cache folder that cannot be created only means no caching.
    let blocked = dir.path().join("file");
    std::fs::write(&blocked, b"x").unwrap();
    AvatarCache::new(blocked.join("inside")).store("k", &entry);

    for n in 0..MAX_ENTRIES + 20 {
        cache.store(&format!("bulk{n}"), &absent);
    }
    let json = std::fs::read_dir(dir.path().join("a"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .count();
    assert!(json <= MAX_ENTRIES, "{json}");
}

// ---- e-mail through the engine ----

/// A server that accepts mail and remembers the DATA of each message.
async fn smtp_inbox() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let inbox = Arc::new(Mutex::new(Vec::new()));
    let store = inbox.clone();
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = l.accept().await else { return };
            let store = store.clone();
            tokio::spawn(async move {
                let (r, mut w) = s.into_split();
                let mut lines = BufReader::new(r).lines();
                let _ = w.write_all(b"220 fake ESMTP\r\n").await;
                while let Ok(Some(line)) = lines.next_line().await {
                    let up = line.to_ascii_uppercase();
                    let reply: &[u8] = if up.starts_with("EHLO") {
                        b"250-fake\r\n250 8BITMIME\r\n"
                    } else if up.starts_with("DATA") {
                        let _ = w.write_all(b"354 go\r\n").await;
                        let mut data = String::new();
                        while let Ok(Some(l)) = lines.next_line().await {
                            if l == "." {
                                break;
                            }
                            data.push_str(&l);
                            data.push('\n');
                        }
                        store.lock().unwrap().push(data);
                        b"250 queued\r\n"
                    } else if up.starts_with("QUIT") {
                        let _ = w.write_all(b"221 bye\r\n").await;
                        return;
                    } else {
                        b"250 ok\r\n"
                    };
                    let _ = w.write_all(reply).await;
                }
            });
        }
    });
    (port, inbox)
}

fn smtp_settings(port: u16) -> SmtpSettings {
    SmtpSettings {
        host: "127.0.0.1".into(),
        port,
        security: SmtpSecurity::None,
        username: None,
        from: "spotter@example.test".into(),
    }
}

#[tokio::test]
async fn the_mail_settings_are_validated_and_the_test_message_arrives() {
    let f = plain();
    let (port, inbox) = smtp_inbox().await;
    assert_eq!(
        f.engine.send_test_email("me@example.test").await.unwrap_err().code(),
        "badRequest",
        "no server yet"
    );
    assert!(!f.engine.mail_status().configured);

    let mut settings = f.engine.settings();
    settings.smtp = Some(SmtpSettings {
        host: "  ".into(),
        ..smtp_settings(port)
    });
    assert_eq!(f.engine.save_settings(settings.clone()).unwrap_err().code(), "badRequest");
    assert!(f.engine.settings().smtp.is_none(), "an invalid server is not saved");

    settings.smtp = Some(SmtpSettings {
        host: " 127.0.0.1 ".into(),
        from: " spotter@example.test ".into(),
        ..smtp_settings(port)
    });
    let saved = f.engine.save_settings(settings).unwrap();
    assert_eq!(saved.smtp.as_ref().unwrap().host, "127.0.0.1", "trimmed");
    assert!(f.engine.mail_status().configured && !f.engine.mail_status().password_set);

    assert_eq!(f.engine.send_test_email("not an address").await.unwrap_err().code(), "badRequest");
    f.engine.send_test_email("me@example.test").await.unwrap();
    let got = inbox.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert!(got[0].contains("Subject: Spotter test message"), "{}", got[0]);

    // A server that is not there is "unavailable" with the next step in the message.
    let mut settings = f.engine.settings();
    settings.smtp = Some(smtp_settings(1));
    f.engine.save_settings(settings).unwrap();
    let e = f.engine.send_test_email("me@example.test").await.unwrap_err();
    assert_eq!(e.code(), "unavailable");
    assert!(e.to_string().contains("could not be reached"), "{e}");
}

#[tokio::test]
async fn the_mail_password_lives_in_the_credential_store_and_goes_with_the_server() {
    let f = plain();
    assert!(!f.engine.mail_status().password_set);
    f.engine.set_mail_password(Some("hunter2")).unwrap();
    assert!(f.engine.mail_status().password_set);
    assert_eq!(f.secrets.get("app", "smtp-password").as_deref(), Some("hunter2"));
    let config = std::fs::read_to_string(f.engine.config_path()).unwrap_or_default();
    assert!(!config.contains("hunter2"), "never in the configuration file");

    let mut settings = f.engine.settings();
    settings.smtp = Some(smtp_settings(2525));
    f.engine.save_settings(settings.clone()).unwrap();
    assert!(!std::fs::read_to_string(f.engine.config_path()).unwrap().contains("hunter2"));
    assert!(f.engine.mail_status().password_set, "saving other settings keeps it");

    settings.smtp = None;
    f.engine.save_settings(settings).unwrap();
    assert!(!f.engine.mail_status().password_set, "switching mail off forgets the password");

    f.engine.set_mail_password(Some("again")).unwrap();
    f.engine.set_mail_password(None).unwrap();
    assert!(!f.engine.mail_status().password_set);
    f.engine.set_mail_password(Some("again")).unwrap();
    f.engine.set_mail_password(Some("")).unwrap();
    assert!(!f.engine.mail_status().password_set, "an empty password removes it");
}

struct RefusingStore;

impl SecretStore for RefusingStore {
    fn get(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn set(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
        Err("the keychain is locked".into())
    }
    fn delete(&self, _: &str, _: &str) -> Result<(), String> {
        Err("the keychain is locked".into())
    }
}

#[test]
fn a_credential_store_that_refuses_the_mail_password_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path(), "test"), Services::new(Arc::new(RefusingStore))).unwrap();
    let e = engine.set_mail_password(Some("x")).unwrap_err();
    assert_eq!(e.code(), "unavailable");
    assert!(e.to_string().contains("credential store"), "{e}");
    assert!(engine.set_mail_password(None).is_err());
}

#[tokio::test]
async fn an_email_channel_gets_the_release_and_without_a_server_it_is_a_journalled_failure() {
    let f = plain();
    let versions = Arc::new(Mutex::new(vec!["1.0.0"]));
    let v = versions.clone();
    let server = Server::start(move |_| text(200, feed(&v.lock().unwrap()))).await;
    let mut spec = feed_spec(&server.url("/f"));
    spec.channels = vec![NotificationChannel {
        kind: ChannelKind::Email,
        target: "me@example.test".into(),
    }];
    let id = f.engine.save_watch(spec, &none()).unwrap().id;
    f.engine.check_one(&id).await.unwrap();

    // No mail server yet.
    versions.lock().unwrap().insert(0, "1.1.0");
    f.clock.advance_secs(7200);
    f.engine.check_one(&id).await.unwrap();
    let failures: Vec<_> = f
        .engine
        .events(20, None)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "action-failed")
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].message.as_deref().unwrap().contains("Set up the mail server"),
        "{failures:?}"
    );

    // With one, the message arrives with the release in it.
    let (port, inbox) = smtp_inbox().await;
    let mut settings = f.engine.settings();
    settings.smtp = Some(smtp_settings(port));
    f.engine.save_settings(settings).unwrap();
    versions.lock().unwrap().insert(0, "1.2.0");
    f.clock.advance_secs(7200);
    f.engine.check_one(&id).await.unwrap();
    let got = inbox.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].contains("1.2.0"), "{}", got[0]);
    assert!(got[0].contains("Sent by Spotter"));
    assert_eq!(
        f.engine
            .events(20, None)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "action-failed")
            .count(),
        1,
        "no new failure"
    );
}
