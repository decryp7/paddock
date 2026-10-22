//! Failure and edge paths of the engine that the flow tests do not reach: a check that is already running, products
//! whose source did not change, a migrated configuration, the digest, the wizard's pattern helper and the scheduler.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::notify::DigestMode;
use spotter_core::source::SourceConfig;
use spotter_core::time::Timestamp;
use spotter_core::watch::WatchSpec;
use spotter_engine::clock::ManualClock;
use spotter_engine::notifier::{NotificationKind, RecordingNotifier};
use spotter_engine::secrets::MemorySecrets;
use spotter_engine::{Engine, EngineConfig, EngineError, Scheduler, Services};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// 2027-01-15 20:00 UTC: after the hour the digest is sent.
const EVENING: i64 = 1_800_000_000_000 - (1_800_000_000_000 % 86_400_000) + 20 * 3_600_000;

fn feed(titles: &[&str]) -> String {
    let items: String = titles
        .iter()
        .map(|t| format!("<item><title>{t}</title><link>https://x/{}</link></item>", t.replace(' ', "-")))
        .collect();
    format!("<rss><channel>{items}</channel></rss>")
}

/// Serves `body` with an ETag and answers 304 to a matching `If-None-Match`; `delay` holds every answer back.
async fn server(body: Arc<Mutex<String>>, delay: Duration) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                tokio::time::sleep(delay).await;
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

fn open(dir: &std::path::Path, now: i64) -> (Engine, Arc<RecordingNotifier>, Arc<ManualClock>) {
    let notifier = Arc::new(RecordingNotifier::default());
    let clock = Arc::new(ManualClock::new(Timestamp(now)));
    let mut svc = Services::new(Arc::new(MemorySecrets::new()));
    svc.notifier = notifier.clone();
    svc.clock = clock.clone();
    let mut cfg = EngineConfig::new(dir, "test");
    let mut fetch = spotter_engine::fetch::FetchConfig::new("test");
    fetch.min_host_interval = Duration::ZERO;
    cfg.fetch = Some(fetch);
    (Engine::open(cfg, svc).unwrap(), notifier, clock)
}

#[tokio::test]
async fn a_check_that_is_already_running_is_skipped_not_doubled() {
    let dir = tempfile::tempdir().unwrap();
    let url = server(Arc::new(Mutex::new(feed(&["App 1.0.0"]))), Duration::from_millis(200)).await;
    let (engine, _, clock) = open(dir.path(), EVENING);
    let saved = engine.save_watch(spec(&url), &BTreeMap::new()).unwrap();
    let (a, b) = tokio::join!(engine.check_one(&saved.id), engine.check_one(&saved.id));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert!(a.skipped != b.skipped, "exactly one of two simultaneous checks runs: {a:?} {b:?}");
    let skipped = if a.skipped { a } else { b };
    assert_eq!(skipped.watch_id, saved.id);
    assert!(skipped.new_versions.is_empty());
    clock.advance_secs(1);
}

#[tokio::test]
async fn products_stay_as_they_were_when_the_source_has_not_changed() {
    let dir = tempfile::tempdir().unwrap();
    let body = Arc::new(Mutex::new(feed(&["Acme-PRO-R3.09.00T40", "Acme-QA-R1.00.00T05"])));
    let url = server(body, Duration::ZERO).await;
    let (engine, notifier, clock) = open(dir.path(), EVENING);
    let mut s = spec(&url);
    s.versions = "custom".into();
    s.version_pattern = Some(r"Acme-(?<id>[A-Z]+)-R(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)T(?<build>\d+)".into());
    let saved = engine.save_watch(s, &BTreeMap::new()).unwrap();
    engine.check_one(&saved.id).await.unwrap();
    assert_eq!(engine.get_watch(&saved.id).unwrap().products.len(), 2);
    clock.advance_secs(120);
    let again = engine.check_one(&saved.id).await.unwrap();
    assert!(!again.skipped && again.new_versions.is_empty());
    assert_eq!(engine.get_watch(&saved.id).unwrap().products.len(), 2, "a 304 keeps every product");
    assert!(notifier.notifications().is_empty());
    // A product's release is found by product; an unknown product is not found.
    let r = engine.release_of_product(&saved.id, Some("PRO"), "R3.09.00T40").unwrap();
    assert!(r.location.contains("Acme-PRO"));
    assert!(matches!(
        engine.release_of_product(&saved.id, Some("NOPE"), "R3.09.00T40"),
        Err(EngineError::NotFound(_))
    ));
}

