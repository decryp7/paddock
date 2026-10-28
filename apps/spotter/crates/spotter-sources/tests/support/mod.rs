//! An in-process HTTP server for the download tests (loopback only; no network). It serves one file, `/f.bin`, and its
//! checksum file, and can play the hosts a downloader meets: with and without byte ranges, throttling with 429 and
//! Retry-After, a connection dropped in the middle of a body, and a slow body (so there is time to cancel).
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, missing_docs)]

use paddock_download::{Config, Downloader, HttpConfig};
use paddock_jobs::{EventSink, JobContext, JobRegistry, JobSnapshot};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How the host behaves.
#[derive(Clone)]
pub struct Host {
    /// Serves `Range` requests (206) or ignores them (always 200 with the whole body).
    pub ranges: bool,
    /// The first this many file requests are answered 429 with `Retry-After: 0`.
    pub throttle_first: usize,
    /// The first body that is sent is cut after this many bytes (the connection closes early).
    pub drop_first_body_after: Option<usize>,
    /// Bytes sent per 10 ms (0: as fast as possible).
    pub slow_chunk: usize,
    /// The text of `/f.bin.sha256` (none: 404).
    pub sidecar: Option<String>,
}

impl Default for Host {
    fn default() -> Self {
        Host {
            ranges: true,
            throttle_first: 0,
            drop_first_body_after: None,
            slow_chunk: 0,
            sidecar: None,
        }
    }
}

/// A running server; stops when dropped.
pub struct Server {
    pub port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start(data: Vec<u8>, host: Host) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (log, halt) = (requests.clone(), stop.clone());
        let file_requests = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || {
            while !halt.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (data, host, log) = (data.clone(), host.clone(), log.clone());
                        let (count, dropped) = (file_requests.clone(), dropped.clone());
                        std::thread::spawn(move || serve(stream, &data, &host, &log, &count, &dropped));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            }
        });
        Server { port, requests, stop }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Every request line with its `Range` header, in arrival order.
    pub fn log(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    /// How many requests asked for a byte range other than the `bytes=0-0` probe.
    pub fn range_requests(&self) -> usize {
        self.log()
            .iter()
            .filter(|r| r.contains("bytes=") && !r.contains("bytes=0-0"))
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(mut s: TcpStream, data: &[u8], host: &Host, log: &Mutex<Vec<String>>, count: &AtomicUsize, dropped: &AtomicBool) {
    let _ = s.set_nonblocking(false);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
    let range = text
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("range:").map(|v| v.trim().to_string()));
    log.lock().unwrap().push(format!("{path} {}", range.clone().unwrap_or_default()));
    if path == "/f.bin.sha256" {
        match &host.sidecar {
            Some(t) => reply(&mut s, "200 OK", &[], t.as_bytes()),
            None => reply(&mut s, "404 Not Found", &[], b"no"),
        }
        return;
    }
    if path != "/f.bin" {
        reply(&mut s, "404 Not Found", &[], b"no");
        return;
    }
    let n = count.fetch_add(1, Ordering::SeqCst);
    if n < host.throttle_first {
        reply(&mut s, "429 Too Many Requests", &[("Retry-After", "0")], b"slow down");
        return;
    }
    let total = data.len();
    let (status, from, to) = match range.as_deref().filter(|_| host.ranges).and_then(parse_range) {
        Some((a, b)) => ("206 Partial Content", a.min(total), b.map_or(total - 1, |b| b.min(total - 1))),
        None => ("200 OK", 0, total - 1),
    };
    let body = &data[from..=to];
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\nETag: \"v1\"\r\n",
        body.len()
    );
    if host.ranges {
        head.push_str("Accept-Ranges: bytes\r\n");
    }
    if status.starts_with("206") {
        head.push_str(&format!("Content-Range: bytes {from}-{to}/{total}\r\n"));
    }
    head.push_str("\r\n");
    if s.write_all(head.as_bytes()).is_err() {
        return;
    }
    let cut = match host.drop_first_body_after {
        Some(cut) if body.len() > cut && !dropped.swap(true, Ordering::SeqCst) => cut,
        _ => body.len(),
    };
    let step = if host.slow_chunk == 0 { 64 * 1024 } else { host.slow_chunk };
    for piece in body[..cut].chunks(step) {
        if s.write_all(piece).is_err() {
            return;
        }
        if host.slow_chunk > 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn parse_range(v: &str) -> Option<(usize, Option<usize>)> {
    let spec = v.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    Some((a.parse().ok()?, if b.is_empty() { None } else { Some(b.parse().ok()?) }))
}

fn reply(s: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

/// A body that is not periodic at the block sizes the downloader uses.
pub fn blob(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + i / 251) % 256) as u8).collect()
}

struct Quiet;
impl EventSink for Quiet {
    fn emit(&self, _: &JobSnapshot) {}
}

/// A shell job to download in (the registry keeps it).
pub fn job() -> (Arc<JobRegistry>, JobContext) {
    let registry = Arc::new(JobRegistry::new(Arc::new(Quiet)));
    let ctx = registry.register("sp.download", "Downloading", None);
    ctx.reporter().start();
    (registry, ctx)
}

/// A downloader with short waits and a small segment threshold, so a megabyte is already segmented.
pub fn downloader() -> Arc<Downloader> {
    let config = Config {
        connections: 4,
        segment_threshold: 256 * 1024,
        min_block: 64 * 1024,
        max_block: 256 * 1024,
        max_buffer: 1024 * 1024,
        attempts: 4,
        backoff: Duration::from_millis(5),
        max_wait: Duration::from_secs(5),
        max_throttles: 3,
        stall_timeout: Duration::from_secs(10),
        range_timeout: Duration::from_secs(10),
        save_every: Duration::from_millis(20),
        disk_reserve: 0,
        ..Config::default()
    };
    Arc::new(Downloader::http(HttpConfig::default()).unwrap().with_config(config))
}

/// Downloads as the app does: the shell's downloader, a shell job nobody watches, nothing cancelling.
pub async fn download(
    http: &spotter_sources::http::Http,
    release: &spotter_core::release::Release,
    folder: &std::path::Path,
    verify: bool,
) -> Result<spotter_sources::download::Downloaded, String> {
    use spotter_sources::download::{download_release, Fetch};
    let registry = Arc::new(paddock_jobs::JobRegistry::new(Arc::new(Quiet)));
    let ctx = registry.register("sp.download", "Downloading", None);
    let config = paddock_download::Config {
        attempts: 2,
        backoff: std::time::Duration::from_millis(5),
        disk_reserve: 0,
        ..paddock_download::Config::default()
    };
    let downloader = Arc::new(
        paddock_download::Downloader::http(paddock_download::HttpConfig::default())
            .unwrap()
            .with_config(config),
    );
    let fetch = Fetch {
        downloader: &downloader,
        http,
        ctx: &ctx,
    };
    download_release(&fetch, release, folder, verify, &spotter_core::job::Job::none()).await
}
