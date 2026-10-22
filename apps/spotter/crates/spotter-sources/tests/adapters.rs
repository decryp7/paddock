//! Failure and edge paths of the adapters: odd tokens, ignored ranges, bad checksums and sources, folders.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::checksum::Sha256Digest;
use spotter_core::release::Release;
use spotter_core::source::{HttpRequest, Source, SourceConfig, SourceError};
use spotter_core::version::VersionScheme;
use spotter_sources::checker::for_plan;
use spotter_sources::download::download;
use spotter_sources::http::Http;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const FILE: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `route(path, request) -> raw HTTP response`.
async fn serve(route: impl Fn(&str, &str, &str) -> String + Send + Sync + 'static) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let (route, b2) = (Arc::new(route), base.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let (route, base) = (route.clone(), b2.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let _ = s.write_all(route(&path, &req, &base).as_bytes()).await;
            });
        }
    });
    base
}

fn reply(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} X\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn release(location: &str) -> Release {
    Release {
        version: VersionScheme::Semver.extract("1.0.0").unwrap(),
        name: "app".into(),
        location: location.into(),
        published_at: None,
        size: None,
        sha256: None,
        detail: Default::default(),
    }
}

#[tokio::test]
async fn registry_token_failures_are_explained() {
    let base = serve(|path, _, base| match path {
        "/ok-token" | "/token-bad" => reply(200, "", "{\"nope\":true}"),
        "/token-500" => reply(500, "", ""),
        "/v2/a" => reply(401, &format!("WWW-Authenticate: Bearer realm=\"{base}/token-bad\"\r\n"), ""),
        "/v2/b" => reply(401, &format!("WWW-Authenticate: Bearer realm=\"{base}/token-500\"\r\n"), ""),
        "/v2/c" => reply(401, "WWW-Authenticate: Basic realm=\"x\"\r\n", ""),
        _ => reply(200, "", "ok"),
    })
    .await;
    let http = Http::new("test").unwrap();
    for p in ["/v2/a", "/v2/b", "/v2/c"] {
        let e = http.get(&HttpRequest::get(format!("{base}{p}")), None, 1024).await.unwrap_err();
        assert!(!e.message().is_empty(), "{p}: {e:?}");
    }
    let already = HttpRequest::get(format!("{base}/v2/c")).header("Authorization", "Bearer x");
    assert!(http.get(&already, None, 1024).await.is_err(), "a second challenge is not answered");
    assert!(http.client().get(&base).build().is_ok());
}

#[tokio::test]
async fn download_failures_leave_nothing_looking_finished() {
    let base = serve(|path, req, _| {
        let ranged = req.to_lowercase().contains("range: bytes=");
        match path {
            "/ignores-range" => reply(200, "", std::str::from_utf8(FILE).unwrap()),
            "/not-satisfiable" if ranged => reply(416, "", ""),
            "/not-satisfiable" => reply(200, "", std::str::from_utf8(FILE).unwrap()),
            "/still-bad" if ranged => reply(416, "", ""),
            "/still-bad" => reply(500, "", ""),
            "/gone" => reply(404, "", ""),
            "/app-1.0.0.zip.sha256" => reply(200, "", "not a checksum at all"),
            _ => reply(404, "", ""),
        }
    })
    .await;
    let http = Http::new("test").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let part = |n: &str| dest.path().join(format!("{n}.part"));

    std::fs::write(part("ignores-range"), b"stale").unwrap();
    let d = download(&http, &release(&format!("{base}/ignores-range")), dest.path(), false)
        .await
        .unwrap();
    assert_eq!(std::fs::read(d.path).unwrap(), FILE, "a server that ignores the range starts over");

    std::fs::write(part("not-satisfiable"), b"stale").unwrap();
    let d = download(&http, &release(&format!("{base}/not-satisfiable")), dest.path(), false)
        .await
        .unwrap();
    assert_eq!(d.size, FILE.len() as u64);

    std::fs::write(part("still-bad"), b"stale").unwrap();
    assert!(download(&http, &release(&format!("{base}/still-bad")), dest.path(), false)
        .await
        .is_err());
    let e = download(&http, &release(&format!("{base}/gone")), dest.path(), false)
        .await
        .unwrap_err();
    assert!(e.contains("404"), "{e}");

    let relative = download(&http, &release("relative/path.zip"), dest.path(), false)
        .await
        .unwrap_err();
    assert!(relative.contains("cannot be downloaded"), "{relative}");
    // An unreadable checksum file means "unverified", not failure.
    let d = download(&http, &release(&format!("{base}/ignores-range")), dest.path(), true)
        .await
        .unwrap();
    assert!(!d.verified);
}