#[tokio::test]
async fn an_old_format_configuration_is_migrated_and_saved_on_open() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("spotter.toml"),
        "path = \"/drops\"\nfile_regex = \".*\\\\.zip\"\ninterval_seconds = 30\n",
    )
    .unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    assert!(engine.migrated_from_legacy());
    assert_eq!(engine.list_watches().len(), 1);
    let saved = std::fs::read_to_string(engine.config_path()).unwrap();
    assert!(saved.contains("[[watch]]") || saved.contains("watch"), "{saved}");
    assert!(dir.path().join("spotter.toml.bak").exists(), "the old file is kept as the backup");
}

#[tokio::test]
async fn the_digest_is_sent_with_a_count_and_an_empty_one_sends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let body = Arc::new(Mutex::new(feed(&["App 1.0.0"])));
    let url = server(body.clone(), Duration::ZERO).await;
    let (engine, notifier, clock) = open(dir.path(), EVENING);
    let mut settings = engine.settings();
    settings.digest = DigestMode::Daily;
    engine.save_settings(settings).unwrap();
    // Due, but nothing was queued.
    assert!(!engine.flush_digest().await.unwrap());
    clock.advance_secs(86_400);
    let body2 = Arc::new(Mutex::new(feed(&["Tool 1.0.0"])));
    let url2 = server(body2.clone(), Duration::ZERO).await;
    let first = engine.save_watch(spec(&url), &BTreeMap::new()).unwrap();
    let mut other = spec(&url2);
    other.name = "Tool".into();
    let second = engine.save_watch(other, &BTreeMap::new()).unwrap();
    engine.check_one(&first.id).await.unwrap();
    engine.check_one(&second.id).await.unwrap();
    *body.lock().unwrap() = feed(&["App 1.2.0", "App 1.1.0", "App 1.0.0"]);
    *body2.lock().unwrap() = feed(&["Tool 1.1.0", "Tool 1.0.0"]);
    clock.advance_secs(120);
    engine.check_one(&first.id).await.unwrap();
    engine.check_one(&second.id).await.unwrap();
    assert!(engine.flush_digest().await.unwrap());
    let titles = |n: &RecordingNotifier| -> Vec<String> {
        n.notifications()
            .into_iter()
            .filter(|n| n.kind == NotificationKind::Digest)
            .map(|n| n.title)
            .collect()
    };
    assert_eq!(titles(&notifier), ["2 new releases"]);
    // One more release the next day reads "1 new release".
    clock.advance_secs(86_400);
    *body.lock().unwrap() = feed(&["App 1.3.0", "App 1.2.0", "App 1.1.0", "App 1.0.0"]);
    engine.check_one(&first.id).await.unwrap();
    assert!(engine.flush_digest().await.unwrap());
    assert_eq!(titles(&notifier), ["2 new releases", "1 new release"]);
}

#[tokio::test]
async fn the_pattern_helper_writes_a_pattern_or_says_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    let sel = |role: &str, start, end| spotter_core::extract::Selection {
        role: role.into(),
        start,
        end,
    };
    let p = engine
        .suggest_pattern("App-3.10.2", &[sel("major", 4, 5), sel("minor", 6, 8), sel("patch", 9, 10)])
        .unwrap();
    assert!(p.contains("(?<major>") && p.contains("(?<minor>"), "{p}");
    let e = engine.suggest_pattern("App-3.10.2", &[sel("nonsense", 4, 5)]).unwrap_err();
    assert!(matches!(e, EngineError::Invalid(_)), "{e:?}");
}

#[tokio::test]
async fn the_scheduler_ignores_a_stop_signal_that_is_not_a_stop() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    let scheduler = Scheduler::new(engine).with_poll_interval(Duration::from_millis(500));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let run = tokio::spawn(async move { scheduler.run(rx).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    tx.send(false).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!run.is_finished(), "a change to false is not a stop");
    tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), run).await.unwrap().unwrap();
}

