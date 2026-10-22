//! The HTTP adapter and downloads against a small local server: forge answers, registry tokens, status mapping,
//! bounded bodies, resume, checksum verification and refusal.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::checksum::Sha256Digest;
use spotter_core::release::Release;
use spotter_core::source::{Plan, Source, SourceConfig, SourceError};
use spotter_core::version::VersionScheme;
use spotter_sources::checker::for_plan;
use spotter_sources::download::download;
use spotter_sources::http::Http;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const FILE: &[u8] = b"the release file contents, long enough to resume halfway";

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn reply(status: u16, body: &[u8]) -> Reply {
    Reply {
        status,
        headers: Vec::new(),
        body: body.to_vec(),
    }
}

fn route(path: &str, headers: &str, base: &str) -> Reply {
    let auth = headers
        .lines()
        .find_map(|l| l.strip_prefix("authorization: ").or_else(|| l.strip_prefix("Authorization: ")));
    let range = headers.lines().find_map(|l| {
        l.to_ascii_lowercase()
            .strip_prefix("range: bytes=")
            .map(|r| r.trim_end_matches('-').to_string())
    });
    match path {
        "/repos/o/r/releases?per_page=100" => reply(
            200,
            br#"[{"tag_name":"v1.2.0","name":"One two","html_url":"https://x/1.2.0","assets":[]},{"tag_name":"v1.1.0","html_url":"h","assets":[]}]"#,
        ),
        "/v2/o/app/tags/list?n=1000" => match auth {
            Some("Bearer tok") => reply(200, br#"{"name":"o/app","tags":["1.0.0","1.1.0","latest"]}"#),
            _ => Reply {
                status: 401,
                headers: vec![(
                    "WWW-Authenticate".into(),
                    format!(r#"Bearer realm="{base}/token",service="local",scope="repository:o/app:pull""#),
                )],
                body: Vec::new(),
            },
        },
        p if p.starts_with("/token?") => reply(200, br#"{"token":"tok"}"#),
        "/down" => reply(503, b""),
        "/private" => reply(403, b""),
        "/big" => reply(200, &vec![b'x'; 4096]),
        "/files/app-1.2.0.zip" => match range {
            Some(from) => {
                let from: usize = from.parse().unwrap();
                Reply {
                    status: 206,
                    headers: vec![("Content-Range".into(), format!("bytes {from}-{}/{}", FILE.len() - 1, FILE.len()))],
                    body: FILE[from..].to_vec(),
                }
            }
            None => reply(200, FILE),
        },
        "/files/app-1.2.0.zip.sha256" => reply(200, format!("{}  app-1.2.0.zip\n", Sha256Digest::of(FILE)).as_bytes()),
        "/files/bad-1.2.0.zip" => reply(200, b"tampered"),
        "/files/bad-1.2.0.zip.sha256" => reply(200, format!("{}\n", Sha256Digest::of(FILE)).as_bytes()),
        _ => reply(404, b""),
    }
}

async fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let shared = Arc::new(base.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let base = shared.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let text = String::from_utf8_lossy(&buf).into_owned();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                let r = route(&path, &text, &base);
                let mut head = format!(
                    "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    r.status,
                    r.body.len()
                );
                for (k, v) in r.headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&r.body).await;
            });
        }
    });
    base
}

fn source(kind: &str, values: &[(&str, &str)]) -> Source {
    Source::parse(&SourceConfig::new(kind, values)).unwrap()
}

#[tokio::test]
async fn forge_answers_are_listed() {
    let base = serve().await;
    let http = Http::new("test").unwrap();
    let src = source("github", &[("repo", "o/r"), ("server", &base)]);
    let checker = for_plan(src.plan(&Default::default()), &http, None);
    let c = checker.list(10).await.unwrap();
    assert_eq!(c.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["One two", "v1.1.0"]);
    assert_eq!(checker.list(1).await.unwrap().len(), 1);
}

#[tokio::test]
async fn registries_answer_bearer_challenges() {
    let base = serve().await;
    let http = Http::new("test").unwrap();
    let host = base.trim_start_matches("http://");
    // The container source speaks https; point a request at the local server directly.
    let req = spotter_core::source::HttpRequest::get(format!("{base}/v2/o/app/tags/list?n=1000"));
    let body = http.get(&req, Some(&("me".into(), "secret".into())), 1 << 20).await.unwrap();
    let img = source("container", &[("image", &format!("{host}/o/app"))]);
    let Plan::Http { parser, .. } = img.plan(&Default::default()) else {
        panic!()
    };
    assert_eq!(parser.parse(&body).unwrap().len(), 3);
}