#[tokio::test]
async fn copies_check_their_source_and_digests() {
    let http = Http::new("test").unwrap();
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let file = src.path().join("app-1.0.0.zip");
    std::fs::write(&file, FILE).unwrap();

    let d = download(&http, &release(file.to_str().unwrap()), dest.path(), true).await.unwrap();
    assert!(!d.verified && std::fs::read(d.path).unwrap() == FILE);

    std::fs::write(
        src.path().join("app-1.0.0.zip.sha256"),
        format!("{}  app-1.0.0.zip\n", Sha256Digest::of(FILE)),
    )
    .unwrap();
    assert!(
        download(&http, &release(file.to_str().unwrap()), dest.path(), true)
            .await
            .unwrap()
            .verified
    );

    std::fs::write(
        src.path().join("app-1.0.0.zip.sha256"),
        format!("{}  app-1.0.0.zip\n", Sha256Digest::of(b"other")),
    )
    .unwrap();
    let e = download(&http, &release(file.to_str().unwrap()), dest.path(), true)
        .await
        .unwrap_err();
    assert!(e.contains("does not match"), "{e}");
    assert!(!dest.path().join("app-1.0.0.zip.part").exists());

    let mut pinned = release(file.to_str().unwrap());
    pinned.sha256 = Some(Sha256Digest::of(FILE));
    assert!(download(&http, &pinned, dest.path(), false).await.unwrap().verified);

    let e = download(&http, &release(src.path().to_str().unwrap()), dest.path(), false)
        .await
        .unwrap_err();
    assert!(e.contains("folder"), "{e}");
    let missing = src.path().join("nope.zip");
    assert!(download(&http, &release(missing.to_str().unwrap()), dest.path(), false)
        .await
        .is_err());
    // The destination cannot be a file.
    let blocker = dest.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    assert!(download(&http, &release(file.to_str().unwrap()), &blocker.join("sub"), false)
        .await
        .is_err());
}

#[tokio::test]
async fn checkers_are_built_for_every_plan() {
    let http = Http::new("test").unwrap();
    let dir = tempfile::tempdir().unwrap();
    for n in ["app-1.0.0.zip", "app-1.1.0.zip", "readme.txt"] {
        std::fs::write(dir.path().join(n), b"x").unwrap();
    }
    let folder = Source::parse(&SourceConfig::new(
        "folder",
        &[("path", dir.path().to_str().unwrap()), ("pattern", "app-*.zip")],
    ))
    .unwrap();
    let checker = for_plan(folder.plan(&Default::default()), &http, None);
    assert_eq!(checker.list(10).await.unwrap().len(), 2);
    assert_eq!(checker.list(1).await.unwrap().len(), 1);

    let gone = dir.path().join("gone");
    let missing = Source::parse(&SourceConfig::new("folder", &[("path", gone.to_str().unwrap())])).unwrap();
    let e = for_plan(missing.plan(&Default::default()), &http, None).list(10).await.unwrap_err();
    assert!(
        matches!(e, SourceError::NotFound(_) | SourceError::Unreachable(_) | SourceError::Failed(_)),
        "{e:?}"
    );

    let page = Source::parse(&SourceConfig::new(
        "web-page",
        &[
            ("url", "https://example.org/"),
            ("mode", "version"),
            ("rendered", "true"),
            ("selector", "span"),
        ],
    ))
    .unwrap();
    let e = for_plan(page.plan(&Default::default()), &http, None).list(10).await.unwrap_err();
    assert!(e.message().contains("browser"), "{}", e.message());

    let base = serve(|_, _, _| {
        reply(
            200,
            "",
            "<rss><channel><item><title>App 1.0.0</title><link>https://x/1</link></item></channel></rss>",
        )
    })
    .await;
    let feed = Source::parse(&SourceConfig::new("feed", &[("url", &format!("{base}/f.xml"))])).unwrap();
    let c = for_plan(feed.plan(&Default::default()), &http, None).list(10).await.unwrap();
    assert_eq!(c.len(), 1);
    let bad = serve(|_, _, _| reply(200, "", "this is not a feed")).await;
    let feed = Source::parse(&SourceConfig::new("feed", &[("url", &format!("{bad}/f.xml"))])).unwrap();
    assert!(for_plan(feed.plan(&Default::default()), &http, None).list(10).await.is_err());
}

#[tokio::test]
async fn folder_listing_has_a_deadline_and_explains_missing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("f");
    std::fs::write(&file, b"x").unwrap();
    let e = spotter_sources::folder::list(file.to_str().unwrap(), std::time::Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(!e.message().is_empty());
    let none = dir.path().join("none");
    let e = spotter_sources::folder::list(none.to_str().unwrap(), std::time::Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(!e.message().is_empty());
    // A zero deadline either wins or loses the race; both must come back without hanging.
    let _ = spotter_sources::folder::list(dir.path().to_str().unwrap(), std::time::Duration::ZERO).await;
}
