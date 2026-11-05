//! A tiny local HTTP server for the tests (loopback only; no real hosts).
#![allow(dead_code)]

pub mod net_log;

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

/// The largest request head the server reads.
const MAX_HEAD: usize = 16 << 10;

/// Serves `route(path)` on a random loopback port for the lifetime of the runtime.
///
/// Each connection is answered like a well-behaved server, so the browser never sees a connection problem the test did not
/// ask for: the whole request head is read before answering (a request split over two reads is not answered early), a
/// status without content (204, 205, 304) has no body, and after the answer the server stops writing and reads until the
/// browser closes its side. Closing a socket that still holds unread bytes makes Windows send a reset instead of an
/// orderly close, which can throw away an answer the browser has not read yet.
pub async fn serve(route: impl Fn(&str) -> Reply + Send + Sync + 'static) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let route = route.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = vec![0u8; 4096];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < MAX_HEAD {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => return, // a connection opened ahead of time and never used
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let req = String::from_utf8_lossy(&head).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let (status, ctype, body) = match route(&path) {
                    Reply::Html(h) => (200, "text/html; charset=utf-8", h.into_bytes()),
                    Reply::Bytes(b) => (200, "application/zip", b),
                    Reply::Status(c @ (204 | 205 | 304)) => (c, "text/plain", Vec::new()),
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
                // Read until the browser closes (bounded, so a client that never does cannot keep the task alive).
                let drain = async { while matches!(s.read(&mut buf).await, Ok(n) if n > 0) {} };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(30), drain).await;
            });
        }
    });
    port
}
