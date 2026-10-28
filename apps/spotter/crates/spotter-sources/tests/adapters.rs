//! Failure and edge paths of the adapters: odd tokens, ignored ranges, bad checksums and sources, folders.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::checksum::Sha256Digest;
use spotter_core::release::Release;
use spotter_core::source::{HttpRequest, Source, SourceConfig, SourceError};
use spotter_core::version::VersionScheme;
use spotter_sources::checker::for_plan;
mod support;
use spotter_sources::http::Http;
use std::sync::Arc;
use support::download;
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

fn raw(status_line: &str, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status_line}\r\n{headers}Connection: close\r\n\r\n{body}")
}

#[tokio::test]
async fn oversized_answers_are_refused_before_and_while_reading() {
    let base = serve(|path, _, _| match path {
        // Claims far more than the download limit: refused before a byte is written.
        "/huge" => raw("200 OK", "Content-Length: 99999999999999\r\n", ""),
        // No length at all: the bound is enforced while reading.
        "/stream" => raw(
            "200 OK",
            "Transfer-Encoding: chunked\r\n",
            "6\r\nabcdef\r\n6\r\nghijkl\r\n0\r\n\r\n",
        ),
        _ => reply(404, "", ""),
    })
    .await;
    let http = Http::new("test").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let e = download(&http, &release(&format!("{base}/huge")), dest.path(), false)
        .await
        .unwrap_err();
    assert!(e.contains("larger than"), "{e}");
    let e = http.get(&HttpRequest::get(format!("{base}/stream")), None, 8).await.unwrap_err();
    assert!(matches!(e, SourceError::InvalidResponse(_)), "{e:?}");
    let ok = http.get(&HttpRequest::get(format!("{base}/stream")), None, 64).await.unwrap();
    assert_eq!(ok, b"abcdefghijkl");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_failures_say_what_to_expect() {
    let base = serve(|path, _, base| match path {
        "/hang" => {
            std::thread::sleep(std::time::Duration::from_millis(600));
            reply(200, "", "late")
        }
        "/loop" => raw("302 Found", &format!("Location: {base}/loop\r\nContent-Length: 0\r\n"), ""),
        _ => "this is not http\r\n\r\n".to_string(),
    })
    .await;
    let url = |p: &str| format!("{base}{p}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(100))
        .redirect(reqwest::redirect::Policy::limited(2))
        .build()
        .unwrap();
    let e = client.get(url("/hang")).send().await.unwrap_err();
    assert!(matches!(spotter_sources::http::transport(e, "x"), SourceError::Unreachable(m) if m.contains("in time")));
    let e = client.get(url("/loop")).send().await.unwrap_err();
    assert!(matches!(spotter_sources::http::transport(e, "x"), SourceError::Failed(m) if m.contains("redirects too often")));
    let e = client.get(url("/garbage")).send().await.unwrap_err();
    assert!(matches!(spotter_sources::http::transport(e, "x"), SourceError::Unreachable(m) if m.contains("could not be read")));
}

#[tokio::test]
async fn a_huge_local_checksum_file_means_unverified_and_a_huge_source_is_refused() {
    let http = Http::new("test").unwrap();
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let file = src.path().join("app-1.0.0.zip");
    std::fs::write(&file, FILE).unwrap();
    let sum = std::fs::File::create(src.path().join("app-1.0.0.zip.sha256")).unwrap();
    sum.set_len(spotter_core::limits::MAX_CHECKSUM_FILE as u64 + 1).unwrap();
    let d = download(&http, &release(file.to_str().unwrap()), dest.path(), true).await.unwrap();
    assert!(!d.verified, "an oversized checksum file is not trusted or read");

    #[cfg(unix)]
    {
        // A sparse file: it claims more than the limit without using the disk.
        let big = src.path().join("huge-1.0.0.bin");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(spotter_sources::download::MAX_DOWNLOAD + 1)
            .unwrap();
        let e = download(&http, &release(big.to_str().unwrap()), dest.path(), false)
            .await
            .unwrap_err();
        assert!(e.contains("larger than"), "{e}");
    }
}

#[tokio::test]
async fn registry_and_request_failures_are_messages() {
    let big = "x".repeat(70_000);
    let base = serve(move |path, _, base| match path {
        "/v2/bad-token" => raw(
            "401 Unauthorized",
            &format!("WWW-Authenticate: Bearer realm=\"{base}/token-newline\",service=\"s\"\r\nContent-Length: 0\r\n"),
            "",
        ),
        p if p.starts_with("/token-newline") => reply(200, "", "{\"token\":\"a\\nb\"}"),
        "/v2/dead-realm" => raw(
            "401 Unauthorized",
            "WWW-Authenticate: Bearer realm=\"http://127.0.0.1:1/token\",service=\"s\"\r\nContent-Length: 0\r\n",
            "",
        ),
        "/v2/huge-token" => raw(
            "401 Unauthorized",
            &format!("WWW-Authenticate: Bearer realm=\"{base}/token-huge\",service=\"s\"\r\nContent-Length: 0\r\n"),
            "",
        ),
        p if p.starts_with("/token-huge") => reply(200, "", &big),
        _ => reply(404, "", ""),
    })
    .await;
    let http = Http::new("test").unwrap();
    for path in ["/v2/bad-token", "/v2/dead-realm", "/v2/huge-token"] {
        let e = http
            .get(&HttpRequest::get(format!("{base}{path}")), None, 1 << 20)
            .await
            .unwrap_err();
        assert!(
            matches!(e, SourceError::InvalidResponse(_) | SourceError::Unreachable(_)),
            "{path}: {e:?}"
        );
    }
    let bad_header = HttpRequest::get(format!("{base}/x")).header("bad name", "v");
    assert!(matches!(
        http.get(&bad_header, None, 1024).await,
        Err(SourceError::InvalidConfig(_))
    ));
}

#[tokio::test]
async fn downloads_survive_a_dead_server_a_cut_connection_and_unwritable_places() {
    let base = serve(|path, _, _| match path {
        // Promises 100 bytes, sends 10 and hangs up.
        "/cut.bin" => "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n0123456789".to_string(),
        "/ok.bin" => reply(200, "", "payload"),
        "/sums/huge.bin.sha256" => reply(200, "", &"a".repeat(70_000)),
        "/sums/huge.bin" => reply(200, "", "payload"),
        _ => reply(404, "", ""),
    })
    .await;
    let http = Http::new("test").unwrap();
    let dest = tempfile::tempdir().unwrap();
    let get = |loc: String, verify: bool| {
        let (http, dest) = (http.clone(), dest.path().to_path_buf());
        async move { download(&http, &release(&loc), &dest, verify).await }
    };

    // Nothing listens: the checksum lookup gives up quietly, the download itself says what happened.
    let e = get("http://127.0.0.1:1/app.bin".into(), true).await.unwrap_err();
    assert!(e.contains("could not be reached") || e.contains("127.0.0.1"), "{e}");

    // The connection is cut: the part stays for the next try and the message says so.
    let e = get(format!("{base}/cut.bin"), false).await.unwrap_err();
    assert!(
        !e.is_empty() && !dest.path().join("cut.bin").exists(),
        "an incomplete file never looks finished: {e}"
    );

    // A publisher's checksum file that is too big is ignored, the file is still delivered (unverified).
    let d = get(format!("{base}/sums/huge.bin"), true).await.unwrap();
    assert!(!d.verified);

    // A folder where the finished file belongs cannot be replaced.
    std::fs::create_dir_all(dest.path().join("ok.bin").join("inner")).unwrap();
    let e = get(format!("{base}/ok.bin"), false).await.unwrap_err();
    assert!(
        e.contains("could not be"),
        "a folder where the finished file belongs is refused: {e}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn parts_that_cannot_be_read_or_written_are_reported() {
    use std::os::unix::fs::PermissionsExt;
    let base = serve(|_, _, _| reply(200, "", "payload")).await;
    let http = Http::new("test").unwrap();
    let dest = tempfile::tempdir().unwrap();

    // A folder where the partial file belongs: the download refuses and delivers nothing.
    std::fs::create_dir(dest.path().join("dir.bin.part")).unwrap();
    let e = download(&http, &release(&format!("{base}/dir.bin")), dest.path(), false)
        .await
        .unwrap_err();
    assert!(!e.is_empty() && !dest.path().join("dir.bin").exists(), "{e}");

    // A read-only partial file: starting over cannot write it.
    let part = dest.path().join("ro.bin.part");
    std::fs::write(&part, b"").unwrap();
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o444)).unwrap();
    if std::fs::OpenOptions::new().write(true).open(&part).is_err() {
        let e = download(&http, &release(&format!("{base}/ro.bin")), dest.path(), false)
            .await
            .unwrap_err();
        assert!(!e.is_empty() && !dest.path().join("ro.bin").exists(), "{e}");
    }

    // The same for a copy: unreadable source, folder as the part, read-only part.
    let src = tempfile::tempdir().unwrap();
    let file = src.path().join("c.bin");
    std::fs::write(&file, b"data").unwrap();
    let missing = src.path().join("gone.bin");
    let e = download(&http, &release(missing.to_str().unwrap()), dest.path(), false)
        .await
        .unwrap_err();
    assert!(e.contains("could not be read"), "{e}");
    std::fs::create_dir(dest.path().join("c.bin.part")).unwrap();
    let e = download(&http, &release(file.to_str().unwrap()), dest.path(), false)
        .await
        .unwrap_err();
    assert!(e.contains("could not be read"), "{e}");
    std::fs::remove_dir(dest.path().join("c.bin.part")).unwrap();
    let part = dest.path().join("c.bin.part");
    std::fs::write(&part, b"").unwrap();
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o444)).unwrap();
    if std::fs::OpenOptions::new().write(true).open(&part).is_err() {
        let e = download(&http, &release(file.to_str().unwrap()), dest.path(), false)
            .await
            .unwrap_err();
        assert!(e.contains("could not be written"), "{e}");
    }
    // A checksum "file" that is a folder is not a checksum.
    let s2 = tempfile::tempdir().unwrap();
    let f2 = s2.path().join("d.bin");
    std::fs::write(&f2, b"data").unwrap();
    std::fs::create_dir(s2.path().join("d.bin.sha256")).unwrap();
    let d = download(&http, &release(f2.to_str().unwrap()), dest.path(), true).await.unwrap();
    assert!(!d.verified);
}

#[tokio::test]
async fn an_answer_cut_off_mid_body_is_a_transport_message() {
    // Announces 100 bytes, sends 5 and closes.
    let base = serve(|_, _, _| "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nhello".to_string()).await;
    let http = Http::new("test").unwrap();
    let e = http.get(&HttpRequest::get(format!("{base}/cut")), None, 1024).await.unwrap_err();
    assert!(
        matches!(
            e,
            SourceError::Unreachable(_) | SourceError::Failed(_) | SourceError::InvalidResponse(_)
        ),
        "{e:?}"
    );
}
