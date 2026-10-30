//! The metadata client takes the proxy decision the downloader uses (rule 7: an in-process server stands in for the proxy).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_download::{ProxyRules, ProxyScheme, ProxyServer};
use spotter_core::source::HttpRequest;
use spotter_sources::http::Http;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{mpsc, Arc};

/// A "proxy" that answers one request and reports its request line.
fn fake_proxy() -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).unwrap_or(0);
            let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or_default().to_string();
            let _ = tx.send(line);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    (port, rx)
}

#[tokio::test]
async fn a_request_goes_through_the_proxy_the_downloader_decided_on() {
    let (port, seen) = fake_proxy();
    let rules = ProxyRules::all(ProxyServer::new(ProxyScheme::Http, "127.0.0.1", port));
    let http = Http::with_proxy("test", Arc::new(rules)).unwrap();
    let req = HttpRequest::get("http://releases.example.invalid/list");
    let body = http.get(&req, None, 1024).await.unwrap();
    assert_eq!(body, b"ok");
    assert_eq!(seen.recv().unwrap(), "GET http://releases.example.invalid/list HTTP/1.1");
}

#[tokio::test]
async fn a_host_the_rules_skip_is_not_sent_to_the_proxy() {
    let (proxy_port, seen) = fake_proxy();
    let (origin_port, origin_seen) = fake_proxy();
    let rules = ProxyRules::all(ProxyServer::new(ProxyScheme::Http, "127.0.0.1", proxy_port)).with_no_proxy("127.0.0.1");
    // The origin is on the bypass list (NO_PROXY), so the request goes straight to it.
    let http = Http::with_proxy("test", Arc::new(rules)).unwrap();
    let req = HttpRequest::get(format!("http://127.0.0.1:{origin_port}/x"));
    http.get(&req, None, 1024).await.unwrap();
    assert!(origin_seen.recv().unwrap().starts_with("GET /x "));
    assert!(seen.try_recv().is_err(), "the proxy was not asked");
}
