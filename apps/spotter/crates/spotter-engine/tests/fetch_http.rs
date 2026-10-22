//! The polite fetcher against a scripted local server: redirects, loops, bearer challenges, validators, caps.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use spotter_core::source::{HttpRequest, SourceError};
use spotter_engine::fetch::{FetchConfig, FetchOptions, Fetched, PoliteFetcher, Validators};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn serve(route: impl Fn(&str, &str) -> String + Send + Sync + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (s2, route, b2) = (seen.clone(), Arc::new(route), base.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let (seen, route, base) = (s2.clone(), route.clone(), b2.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock().unwrap().push(req.clone());
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let _ = s.write_all(route(&path, &base).as_bytes()).await;
            });
        }
    });
    (base, seen)
}

fn reply(status: u16, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} X\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn fetcher(tweak: impl FnOnce(&mut FetchConfig)) -> PoliteFetcher {
    let mut cfg = FetchConfig::new("test");
    cfg.min_host_interval = Duration::ZERO;
    tweak(&mut cfg);
    PoliteFetcher::new(cfg).unwrap()
}

fn local() -> FetchOptions {
    FetchOptions {
        allow_local: true,
        ..FetchOptions::default()
    }
}

fn body_of(f: Fetched) -> (Vec<u8>, String, Validators) {
    match f {
        Fetched::Body {
            bytes,
            final_url,
            validators,
        } => (bytes, final_url, validators),
        Fetched::NotModified => panic!("not modified"),
    }
}

#[tokio::test]
async fn redirects_are_followed_and_capped() {
    let (base, _) = serve(|path, base| match path {
        "/a" => reply(302, &format!("Location: {base}/b\r\n"), ""),
        "/b" => reply(200, "ETag: \"v1\"\r\nLast-Modified: Wed, 21 Oct 2026 07:28:00 GMT\r\n", "hello"),
        "/loop1" => reply(302, "Location: /loop2\r\n", ""),
        "/loop2" => reply(302, "Location: /loop1\r\n", ""),
        "/nowhere" => reply(302, "", ""),
        _ => reply(404, "", ""),
    })
    .await;
    let f = fetcher(|_| {});
    let (bytes, final_url, v) = body_of(f.get(&HttpRequest::get(format!("{base}/a")), &local()).await.unwrap());
    assert_eq!(bytes, b"hello");
    assert!(final_url.ends_with("/b"));
    assert_eq!(v.etag.as_deref(), Some("\"v1\""));
    assert!(v.last_modified.is_some() && !v.is_empty());

    let e = f.get(&HttpRequest::get(format!("{base}/loop1")), &local()).await.unwrap_err();
    assert!(e.message().contains("circle"), "{}", e.message());
    let e = f.get(&HttpRequest::get(format!("{base}/nowhere")), &local()).await.unwrap_err();
    assert!(matches!(e, SourceError::InvalidResponse(_)));
    assert!(f.get(&HttpRequest::get(format!("{base}/gone")), &local()).await.is_err());

    let few = fetcher(|c| c.max_redirects = 0);
    let e = few.get(&HttpRequest::get(format!("{base}/a")), &local()).await.unwrap_err();
    assert!(e.message().contains("too often"), "{}", e.message());
    assert_eq!(f.config().max_redirects, FetchConfig::new("x").max_redirects);
    let _ = f.client();
}

#[tokio::test]
async fn validators_give_not_modified_and_bad_requests_fail_early() {
    let (base, seen) = serve(|_, _| reply(304, "", "")).await;
    let f = fetcher(|_| {});
    let opts = FetchOptions {
        allow_local: true,
        conditional: Some(Validators {
            etag: Some("\"x\"".into()),
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()),
        }),
        credentials: None,
    };
    assert!(matches!(
        f.get(&HttpRequest::get(format!("{base}/")), &opts).await.unwrap(),
        Fetched::NotModified
    ));
    let req = seen.lock().unwrap()[0].to_lowercase();
    assert!(req.contains("if-none-match: \"x\"") && req.contains("if-modified-since"), "{req}");
    // A 304 nobody asked for is an error, not "unchanged".
    assert!(f.get(&HttpRequest::get(format!("{base}/")), &local()).await.is_err());

    assert!(matches!(
        f.get(&HttpRequest::get("not a url"), &local()).await,
        Err(SourceError::InvalidConfig(_))
    ));
    let bad_name = HttpRequest::get(format!("{base}/")).header("bad header", "x");
    assert!(matches!(f.get(&bad_name, &local()).await, Err(SourceError::InvalidConfig(_))));
    let bad_value = HttpRequest::get(format!("{base}/")).header("X-Token", "line\nbreak");
    assert!(matches!(f.get(&bad_value, &local()).await, Err(SourceError::InvalidConfig(_))));
    let e = f
        .get(&HttpRequest::get(format!("{base}/")), &FetchOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(e, SourceError::InvalidConfig(_)), "loopback is refused by default");
}

