//! A tiny local HTTP server for the tests (loopback only; no real hosts).
#![allow(dead_code)]

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What a route answers.
pub enum Reply {
    Html(String),
    Bytes(Vec<u8>),
    Status(u16),
    /// Accepts the request and never answers.
    Hang,
}

/// Serves `route(path)` on a random loopback port for the lifetime of the runtime.
pub async fn serve(route: impl Fn(&str) -> Reply + Send + Sync + 'static) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let route = route.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, ctype, body) = match route(&path) {
                    Reply::Html(h) => (200, "text/html; charset=utf-8", h.into_bytes()),
                    Reply::Bytes(b) => (200, "application/zip", b),
                    Reply::Status(c) => (c, "text/plain", b"nope".to_vec()),
                    Reply::Hang => {
                        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                        return;
                    }
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
                let _ = s.shutdown().await;
            });
        }
    });
    port
}