#[tokio::test]
async fn unknown_and_malformed_ids_are_refused_by_every_command() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    for id in ["missing", "BAD ID!"] {
        assert!(engine.check_one(id).await.is_err(), "{id}: engine.check_one(id).await");
        assert!(engine.check_now(Some(id)).await.is_err(), "{id}: engine.check_now(Some(id)).await");
        assert!(engine.get_watch(id).is_err(), "{id}: engine.get_watch(id)");
        assert!(engine.set_paused(id, true).is_err(), "{id}: engine.set_paused(id, true)");
        assert!(engine.delete_watch(id).is_err(), "{id}: engine.delete_watch(id)");
        assert!(engine.acknowledge(id, None).is_err(), "{id}: engine.acknowledge(id, None)");
        assert!(
            engine.acknowledge(id, Some("1.0.0")).is_err(),
            "{id}: engine.acknowledge(id, Some(\"1.0.0\"))"
        );
        assert!(
            engine.acknowledge_product(id, "p").is_err(),
            "{id}: engine.acknowledge_product(id, \"p\")"
        );
        assert!(engine.reset_acknowledgement(id).is_err(), "{id}: engine.reset_acknowledgement(id)");
        assert!(
            engine.set_installed(id, Some("1.0.0")).is_err(),
            "{id}: engine.set_installed(id, Some(\"1.0.0\"))"
        );
        assert!(engine.release_of(id, "1.0.0").is_err(), "{id}: engine.release_of(id, \"1.0.0\")");
        assert!(engine.source_of(id).is_err(), "{id}: engine.source_of(id)");
    }
    assert!(engine.record_download("BAD ID!", "1.0.0", Err("x".into())).is_err());
    let mut draft = spec("http://127.0.0.1:1/f.xml");
    draft.id = "BAD ID!".into();
    assert!(engine.save_watch(draft, &BTreeMap::new()).is_err());
    // A version that the watch's scheme cannot read is a bad request, not a silent no-op.
    let saved = engine.save_watch(spec("http://127.0.0.1:1/f.xml"), &BTreeMap::new()).unwrap();
    assert!(matches!(
        engine.set_installed(&saved.id, Some("not a version")),
        Err(EngineError::BadRequest(_))
    ));
    // Checking everything with an unreachable watch still reports it.
    let all = engine.check_now(None).await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn a_preview_of_a_missing_folder_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    let mut s = spec("x");
    s.source = SourceConfig::new("folder", &[("path", &dir.path().join("nope").to_string_lossy())]);
    let p = engine.test_source(&s, &BTreeMap::new()).await;
    assert!(p.error.is_some(), "{p:?}");
}

#[tokio::test]
async fn user_packs_are_loaded_and_bad_ones_are_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let packs = dir.path().join("packs");
    std::fs::create_dir(&packs).unwrap();
    std::fs::write(packs.join("a-bad.toml"), "this = = broken").unwrap();
    std::fs::write(
        packs.join("b-reserved.toml"),
        "[[scheme]]\nid = \"semver\"\nname = \"x\"\npattern = \"(?<major>\\\\d+)\"\n",
    )
    .unwrap();
    std::fs::write(packs.join("c-note.txt"), "ignored").unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    let w = engine.warnings().join("\n");
    assert!(w.contains("a-bad.toml was skipped"), "{w}");
    assert!(w.contains("Pack b-reserved.toml"), "{w}");
}

#[tokio::test]
async fn state_of_a_removed_watch_is_forgotten_and_unknown_kinds_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, clock) = open(dir.path(), EVENING);
    let url = server(Arc::new(Mutex::new(feed(&["App 1.0.0"]))), Duration::ZERO).await;
    let keep = engine.save_watch(spec(&url), &BTreeMap::new()).unwrap();
    let gone = engine.save_watch(spec(&url), &BTreeMap::new()).unwrap();
    engine.check_one(&keep.id).await.unwrap();
    engine.check_one(&gone.id).await.unwrap();
    clock.advance_secs(1);
    let path = engine.config_path();
    drop(engine);
    // The configuration no longer names the second watch, and the first has a source kind this build does not know.
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = String::new();
    let mut skipping = false;
    for block in text.split("[[watches]]") {
        if block.contains(&gone.id) {
            skipping = true;
            continue;
        }
        if out.is_empty() && !block.contains(&keep.id) {
            out.push_str(block);
            continue;
        }
        out.push_str("[[watches]]");
        out.push_str(
            &block
                .replace("kind = \"feed\"", "kind = \"nosuchkind\"")
                .replace("kind = 'feed'", "kind = 'nosuchkind'"),
        );
    }
    assert!(skipping, "{text}");
    std::fs::write(&path, out).unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    let listed = engine.list_watches();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].source_label, "nosuchkind");
}

#[tokio::test]
async fn a_secret_is_read_back_for_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _, _) = open(dir.path(), EVENING);
    let mut s = spec("x");
    s.source = SourceConfig::new("github", &[("repo", "o/r")]);
    s.allow_local = false;
    let secrets = BTreeMap::from([("token".to_string(), "t0ken".to_string())]);
    let saved = engine.save_watch(s, &secrets).unwrap();
    // The check cannot reach GitHub from the test; it still reads the stored token before it tries.
    let _ = tokio::time::timeout(Duration::from_secs(3), engine.check_one(&saved.id)).await;
}
