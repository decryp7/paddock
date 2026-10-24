//! A small HTTP/1.1 client for services on `127.0.0.1` only. Pitwall's contract is plain HTTP on loopback, and the apps
//! need two things a general client does not give: Cancel that acts within a fraction of a second even while the server is
//! silent (the socket is read in short slices and closed on cancel, which is how Pitwall and llama-server cancel a
//! request), and separate limits for connecting, the first byte and the gap between stream chunks.
//!
//! Bounded everywhere: header block, line and body sizes. One request per connection (`Connection: close`); bodies are
//! `Content-Length`, chunked or close-delimited. No redirects, no proxies, no TLS, no other host than 127.0.0.1.
//! Extracted from Debrief's `engine/pitwall/http.rs`; Debrief's llama-server client (`debrief-ai-worker/src/http.rs`)
//! moves onto the same module (plan X16 note).

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long one blocking read waits before Cancel and the deadlines are looked at again.
const SLICE: Duration = Duration::from_millis(50);
/// Largest status line plus headers.
const MAX_HEAD: usize = 64 * 1024;
/// Largest chunk-size line.
const MAX_LINE: usize = 1024;
/// Most headers read.
const MAX_HEADERS: usize = 128;

/// A cancel flag shared between the caller and a running request. Cloning shares the flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token that is not cancelled.
    pub fn new() -> CancelToken {
        CancelToken::default()
    }

    /// Wraps a flag an app already has (Debrief's `ChatRequest::cancel`).
    pub fn from_flag(flag: Arc<AtomicBool>) -> CancelToken {
        CancelToken(flag)
    }

    /// Cancels every request holding this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether it was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// The limits of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Connecting to the port.
    pub connect: Duration,
    /// From sending the request to the first byte of the answer (a cold model load happens here).
    pub first_byte: Duration,
    /// Longest silence between two pieces of the body.
    pub idle: Duration,
}

impl Timeouts {
    /// Quick calls (`/x/v1/health`, pairing, `select`, model lists).
    pub const QUICK: Timeouts = Timeouts {
        connect: Duration::from_secs(1),
        first_byte: Duration::from_secs(3),
        idle: Duration::from_secs(3),
    };
    /// A chat answer: the model may have to be loaded first, then tokens stream.
    pub const CHAT: Timeouts = Timeouts {
        connect: Duration::from_secs(2),
        first_byte: Duration::from_secs(120),
        idle: Duration::from_secs(60),
    };
}

/// Why a request failed below HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// Nothing listens on the port (or it refused): the service is not running.
    Refused(String),
    /// A limit ran out: the text names it ("connecting", "the first answer", "the next piece of the answer").
    Timeout(&'static str),
    /// The caller's [`CancelToken`] was cancelled.
    Cancelled,
    /// The connection closed in the middle of an answer.
    Closed,
    /// The server answered something that is not HTTP as expected.
    Protocol(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Refused(e) => write!(f, "nothing answers on the port ({e})"),
            HttpError::Timeout(what) => write!(f, "no answer in time ({what})"),
            HttpError::Cancelled => f.write_str("cancelled"),
            HttpError::Closed => f.write_str("the connection closed in the middle of the answer"),
            HttpError::Protocol(e) => write!(f, "an unexpected answer ({e})"),
        }
    }
}

impl std::error::Error for HttpError {}

/// One request.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// `GET`, `POST`, `DELETE`.
    pub method: &'a str,
    /// The path and query.
    pub path: &'a str,
    /// Extra headers; values never carry line breaks.
    pub headers: Vec<(&'a str, String)>,
    /// A JSON body.
    pub body: Option<Vec<u8>>,
}

