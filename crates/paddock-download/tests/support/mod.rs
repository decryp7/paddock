//! An in-process HTTP/1.1 server on a loopback port (std only; no network access), the job kit, and helpers shared by the tests
//! and the benchmark.
#![allow(dead_code)]

use paddock_jobs::{
    CancelToken, EventSink, FakeClock, JobContext, JobRegistry, JobSnapshot, JobState,
};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

impl Req {
    /// `Range: bytes=a-b` as `(a, Some(b))`, `bytes=a-` as `(a, None)`.
    pub fn range(&self) -> Option<(usize, Option<usize>)> {
        let r = self.headers.get("range")?.strip_prefix("bytes=")?;
        let (a, b) = r.split_once('-')?;
        Some((
            a.parse().ok()?,
            if b.is_empty() {
                None
            } else {
                Some(b.parse().ok()?)
            },
        ))
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Send no `Content-Length` (the body ends when the connection closes).
    pub no_length: bool,
    /// Send only this many body bytes, then close (a dropped connection).
    pub cut_after: Option<usize>,
    /// Send this many body bytes, then hold the connection open without sending anything (until the server is dropped).
    pub stall_after: Option<usize>,
    /// Wait this long before the status line.
    pub delay: Duration,
    /// Send the body at about this many bytes per second.
    pub rate_bps: Option<u64>,
}

impl Resp {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Resp {
        Resp {
            status,
            headers: vec![],
            body: body.into(),
            no_length: false,
            cut_after: None,
            stall_after: None,
            delay: Duration::ZERO,
            rate_bps: None,
        }
    }
    pub fn header(mut self, k: &str, v: &str) -> Resp {
        self.headers.push((k.into(), v.into()));
        self
    }
    pub fn cut(mut self, n: usize) -> Resp {
        self.cut_after = Some(n);
        self
    }
    pub fn stall(mut self, n: usize) -> Resp {
        self.stall_after = Some(n);
        self
    }
    pub fn delay(mut self, d: Duration) -> Resp {
        self.delay = d;
        self
    }
    pub fn rate(mut self, bps: u64) -> Resp {
        self.rate_bps = Some(bps);
        self
    }
    pub fn no_length(mut self) -> Resp {
        self.no_length = true;
        self
    }
}

type Handler = Arc<dyn Fn(&Req, usize) -> Resp + Send + Sync>;

pub struct Server {
    pub port: u16,
    log: Arc<Mutex<Vec<Req>>>,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

impl Server {
    /// `handler(request, n)` answers the `n`-th request (0-based).
    pub fn start(handler: impl Fn(&Req, usize) -> Resp + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let handler: Handler = Arc::new(handler);
        let (l, st, a, p) = (log.clone(), stop.clone(), active.clone(), peak.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let (h, l, st, a, p) =
                    (handler.clone(), l.clone(), st.clone(), a.clone(), p.clone());
                std::thread::spawn(move || serve(conn, &h, &l, &st, &a, &p));
            }
        });
        Server {
            port,
            log,
            stop,
            active,
            peak,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn log(&self) -> Vec<Req> {
        self.log.lock().unwrap().clone()
    }

    /// The most connections that were being answered at once.
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Counts a request as active from its arrival until just before its last byte is written, so the count never includes a
/// connection the client may already have finished with.
struct Active<'a>(&'a AtomicUsize, bool);

impl Active<'_> {
    fn end(&mut self) {
        if !self.1 {
            self.1 = true;
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.end();
    }
}

fn serve(
    mut s: TcpStream,
    h: &Handler,
    log: &Mutex<Vec<Req>>,
    stop: &AtomicBool,
    active: &AtomicUsize,
    peak: &AtomicUsize,
) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        match s.read(&mut tmp) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let (method, path) = (
        first.next().unwrap_or("").to_string(),
        first.next().unwrap_or("").to_string(),
    );
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
    let req = Req {
        method,
        path,
        headers,
    };
    peak.fetch_max(active.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
    let mut counted = Active(active, false);
    let n = {
        let mut l = log.lock().unwrap();
        l.push(req.clone());
        l.len() - 1
    };
    let resp = h(&req, n);
    if !nap(resp.delay, stop) {
        return;
    }
    let mut out = format!("HTTP/1.1 {} X\r\nconnection: close\r\n", resp.status);
    if !resp.no_length {
        out.push_str(&format!("content-length: {}\r\n", resp.body.len()));
    }
    for (k, v) in &resp.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let limit = resp
        .cut_after
        .or(resp.stall_after)
        .unwrap_or(resp.body.len())
        .min(resp.body.len());
    if limit == 0 {
        counted.end();
    }
    if s.write_all(out.as_bytes()).is_err() {
        return;
    }
    let (body, last) = resp.body[..limit].split_at(limit.saturating_sub(1));
    match resp.rate_bps {
        Some(rate) => {
            for part in body.chunks(16 * 1024) {
                if s.write_all(part).is_err() || stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_secs_f64(part.len() as f64 / rate as f64));
            }
        }
        None => {
            let _ = s.write_all(body);
        }
    }
    counted.end();
    let _ = s.write_all(last);
    let _ = s.flush();
    if resp.stall_after.is_some() {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let _ = s.shutdown(Shutdown::Both);
}

fn nap(d: Duration, stop: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

/// A 206 for `data` honouring the request's Range (200 with the whole file without one), with an ETag.
pub fn partial(req: &Req, data: &[u8]) -> Resp {
    partial_with(req, data, "\"v1\"")
}

pub fn partial_with(req: &Req, data: &[u8], etag: &str) -> Resp {
    if let Some(v) = req.headers.get("if-range")
        && v != etag
    {
        return Resp::new(200, data.to_vec()).header("etag", etag);
    }
    match req.range() {
        Some((a, b)) if a < data.len() => {
            let b = b.unwrap_or(data.len() - 1).min(data.len() - 1);
            Resp::new(206, data[a..=b].to_vec())
                .header("content-range", &format!("bytes {a}-{b}/{}", data.len()))
                .header("etag", etag)
        }
        Some(_) if data.is_empty() => {
            Resp::new(416, Vec::new()).header("content-range", "bytes */0")
        }
        _ => Resp::new(200, data.to_vec()).header("etag", etag),
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn blob(len: usize) -> Vec<u8> {
    let mut x: u32 = 12345;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect()
}

/// Records every snapshot (no throttling) and cancels the job when a chosen phase begins or its count passes a value.
#[derive(Default)]
pub struct Recorder {
    pub snaps: Mutex<Vec<JobSnapshot>>,
    trigger: Mutex<Option<(String, u64)>>,
    token: OnceLock<CancelToken>,
    /// When set, the trigger cancels this token instead of the job's.
    external: Mutex<Option<CancelToken>>,
    pub fired: Mutex<Option<Instant>>,
}

impl Recorder {
    /// Cancels as soon as phase `id` has counted at least `done` (0: the moment it begins).
    pub fn cancel_at(&self, id: &str, done: u64) {
        *self.trigger.lock().unwrap() = Some((id.to_string(), done));
    }

    /// As [`cancel_at`](Self::cancel_at), but cancels `token` (given to `download_with_cancel`) and not the job.
    pub fn cancel_external_at(&self, token: &CancelToken, id: &str, done: u64) {
        *self.external.lock().unwrap() = Some(token.clone());
        self.cancel_at(id, done);
    }

    pub fn phases(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in self.snaps.lock().unwrap().iter() {
            if let JobState::Running { phase: Some(p) } = &s.state
                && out.last() != Some(&p.phase.id)
            {
                out.push(p.phase.id.clone());
            }
        }
        out
    }

    /// Every (phase, done, total) seen, in order.
    pub fn progress(&self) -> Vec<(String, u64, Option<u64>)> {
        self.snaps
            .lock()
            .unwrap()
            .iter()
            .filter_map(|s| match &s.state {
                JobState::Running { phase: Some(p) } => {
                    Some((p.phase.id.clone(), p.done, p.phase.total))
                }
                _ => None,
            })
            .collect()
    }

    /// Asserts that no phase's count goes backwards and none passes its total.
    pub fn assert_monotonic(&self) {
        let mut last: Option<(String, u64)> = None;
        for (id, done, total) in self.progress() {
            if let Some((lid, ld)) = &last
                && *lid == id
            {
                assert!(done >= *ld, "phase {id} went back from {ld} to {done}");
            }
            if let Some(t) = total {
                assert!(done <= t, "phase {id} counted {done} of {t}");
            }
            last = Some((id, done));
        }
    }
}

impl EventSink for Recorder {
    fn emit(&self, s: &JobSnapshot) {
        self.snaps.lock().unwrap().push(s.clone());
        if let JobState::Running { phase: Some(p) } = &s.state {
            let hit = matches!(&*self.trigger.lock().unwrap(), Some((id, done)) if *id == p.phase.id && p.done >= *done);
            let target = self
                .external
                .lock()
                .unwrap()
                .clone()
                .or_else(|| self.token.get().cloned());
            if hit && let Some(t) = target {
                t.cancel();
                self.fired.lock().unwrap().get_or_insert_with(Instant::now);
            }
        }
    }
}

/// A job whose every update is recorded (the fake clock never advances, so nothing is throttled away: min interval 0).
pub fn job() -> (JobContext, Arc<Recorder>) {
    let rec = Arc::new(Recorder::default());
    let jobs = JobRegistry::with_clock(rec.clone(), Arc::new(FakeClock::new()), 0);
    let ctx = jobs.register("download", "Downloading", Some("file"));
    let _ = rec.token.set(ctx.cancel_token().clone());
    (ctx, rec)
}

/// What a [`ConnectProxy`] saw of one tunnel request.
#[derive(Debug, Clone)]
pub struct Tunnel {
    /// `host:port` after `CONNECT`.
    pub target: String,
    /// The `Proxy-Authorization` header value, if any.
    pub authorization: Option<String>,
    /// True when the tunnel was refused with 407.
    pub refused: bool,
}

/// An in-process HTTP CONNECT proxy on a loopback port: it checks `Proxy-Authorization: Basic <base64(user:pass)>` when given
/// credentials, then pipes bytes both ways to the target. No network beyond loopback.
pub struct ConnectProxy {
    pub port: u16,
    log: Arc<Mutex<Vec<Tunnel>>>,
    stop: Arc<AtomicBool>,
}

impl ConnectProxy {
    pub fn start(credentials: Option<(&str, &str)>) -> ConnectProxy {
        let expected =
            credentials.map(|(u, p)| format!("Basic {}", base64(format!("{u}:{p}").as_bytes())));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (l, st) = (log.clone(), stop.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if st.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let (l, expected) = (l.clone(), expected.clone());
                std::thread::spawn(move || tunnel(conn, &l, expected.as_deref()));
            }
        });
        ConnectProxy { port, log, stop }
    }

    /// The proxy's address as an `http://` URL.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn log(&self) -> Vec<Tunnel> {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for ConnectProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn tunnel(mut client: TcpStream, log: &Mutex<Vec<Tunnel>>, expected: Option<&str>) {
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match client.read(&mut b) {
            Ok(1) => head.push(b[0]),
            _ => return,
        }
        if head.len() > 16 * 1024 {
            return;
        }
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split(' ');
    if parts.next() != Some("CONNECT") {
        let _ = client.write_all(b"HTTP/1.1 405 Method Not Allowed\r\ncontent-length: 0\r\n\r\n");
        return;
    }
    let target = parts.next().unwrap_or_default().to_string();
    let authorization = lines.find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case("proxy-authorization")
            .then(|| v.trim().to_string())
    });
    let refused = expected.is_some_and(|e| authorization.as_deref() != Some(e));
    log.lock().unwrap().push(Tunnel {
        target: target.clone(),
        authorization,
        refused,
    });
    if refused {
        let _ = client.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nproxy-authenticate: Basic realm=\"t\"\r\ncontent-length: 0\r\n\r\n");
        return;
    }
    let Ok(upstream) = TcpStream::connect(&target) else {
        let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n");
        return;
    };
    if client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .is_err()
    {
        return;
    }
    let (mut c2, mut u2) = (client.try_clone().unwrap(), upstream.try_clone().unwrap());
    let (mut c1, mut u1) = (client, upstream);
    let up = std::thread::spawn(move || {
        let _ = std::io::copy(&mut c1, &mut u1);
        let _ = u1.shutdown(Shutdown::Both);
    });
    let _ = std::io::copy(&mut u2, &mut c2);
    let _ = c2.shutdown(Shutdown::Both);
    let _ = up.join();
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, &b)| a | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