#[tokio::test]
async fn statuses_sizes_and_connection_failures_map_to_errors() {
    let base = serve().await;
    let http = Http::new("test").unwrap();
    let get = |p: &str, max: usize| {
        let http = http.clone();
        let url = format!("{base}{p}");
        async move { http.get(&spotter_core::source::HttpRequest::get(url), None, max).await }
    };
    assert!(matches!(get("/down", 1024).await, Err(SourceError::Unreachable(_))));
    assert!(matches!(get("/private", 1024).await, Err(SourceError::Unauthorized(_))));
    assert!(matches!(get("/missing", 1024).await, Err(SourceError::NotFound(_))));
    assert!(matches!(get("/big", 1024).await, Err(SourceError::InvalidResponse(m)) if m.contains("at most")));
    assert_eq!(get("/big", 8192).await.unwrap().len(), 4096);
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = format!("http://{}/x", closed.local_addr().unwrap());
    drop(closed);
    let e = http
        .get(&spotter_core::source::HttpRequest::get(dead), None, 1024)
        .await
        .unwrap_err();
    assert!(matches!(e, SourceError::Unreachable(_)), "{e:?}");
    let bad_header = spotter_core::source::HttpRequest::get(format!("{base}/x")).header("Authorization", "bad\nvalue");
    assert!(matches!(
        http.get(&bad_header, None, 1024).await,
        Err(SourceError::InvalidConfig(_))
    ));
}

fn release(location: &str) -> Release {
    Release {
        version: VersionScheme::Semver.extract("1.2.0").unwrap(),
        name: "app".into(),
        location: location.into(),
        published_at: None,
        size: None,
        sha256: None,
    }
}

#[tokio::test]
async fn downloads_resume_and_verify_checksums() {
    let base = serve().await;
    let http = Http::new("test").unwrap();
    let dest = tempfile::tempdir().unwrap();
    // A part left by an interrupted run is resumed with a range request.
    std::fs::write(dest.path().join("app-1.2.0.zip.part"), &FILE[..20]).unwrap();
    let d = download(&http, &release(&format!("{base}/files/app-1.2.0.zip")), dest.path(), true)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&d.path).unwrap(), FILE);
    assert!(d.verified, "the .sha256 next to it was checked");
    assert_eq!(d.size, FILE.len() as u64);
    assert!(!dest.path().join("app-1.2.0.zip.part").exists());

    let e = download(&http, &release(&format!("{base}/files/bad-1.2.0.zip")), dest.path(), true)
        .await
        .unwrap_err();
    assert!(e.contains("does not match"));
    assert!(!dest.path().join("bad-1.2.0.zip").exists() && !dest.path().join("bad-1.2.0.zip.part").exists());

    let mut known = release(&format!("{base}/files/app-1.2.0.zip"));
    known.sha256 = Some(Sha256Digest::of(b"something else"));
    assert!(
        download(&http, &known, dest.path(), false)
            .await
            .unwrap_err()
            .contains("does not match"),
        "a known digest is always checked"
    );

    let unverified = download(&http, &release(&format!("{base}/files/bad-1.2.0.zip")), dest.path(), false)
        .await
        .unwrap();
    assert!(!unverified.verified);
    assert!(download(&http, &release(&format!("{base}/missing.zip")), dest.path(), false)
        .await
        .unwrap_err()
        .contains("404"));
    assert!(download(&http, &release("ghcr.io/o/app:1.2.0"), dest.path(), false)
        .await
        .unwrap_err()
        .contains("cannot be downloaded"));
}

#[tokio::test]
async fn files_on_shares_are_copied_with_resume_and_sidecar_checksums() {
    let http = Http::new("test").unwrap();
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let file = src.path().join("R1.02.00T01.zip");
    std::fs::write(&file, FILE).unwrap();
    std::fs::write(src.path().join("R1.02.00T01.zip.sha256"), format!("{}", Sha256Digest::of(FILE))).unwrap();
    std::fs::write(dest.path().join("R1.02.00T01.zip.part"), &FILE[..10]).unwrap();
    let d = download(&http, &release(&file.to_string_lossy()), dest.path(), true).await.unwrap();
    assert!(d.verified);
    assert_eq!(std::fs::read(&d.path).unwrap(), FILE);
    // A part longer than the file cannot belong to it: start over.
    std::fs::write(dest.path().join("R1.02.00T01.zip.part"), vec![0u8; FILE.len() + 5]).unwrap();
    assert!(
        download(&http, &release(&file.to_string_lossy()), dest.path(), true)
            .await
            .unwrap()
            .verified
    );
    let folder = download(&http, &release(&src.path().to_string_lossy()), dest.path(), false)
        .await
        .unwrap_err();
    assert!(folder.contains("only files"), "{folder}");
    let missing = download(&http, &release(&src.path().join("nope").to_string_lossy()), dest.path(), false)
        .await
        .unwrap_err();
    assert!(missing.contains("could not be read"));
}