#[tokio::test]
async fn bodies_are_capped() {
    let (base, _) = serve(|_, _| reply(200, "", &"x".repeat(5000))).await;
    let f = fetcher(|c| c.max_body = 1000);
    assert!(f.get(&HttpRequest::get(format!("{base}/")), &local()).await.is_err());
}

#[tokio::test]
async fn registries_hand_out_tokens_once() {
    let (base, seen) = serve(|path, base| {
        if path.starts_with("/token") {
            reply(200, "", "{\"token\":\"tok-1\"}")
        } else if path == "/badtoken" {
            reply(200, "", "{\"nothing\":1}")
        } else if path == "/v2/bad" {
            reply(
                401,
                &format!("WWW-Authenticate: Bearer realm=\"{base}/badtoken\",service=\"r\"\r\n"),
                "",
            )
        } else if path == "/v2/denied" {
            reply(401, &format!("WWW-Authenticate: Bearer realm=\"{base}/token\"\r\n"), "")
        } else if path == "/v2/plain" {
            reply(401, "WWW-Authenticate: Basic realm=\"x\"\r\n", "")
        } else {
            reply(
                401,
                &format!("WWW-Authenticate: Bearer realm=\"{base}/token\",service=\"reg\",scope=\"repo:a:pull\"\r\n"),
                "",
            )
        }
    })
    .await;
    let f = fetcher(|_| {});
    let opts = FetchOptions {
        allow_local: true,
        conditional: None,
        credentials: Some(("user".into(), "pw".into())),
    };
    // The server keeps answering 401 (it only checks nothing): the second 401 is the final answer.
    let e = f.get(&HttpRequest::get(format!("{base}/v2/x")), &opts).await.unwrap_err();
    assert!(!e.message().is_empty(), "{e:?}");
    let log = seen.lock().unwrap().join("\n").to_lowercase();
    assert!(
        log.contains("get /token?service=reg") && log.contains("authorization: basic"),
        "{log}"
    );
    assert!(log.contains("authorization: bearer tok-1"), "{log}");
    assert!(f.get(&HttpRequest::get(format!("{base}/v2/bad")), &opts).await.is_err());
    assert!(f.get(&HttpRequest::get(format!("{base}/v2/plain")), &opts).await.is_err());
    assert!(f
        .get(
            &HttpRequest::get(format!("{base}/v2/denied")),
            &FetchOptions {
                credentials: None,
                ..local()
            }
        )
        .await
        .is_err());
    // The token endpoint is guarded as well.
    assert!(f
        .get(&HttpRequest::get(format!("{base}/v2/denied")), &FetchOptions::default())
        .await
        .is_err());
}

#[tokio::test]
async fn one_host_is_asked_politely() {
    let (base, _) = serve(|_, _| reply(200, "", "ok")).await;
    let f = fetcher(|c| c.min_host_interval = Duration::from_millis(150));
    let started = std::time::Instant::now();
    for _ in 0..3 {
        f.get(&HttpRequest::get(format!("{base}/")), &local()).await.unwrap();
    }
    assert!(started.elapsed() >= Duration::from_millis(280), "{:?}", started.elapsed());
}

#[tokio::test]
async fn a_dead_server_is_unreachable() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let f = fetcher(|c| c.request_timeout = Duration::from_secs(2));
    let e = f
        .get(&HttpRequest::get(format!("http://127.0.0.1:{port}/")), &local())
        .await
        .unwrap_err();
    assert!(matches!(e, SourceError::Unreachable(_)), "{e:?}");
}