impl<'a> Request<'a> {
    /// A `GET`.
    pub fn get(path: &'a str) -> Request<'a> {
        Request {
            method: "GET",
            path,
            headers: vec![],
            body: None,
        }
    }

    /// A `POST` with a JSON body.
    pub fn post(path: &'a str, body: Vec<u8>) -> Request<'a> {
        Request {
            method: "POST",
            path,
            headers: vec![],
            body: Some(body),
        }
    }

    /// A `DELETE`.
    pub fn delete(path: &'a str) -> Request<'a> {
        Request {
            method: "DELETE",
            path,
            headers: vec![],
            body: None,
        }
    }

    /// Adds a header.
    pub fn header(mut self, name: &'a str, value: impl Into<String>) -> Request<'a> {
        self.headers.push((name, value.into()));
        self
    }
}

enum Framing {
    Length(u64),
    Chunked { left: u64, done: bool },
    Close { done: bool },
}

/// An answer whose body is read as it arrives. Dropping it closes the connection.
pub struct Response {
    /// The status code.
    pub status: u16,
    headers: Vec<(String, String)>,
    conn: Conn,
    framing: Framing,
}

impl Response {
    /// A header's value (names compare case-insensitively).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The next decoded piece of the body (empty at the end).
    pub fn next_piece(&mut self) -> Result<Vec<u8>, HttpError> {
        loop {
            match &mut self.framing {
                Framing::Length(0)
                | Framing::Chunked { done: true, .. }
                | Framing::Close { done: true } => return Ok(vec![]),
                Framing::Length(left) => {
                    let piece = self.conn.some(*left)?;
                    if piece.is_empty() {
                        return Err(HttpError::Closed);
                    }
                    *left -= piece.len() as u64;
                    return Ok(piece);
                }
                Framing::Close { done } => {
                    let piece = self.conn.some(u64::MAX)?;
                    if piece.is_empty() {
                        *done = true;
                    }
                    return Ok(piece);
                }
                Framing::Chunked { left, done } => {
                    if *left == 0 {
                        let line = self.conn.line(MAX_LINE)?;
                        let size = line.split(';').next().unwrap_or("").trim();
                        let n = u64::from_str_radix(size, 16)
                            .map_err(|_| HttpError::Protocol("a bad chunk size".into()))?;
                        if n == 0 {
                            // Trailers end with an empty line.
                            while !self.conn.line(MAX_LINE)?.is_empty() {}
                            *done = true;
                            continue;
                        }
                        *left = n;
                    }
                    let piece = self.conn.some(*left)?;
                    if piece.is_empty() {
                        return Err(HttpError::Closed);
                    }
                    *left -= piece.len() as u64;
                    if *left == 0 && !self.conn.line(MAX_LINE)?.is_empty() {
                        return Err(HttpError::Protocol("a chunk without its line end".into()));
                    }
                    return Ok(piece);
                }
            }
        }
    }

    /// The whole body, at most `limit` bytes.
    pub fn read_all(mut self, limit: usize) -> Result<Vec<u8>, HttpError> {
        let mut out = Vec::new();
        loop {
            let piece = self.next_piece()?;
            if piece.is_empty() {
                return Ok(out);
            }
            if out.len() + piece.len() > limit {
                return Err(HttpError::Protocol(format!(
                    "the answer is larger than {limit} bytes"
                )));
            }
            out.extend_from_slice(&piece);
        }
    }
}

struct Conn {
    stream: TcpStream,
    buf: Vec<u8>,
    pos: usize,
    cancel: Option<CancelToken>,
    idle: Duration,
}

impl Drop for Conn {
    fn drop(&mut self) {
        // Closing the connection is how the server hears a cancel.
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

impl Conn {
    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(CancelToken::is_cancelled)
    }

    /// Reads more bytes into the buffer, waiting at most `limit`; false at the end of the stream.
    fn fill(&mut self, limit: Duration, what: &'static str) -> Result<bool, HttpError> {
        let deadline = Instant::now() + limit;
        let mut tmp = [0u8; 16 * 1024];
        loop {
            if self.cancelled() {
                return Err(HttpError::Cancelled);
            }
            match self.stream.read(&mut tmp) {
                Ok(0) => return Ok(false),
                Ok(n) => {
                    if self.pos > 0 {
                        self.buf.drain(..self.pos);
                        self.pos = 0;
                    }
                    self.buf.extend_from_slice(&tmp[..n]);
                    return Ok(true);
                }
                Err(e) => match classify_read_error(&e)? {
                    ReadFailure::Retry => {
                        if Instant::now() >= deadline {
                            return Err(HttpError::Timeout(what));
                        }
                    }
                    ReadFailure::Ended => return Ok(false),
                },
            }
        }
    }

    fn available(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Up to `max` bytes that are already here or arrive next (empty at the end of the stream).
    fn some(&mut self, max: u64) -> Result<Vec<u8>, HttpError> {
        if self.available().is_empty() && !self.fill(self.idle, "the next piece of the answer")? {
            return Ok(vec![]);
        }
        let n = self
            .available()
            .len()
            .min(usize::try_from(max).unwrap_or(usize::MAX));
        let out = self.available()[..n].to_vec();
        self.pos += n;
        Ok(out)
    }

    /// One CRLF-terminated line (without it), at most `max` bytes.
    fn line(&mut self, max: usize) -> Result<String, HttpError> {
        loop {
            if let Some(i) = self.available().windows(2).position(|w| w == b"\r\n") {
                let line = String::from_utf8_lossy(&self.available()[..i]).into_owned();
                self.pos += i + 2;
                return Ok(line);
            }
            if self.available().len() > max {
                return Err(HttpError::Protocol("a line is too long".into()));
            }
            if !self.fill(self.idle, "the next piece of the answer")? {
                return Err(HttpError::Closed);
            }
        }
    }
}

/// What a failed socket read means (when it is not fatal).
#[derive(Debug, PartialEq, Eq)]
enum ReadFailure {
    /// Nothing arrived within the slice (or the call was interrupted): look at the deadline and Cancel, then read again.
    Retry,
    /// The peer reset or closed the connection: the stream ended.
    Ended,
}

fn classify_read_error(e: &std::io::Error) -> Result<ReadFailure, HttpError> {
    match e.kind() {
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted => {
            Ok(ReadFailure::Retry)
        }
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe => {
            Ok(ReadFailure::Ended)
        }
        // Anything else cannot be trusted.
        _ => Err(HttpError::Protocol(e.to_string())),
    }
}

/// A failed connect: a limit that ran out, or nothing listening.
fn connect_error(e: &std::io::Error) -> HttpError {
    match e.kind() {
        ErrorKind::TimedOut | ErrorKind::WouldBlock => HttpError::Timeout("connecting"),
        _ => HttpError::Refused(e.to_string()),
    }
}

/// Sends `req` to `127.0.0.1:port` and reads the status and headers. The body is read from the [`Response`].
pub fn send(
    port: u16,
    req: &Request<'_>,
    t: Timeouts,
    cancel: Option<&CancelToken>,
) -> Result<Response, HttpError> {
    send_as(port, "paddock-pitwall", req, t, cancel)
}

/// [`send`] with the `User-Agent` an app wants Pitwall's request log to show.
pub fn send_as(
    port: u16,
    user_agent: &str,
    req: &Request<'_>,
    t: Timeouts,
    cancel: Option<&CancelToken>,
) -> Result<Response, HttpError> {
    if cancel.is_some_and(CancelToken::is_cancelled) {
        return Err(HttpError::Cancelled);
    }
    if user_agent.contains(['\r', '\n'])
        || req.path.contains(['\r', '\n', ' '])
        || req.method.contains(['\r', '\n', ' '])
    {
        return Err(HttpError::Protocol(
            "a request line with a line break".into(),
        ));
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = TcpStream::connect_timeout(&addr, t.connect).map_err(|e| connect_error(&e))?;
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(SLICE))
        .and_then(|()| stream.set_write_timeout(Some(t.connect.max(Duration::from_secs(5)))))
        .map_err(|e| HttpError::Protocol(e.to_string()))?;
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept-Encoding: identity\r\nUser-Agent: {user_agent}\r\n",
        req.method, req.path
    );
    for (n, v) in &req.headers {
        // Header values never carry line breaks (a key or a model name with one would split the request).
        if v.contains(['\r', '\n']) || n.contains(['\r', '\n', ':']) {
            return Err(HttpError::Protocol("a header with a line break".into()));
        }
        head.push_str(&format!("{n}: {v}\r\n"));
    }
    if let Some(b) = &req.body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    let mut conn = Conn {
        stream,
        buf: Vec::new(),
        pos: 0,
        cancel: cancel.cloned(),
        idle: t.idle,
    };
    let write = |s: &mut TcpStream, bytes: &[u8]| s.write_all(bytes).map_err(|_| HttpError::Closed);
    write(&mut conn.stream, head.as_bytes())?;
    if let Some(b) = &req.body {
        write(&mut conn.stream, b)?;
    }
    let _ = conn.stream.flush();
    // The head: wait for the first byte with the first-byte limit, then read the rest of the head.
    let started = Instant::now();
    loop {
        if conn.available().windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if conn.available().len() > MAX_HEAD {
            return Err(HttpError::Protocol(
                "the answer's headers are too large".into(),
            ));
        }
        let left = if conn.available().is_empty() {
            t.first_byte.saturating_sub(started.elapsed())
        } else {
            t.idle
        };
        if !conn.fill(left.max(SLICE), "the first answer")? {
            return Err(if conn.available().is_empty() {
                HttpError::Closed
            } else {
                HttpError::Protocol("the headers were cut off".into())
            });
        }
    }
    let status_line = conn.line(MAX_HEAD)?;
    let mut parts = status_line.splitn(3, ' ');
    let (version, code) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if !version.starts_with("HTTP/1.") {
        return Err(HttpError::Protocol("not an HTTP answer".into()));
    }
    let status: u16 = code
        .parse()
        .map_err(|_| HttpError::Protocol("a bad status".into()))?;
    let mut headers = Vec::new();
    loop {
        let line = conn.line(MAX_HEAD)?;
        if line.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(HttpError::Protocol("too many headers".into()));
        }
        if let Some((n, v)) = line.split_once(':') {
            headers.push((n.trim().to_string(), v.trim().to_string()));
        }
    }
    let find = |n: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(n))
            .map(|(_, v)| v.as_str())
    };
    let framing =
        if find("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
            Framing::Chunked {
                left: 0,
                done: false,
            }
        } else if let Some(n) = find("content-length") {
            Framing::Length(
                n.parse()
                    .map_err(|_| HttpError::Protocol("a bad Content-Length".into()))?,
            )
        } else if status == 204 || status == 304 || req.method == "HEAD" {
            Framing::Length(0)
        } else {
            Framing::Close { done: false }
        };
    Ok(Response {
        status,
        headers,
        conn,
        framing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// Serves one connection: reads the request, writes the pieces, then (when `hold`) keeps the connection open until the
    /// returned sender is dropped. Returns the port, the request text and the release sender.
    fn serve(pieces: Vec<Vec<u8>>, hold: bool) -> (u16, mpsc::Receiver<String>, mpsc::Sender<()>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (req_tx, req_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = l.accept() else { return };
            s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            // The whole request is read (head and body): closing with unread bytes would reset the connection and the
            // client could lose the answer.
            let mut req: Vec<u8> = Vec::new();
            let mut tmp = [0u8; 8192];
            loop {
                if let Some(end) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&req[..end]).to_ascii_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if req.len() >= end + 4 + len {
                        break;
                    }
                }
                match s.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&tmp[..n]),
                }
            }
            let n = req.len();
            let _ = req_tx.send(String::from_utf8_lossy(&req[..n]).into_owned());
            for p in pieces {
                if s.write_all(&p).is_err() {
                    break;
                }
                let _ = s.flush();
            }
            if hold {
                // Ends when the test drops its sender.
                let _ = release_rx.recv();
            }
        });
        (port, req_rx, release_tx)
    }

    #[test]
    fn content_length_chunked_and_close_delimited_bodies() {
        let (port, req, _r) = serve(
            vec![
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-Pitwall-Model: m\r\n\r\nhello".to_vec(),
            ],
            false,
        );
        let r = send(
            port,
            &Request::get("/x").header("Authorization", "Bearer k"),
            Timeouts::QUICK,
            None,
        )
        .unwrap();
        assert_eq!((r.status, r.header("x-pitwall-model")), (200, Some("m")));
        assert_eq!(r.read_all(100).unwrap(), b"hello");
        let req = req.recv().unwrap();
        assert!(
            req.starts_with("GET /x HTTP/1.1\r\nHost: 127.0.0.1:"),
            "{req}"
        );
        assert!(req.contains("Authorization: Bearer k\r\n") && req.contains("Connection: close"));
        assert!(req.contains("User-Agent: paddock-pitwall\r\n"));

        let (port, _q, _r) = serve(
            vec![
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n".to_vec(),
                b"2;x=y\r\nde\r\n0\r\nTrailer: t\r\n\r\n".to_vec(),
            ],
            false,
        );
        let r = send(
            port,
            &Request::post("/c", b"{}".to_vec()),
            Timeouts::QUICK,
            None,
        )
        .unwrap();
        assert_eq!(r.read_all(100).unwrap(), b"abcde");

        let (port, _q, _r) = serve(vec![b"HTTP/1.1 500 Oops\r\n\r\nbody".to_vec()], false);
        let r = send(port, &Request::delete("/"), Timeouts::QUICK, None).unwrap();
        assert_eq!(r.status, 500);
        assert_eq!(r.read_all(100).unwrap(), b"body");

        let (port, _q, _r) = serve(
            vec![b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort".to_vec()],
            false,
        );
        let r = send(port, &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert_eq!(r.read_all(100).unwrap_err(), HttpError::Closed);

        let (port, _q, _r) = serve(
            vec![
                b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n".to_vec(),
                vec![b'x'; 500],
            ],
            false,
        );
        let r = send(port, &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert!(matches!(r.read_all(100), Err(HttpError::Protocol(_))));

        let (port, _q, _r) = serve(vec![b"HTTP/1.1 204 No Content\r\n\r\n".to_vec()], false);
        let r = send_as(port, "Debrief", &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert_eq!(r.read_all(10).unwrap(), b"");
    }

    #[test]
    fn refused_garbage_and_header_injection() {
        let free = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(matches!(
            send(free, &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Refused(_))
        ));
        let (port, _q, _r) = serve(vec![b"SSH-2.0-nope\r\n\r\n".to_vec()], false);
        assert!(matches!(
            send(port, &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        let (port, _q, _r) = serve(
            vec![b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n".to_vec()],
            false,
        );
        let r = send(port, &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert!(matches!(r.read_all(10), Err(HttpError::Protocol(_))));
        let (port, _q, _r) = serve(vec![], false);
        let bad = Request::get("/").header("X", "a\r\nInjected: 1");
        assert!(matches!(
            send(port, &bad, Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        let (port, _q, _r) = serve(vec![], false);
        let bad_name = Request::get("/").header("X: y", "v");
        assert!(matches!(
            send(port, &bad_name, Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        // A request line, method or agent with a line break or space never leaves the process.
        assert!(matches!(
            send(port, &Request::get("/a b"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        assert!(matches!(
            send(port, &Request::get("/a\r\nX: 1"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        assert!(matches!(
            send_as(port, "a\nb", &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        // A server that closes without answering.
        let (port, _q, _r) = serve(vec![], false);
        assert_eq!(
            send(port, &Request::get("/"), Timeouts::QUICK, None).err(),
            Some(HttpError::Closed)
        );
        // Too many headers.
        let mut many = b"HTTP/1.1 200 OK\r\n".to_vec();
        for i in 0..200 {
            many.extend_from_slice(format!("X-{i}: v\r\n").as_bytes());
        }
        many.extend_from_slice(b"\r\n");
        let (port, _q, _r) = serve(vec![many], false);
        assert!(
            matches!(send(port, &Request::get("/"), Timeouts::QUICK, None), Err(HttpError::Protocol(m)) if m.contains("too many"))
        );
        assert!(HttpError::Timeout("x").to_string().contains("in time"));
        assert!(HttpError::Closed.to_string().contains("closed"));
        assert!(
            HttpError::Refused("r".into())
                .to_string()
                .contains("nothing answers")
        );
        assert_eq!(HttpError::Cancelled.to_string(), "cancelled");
        assert!(
            HttpError::Protocol("p".into())
                .to_string()
                .contains("unexpected")
        );
    }

    #[test]
    fn socket_errors_mean_what_they_should() {
        use std::io::Error;
        for k in [
            ErrorKind::WouldBlock,
            ErrorKind::TimedOut,
            ErrorKind::Interrupted,
        ] {
            assert_eq!(
                classify_read_error(&Error::from(k)),
                Ok(ReadFailure::Retry),
                "{k:?}"
            );
        }
        for k in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
        ] {
            assert_eq!(
                classify_read_error(&Error::from(k)),
                Ok(ReadFailure::Ended),
                "{k:?}"
            );
        }
        assert!(matches!(
            classify_read_error(&Error::from(ErrorKind::InvalidData)),
            Err(HttpError::Protocol(_))
        ));
        assert_eq!(
            connect_error(&Error::from(ErrorKind::TimedOut)),
            HttpError::Timeout("connecting")
        );
        assert_eq!(
            connect_error(&Error::from(ErrorKind::WouldBlock)),
            HttpError::Timeout("connecting")
        );
        assert!(matches!(
            connect_error(&Error::from(ErrorKind::ConnectionRefused)),
            HttpError::Refused(_)
        ));
    }

    #[test]
    fn a_server_that_resets_the_connection_ends_the_answer_not_the_process() {
        // The server takes the request and closes without reading it: the unread bytes make the OS send a reset.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let (got_tx, got_rx) = mpsc::channel();
        let h = std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            // Wait until the request is in the socket (without reading it), then close.
            let mut one = [0u8; 1];
            let _ = s.peek(&mut one);
            let _ = got_tx.send(());
            drop(s);
        });
        let body = vec![b'x'; 64 * 1024];
        let r = send(port, &Request::post("/c", body), Timeouts::QUICK, None);
        got_rx.recv().unwrap();
        h.join().unwrap();
        let err = r.err();
        assert!(
            matches!(err, Some(HttpError::Closed | HttpError::Protocol(_))),
            "{err:?}"
        );
    }

    #[test]
    fn broken_framing_and_oversized_heads_are_protocol_errors_never_panics() {
        // Headers that never end.
        let mut endless = b"HTTP/1.1 200 OK\r\nX: ".to_vec();
        endless.extend(vec![b'a'; MAX_HEAD + 10]);
        let (port, _q, _r) = serve(vec![endless], true);
        assert!(matches!(
            send(port, &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(m)) if m.contains("too large")
        ));
        // Headers cut off by a closing server.
        let (port, _q, _r) = serve(vec![b"HTTP/1.1 200 OK\r\nX: y".to_vec()], false);
        assert!(matches!(
            send(port, &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(m)) if m.contains("cut off")
        ));
        // A chunk that does not end with a line end.
        let (port, _q, _r) = serve(
            vec![b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcde\r\n".to_vec()],
            false,
        );
        let mut r = send(port, &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert!(matches!(r.next_piece(), Err(HttpError::Protocol(m)) if m.contains("line end")));
        // A body that stops in the middle of a chunk.
        let (port, _q, _r) = serve(
            vec![b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab".to_vec()],
            false,
        );
        let mut r = send(port, &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert_eq!(r.next_piece().unwrap(), b"ab");
        assert_eq!(r.next_piece().unwrap_err(), HttpError::Closed);
        // A chunk-size line that never ends.
        let mut long = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        long.extend(vec![b'a'; 3 * MAX_LINE]);
        let (port, _q, _r) = serve(vec![long], true);
        let mut r = send(port, &Request::get("/"), Timeouts::QUICK, None).unwrap();
        assert!(matches!(r.next_piece(), Err(HttpError::Protocol(m)) if m.contains("too long")));
        // A bad Content-Length and a bad status.
        let (port, _q, _r) = serve(
            vec![b"HTTP/1.1 200 OK\r\nContent-Length: many\r\n\r\n".to_vec()],
            false,
        );
        assert!(matches!(
            send(port, &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
        let (port, _q, _r) = serve(vec![b"HTTP/1.1 abc OK\r\n\r\n".to_vec()], false);
        assert!(matches!(
            send(port, &Request::get("/"), Timeouts::QUICK, None),
            Err(HttpError::Protocol(_))
        ));
    }

    #[test]
    fn slow_first_byte_and_stalled_body_time_out() {
        let quick = Timeouts {
            connect: Duration::from_secs(1),
            first_byte: Duration::from_millis(100),
            idle: Duration::from_millis(100),
        };
        // Nothing is ever answered.
        let (silent, _q, _hold) = serve(vec![], true);
        assert_eq!(
            send(silent, &Request::get("/"), quick, None).err(),
            Some(HttpError::Timeout("the first answer"))
        );
        // The head arrives, the body never does.
        let (port, _q, _hold2) = serve(vec![b"HTTP/1.1 200 OK\r\n\r\n".to_vec()], true);
        let mut r = send(port, &Request::get("/"), quick, None).unwrap();
        assert_eq!(
            r.next_piece().unwrap_err(),
            HttpError::Timeout("the next piece of the answer")
        );
    }

    #[test]
    fn cancel_acts_while_the_server_is_silent() {
        let slow = Timeouts {
            connect: Duration::from_secs(1),
            first_byte: Duration::from_secs(300),
            idle: Duration::from_secs(300),
        };
        let (silent, req, _hold) = serve(vec![], true);
        let cancel = CancelToken::new();
        let c = cancel.clone();
        // Cancels once the server has the request, so the client is waiting for the first byte.
        let h = std::thread::spawn(move || {
            let _ = req.recv();
            c.cancel();
        });
        assert_eq!(
            send(silent, &Request::get("/"), slow, Some(&cancel)).err(),
            Some(HttpError::Cancelled)
        );
        h.join().unwrap();
        // Already cancelled: no socket is opened at all.
        let free = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert_eq!(
            send(free, &Request::get("/"), slow, Some(&cancel)).err(),
            Some(HttpError::Cancelled)
        );
        // A cancel in the middle of a body.
        let (port, _q, _hold2) = serve(
            vec![b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec()],
            true,
        );
        let live = CancelToken::new();
        let mut r = send(port, &Request::get("/"), slow, Some(&live)).unwrap();
        let l2 = live.clone();
        let h = std::thread::spawn(move || l2.cancel());
        h.join().unwrap();
        assert_eq!(r.next_piece().unwrap_err(), HttpError::Cancelled);
        assert!(CancelToken::from_flag(Arc::new(AtomicBool::new(true))).is_cancelled());
    }
}
