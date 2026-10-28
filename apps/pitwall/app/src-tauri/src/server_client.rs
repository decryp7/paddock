//! The app's only way to the Pitwall server: the management API on 127.0.0.1 (`POST /mgmt/v1/rpc/<method>`,
//! `GET /mgmt/v1/live`, `POST /mgmt/v1/chat`). Commands depend on the [`ServerClient`] trait and never on HTTP
//! details (AGENTS.md: dependency inversion); [`LoopbackClient`] is the real implementation over `pitwall-loopback`
//! (the only HTTP client allowed in the shell), [`FakeServer`] is the test double.
//!
//! Failure contract (the same for every implementation): a server that is not running is
//! [`ErrorCode::ServerUnavailable`] with the next step in the message; a request that does not finish in time is also
//! `ServerUnavailable` and says to retry; a wrong or missing admin key is `Unauthorized` after one automatic retry
//! with a freshly read key; anything else is the server's own [`CommandError`].

use crate::credentials::AdminKeys;
use pitwall_core::config::DEFAULT_PORT;
use pitwall_core::mgmt::{CommandError, ErrorCode};
use pitwall_loopback::{Client, LoopbackError, LoopbackPort};
use serde_json::Value;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// A boxed future, so the traits are usable as `dyn`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What to do when the server is not running (shown with every `ServerUnavailable`).
pub const START_HINT: &str = "Use Repair service (Settings, Background service) to start it again, or run pitwall-server.";

/// One request/response call per management method.
pub trait ServerClient: Send + Sync {
    /// Calls `method` with JSON `args` (camelCase) and waits at most `timeout`; `None` waits for as long as the connection
    /// lives (a job's long operation): it ends when the server answers or the connection closes.
    fn call<'a>(&'a self, method: &'a str, args: Value, timeout: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>>;

    /// Opens a server-sent event stream: `GET path` without a body, or `POST path` with one. The head must arrive
    /// within `open_timeout`.
    fn open_stream<'a>(
        &'a self,
        path: &'a str,
        body: Option<Value>,
        open_timeout: Duration,
    ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>>;
}

/// The `data:` payloads of an open event stream.
pub trait FrameStream: Send {
    /// The next payload; `None` when the server ended the stream.
    fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<String>, CommandError>>;
}

/// The error for a server that cannot be reached.
pub fn unavailable() -> CommandError {
    CommandError::new(
        ErrorCode::ServerUnavailable,
        format!("Pitwall's server is not running. {START_HINT}"),
    )
}

/// The error for a request that did not finish in time.
pub fn timed_out(after: Duration) -> CommandError {
    CommandError::new(
        ErrorCode::ServerUnavailable,
        format!(
            "Pitwall's server did not answer within {}s; it may be busy. Retry, or restart the service.",
            after.as_secs().max(1)
        ),
    )
}

/// Maps a transport failure to the typed error the UI shows.
pub fn map_loopback_error(e: &LoopbackError, timeout: Duration) -> CommandError {
    match e {
        LoopbackError::Connect(..) => unavailable(),
        LoopbackError::Timeout(_) => timed_out(timeout),
        LoopbackError::Http(m) => CommandError::new(
            ErrorCode::ServerUnavailable,
            format!("The connection to Pitwall's server broke ({m}). Retry, or restart the service."),
        ),
        LoopbackError::Invalid(m) => CommandError::new(ErrorCode::Internal, format!("The app built an invalid request: {m}")),
        LoopbackError::Body(m) => CommandError::new(ErrorCode::Internal, format!("Pitwall's server sent an unusable answer ({m}).")),
    }
}

/// The error for a non-success answer: the server's own `CommandError` when the body is one, else one made from the
/// status.
pub fn error_from_response(status: u16, body: &Value) -> CommandError {
    if let Ok(e) = serde_json::from_value::<CommandError>(body.clone()) {
        return e;
    }
    let code = match status {
        401 => ErrorCode::Unauthorized,
        403 => ErrorCode::Forbidden,
        404 => ErrorCode::NotFound,
        400 | 422 => ErrorCode::BadRequest,
        409 => ErrorCode::Conflict,
        503 => ErrorCode::ServerUnavailable,
        _ => ErrorCode::Internal,
    };
    CommandError::new(code, format!("Pitwall's server answered with status {status}."))
}

/// The ports to try, most likely first: the one in `discovery.json` (bounded and strictly parsed), then the well-known
/// one. A stale file that points at a dead port falls through to the default.
pub fn candidate_ports(discovery_text: Option<&str>) -> Vec<LoopbackPort> {
    candidate_ports_with(discovery_text, Some(DEFAULT_PORT))
}

/// [`candidate_ports`] with the well-known port given (`None`: only what the discovery file names).
fn candidate_ports_with(discovery_text: Option<&str>, default: Option<u16>) -> Vec<LoopbackPort> {
    let from_file = discovery_text
        .and_then(|t| pitwall_core::discovery::Discovery::parse(t).ok())
        .and_then(|d| LoopbackPort::new(d.port));
    let mut out: Vec<LoopbackPort> = from_file.into_iter().collect();
    if let Some(p) = default.and_then(LoopbackPort::new) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// The real client: loopback HTTP with the admin key.
pub struct LoopbackClient {
    data_dir: PathBuf,
    keys: Arc<dyn AdminKeys>,
    /// Whether the well-known port is tried after the one in the discovery file (always, except in tests).
    try_default_port: bool,
}

impl LoopbackClient {
    /// A client of the server whose data folder is `data_dir`.
    pub fn new(data_dir: PathBuf, keys: Arc<dyn AdminKeys>) -> LoopbackClient {
        LoopbackClient {
            data_dir,
            keys,
            try_default_port: true,
        }
    }

    /// Tests only: never fall back to the well-known port, which a Pitwall installed on the machine may be using.
    #[cfg(test)]
    fn discovery_port_only(mut self) -> LoopbackClient {
        self.try_default_port = false;
        self
    }

    fn ports(&self) -> Vec<LoopbackPort> {
        let path = self.data_dir.join(pitwall_core::discovery::FILE_NAME);
        let text = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.len() as usize <= pitwall_core::discovery::MAX_BYTES)
            .and_then(|_| std::fs::read_to_string(path).ok());
        candidate_ports_with(text.as_deref(), self.try_default_port.then_some(DEFAULT_PORT))
    }

    async fn call_once(&self, method: &str, args: &Value, timeout: Option<Duration>, key: Option<String>) -> Result<Value, CommandError> {
        let path = format!("/mgmt/v1/rpc/{method}");
        let mut last = unavailable();
        for port in self.ports() {
            let client = Client::new(port, key.clone());
            let sent = match timeout {
                Some(t) => client.json("POST", &path, Some(args), t).await,
                None => client.json_no_deadline("POST", &path, Some(args)).await,
            };
            match sent {
                Ok((status, body)) if (200..300).contains(&status) => return Ok(body),
                Ok((status, body)) => return Err(error_from_response(status, &body)),
                Err(LoopbackError::Connect(..)) => last = unavailable(),
                // Only the connect timeout can fire without a deadline of the caller's own.
                Err(e) => return Err(map_loopback_error(&e, timeout.unwrap_or(Duration::from_secs(3)))),
            }
        }
        Err(last)
    }
}

impl ServerClient for LoopbackClient {
    fn call<'a>(&'a self, method: &'a str, args: Value, timeout: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>> {
        Box::pin(async move {
            if !valid_method(method) {
                return Err(CommandError::new(ErrorCode::BadRequest, "not a management method"));
            }
            let args = if args.is_null() { Value::Object(Default::default()) } else { args };
            match self.call_once(method, &args, timeout, self.keys.current()).await {
                Err(e) if e.code == ErrorCode::Unauthorized => {
                    // The server may have made a new admin key (data folder reset): read it fresh, once.
                    let fresh = self.keys.refresh();
                    self.call_once(method, &args, timeout, fresh).await
                }
                other => other,
            }
        })
    }

    fn open_stream<'a>(
        &'a self,
        path: &'a str,
        body: Option<Value>,
        open_timeout: Duration,
    ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>> {
        Box::pin(async move {
            let mut refreshed = false;
            let mut key = self.keys.current();
            loop {
                let mut last = unavailable();
                let mut unauthorized = false;
                for port in self.ports() {
                    let client = Client::new(port, key.clone());
                    let (method, bytes) = match &body {
                        Some(b) => ("POST", Some(serde_json::to_vec(b).unwrap_or_default())),
                        None => ("GET", None),
                    };
                    match client.send(method, path, bytes, open_timeout).await {
                        Ok(resp) if (200..300).contains(&resp.status) => {
                            return Ok(Box::new(SseFrames {
                                resp,
                                decoder: pitwall_core::sse::Decoder::new(),
                                ready: std::collections::VecDeque::new(),
                            }) as Box<dyn FrameStream>)
                        }
                        Ok(resp) => {
                            let status = resp.status;
                            let raw = resp.bytes(64 * 1024).await.unwrap_or_default();
                            let err = error_from_response(status, &serde_json::from_slice(&raw).unwrap_or(Value::Null));
                            if err.code == ErrorCode::Unauthorized && !refreshed {
                                unauthorized = true;
                                break;
                            }
                            return Err(err);
                        }
                        Err(LoopbackError::Connect(..)) => last = unavailable(),
                        Err(e) => return Err(map_loopback_error(&e, open_timeout)),
                    }
                }
                if unauthorized {
                    refreshed = true;
                    key = self.keys.refresh();
                    continue;
                }
                return Err(last);
            }
        })
    }
}

/// Method names are `snake_case` identifiers; anything else never reaches the URL.
pub fn valid_method(m: &str) -> bool {
    !m.is_empty() && m.len() <= 64 && m.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

struct SseFrames {
    resp: pitwall_loopback::Response,
    decoder: pitwall_core::sse::Decoder,
    ready: std::collections::VecDeque<String>,
}

impl FrameStream for SseFrames {
    fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<String>, CommandError>> {
        Box::pin(async move {
            loop {
                if let Some(f) = self.ready.pop_front() {
                    return Ok(Some(f));
                }
                match self.resp.chunk().await {
                    Ok(Some(c)) => {
                        let events = self
                            .decoder
                            .feed(&c)
                            .map_err(|e| CommandError::new(ErrorCode::Internal, format!("Pitwall's live feed is unusable: {e}")))?;
                        self.ready.extend(events.into_iter().map(|e| e.data));
                    }
                    Ok(None) => return Ok(None),
                    Err(e) => return Err(map_loopback_error(&e, Duration::from_secs(0))),
                }
            }
        })
    }
}

/// A scripted server for tests: answers per method, can hang, and serves canned stream frames.
pub mod fake {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// What the fake does for a method.
    #[derive(Clone)]
    pub enum Reply {
        /// Answer with this JSON.
        Ok(Value),
        /// Fail with this error.
        Err(CommandError),
        /// Never answer (the caller's timeout must fire).
        Hang,
    }

    /// The fake server.
    #[derive(Default)]
    pub struct FakeServer {
        replies: Mutex<HashMap<String, Reply>>,
        calls: Mutex<Vec<(String, Value)>>,
        stream: Mutex<Option<Result<Vec<String>, CommandError>>>,
    }

    impl FakeServer {
        /// A fake that answers `null` to everything not scripted.
        pub fn new() -> FakeServer {
            FakeServer::default()
        }
        /// Scripts a reply.
        pub fn reply(&self, method: &str, reply: Reply) -> &Self {
            self.replies
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(method.to_string(), reply);
            self
        }
        /// Scripts the next stream: its frames, or the error opening it fails with.
        pub fn stream(&self, frames: Result<Vec<String>, CommandError>) -> &Self {
            *self.stream.lock().unwrap_or_else(|p| p.into_inner()) = Some(frames);
            self
        }
        /// The calls made so far.
        pub fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
        }
    }

    struct Frames(std::collections::VecDeque<String>);

    impl FrameStream for Frames {
        fn next_frame(&mut self) -> BoxFuture<'_, Result<Option<String>, CommandError>> {
            Box::pin(async move { Ok(self.0.pop_front()) })
        }
    }

    impl ServerClient for FakeServer {
        fn call<'a>(&'a self, method: &'a str, args: Value, timeout: Option<Duration>) -> BoxFuture<'a, Result<Value, CommandError>> {
            Box::pin(async move {
                self.calls
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((method.to_string(), args));
                let reply = self.replies.lock().unwrap_or_else(|p| p.into_inner()).get(method).cloned();
                match reply {
                    Some(Reply::Ok(v)) => Ok(v),
                    Some(Reply::Err(e)) => Err(e),
                    Some(Reply::Hang) => {
                        // A real transport enforces its own timeout (none: it waits for the connection); the fake does the same.
                        match timeout {
                            Some(t) => {
                                tokio::time::sleep(t).await;
                                Err(timed_out(t))
                            }
                            None => std::future::pending().await,
                        }
                    }
                    None => Ok(Value::Null),
                }
            })
        }

        fn open_stream<'a>(
            &'a self,
            _path: &'a str,
            _body: Option<Value>,
            _open_timeout: Duration,
        ) -> BoxFuture<'a, Result<Box<dyn FrameStream>, CommandError>> {
            Box::pin(async move {
                let scripted = self.stream.lock().unwrap_or_else(|p| p.into_inner()).take();
                match scripted {
                    Some(Ok(frames)) => Ok(Box::new(Frames(frames.into())) as Box<dyn FrameStream>),
                    Some(Err(e)) => Err(e),
                    None => Err(unavailable()),
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::StaticKeys;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn transport_failures_become_typed_errors_with_a_next_step() {
        let t = Duration::from_secs(15);
        let e = map_loopback_error(&LoopbackError::Connect(11435, "refused".into()), t);
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("Repair service"), "{}", e.message);
        let e = map_loopback_error(&LoopbackError::Timeout(t), t);
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("15s") && e.message.contains("Retry"), "{}", e.message);
        assert!(map_loopback_error(&LoopbackError::Http("eof".into()), t).message.contains("Retry"));
        assert_eq!(map_loopback_error(&LoopbackError::Invalid("x"), t).code, ErrorCode::Internal);
        assert_eq!(map_loopback_error(&LoopbackError::Body("big".into()), t).code, ErrorCode::Internal);
    }

    #[test]
    fn the_servers_own_error_is_kept_and_other_answers_get_one_from_the_status() {
        let own = json!({"code": "insufficientMemory", "message": "needs 8 GB"});
        let e = error_from_response(500, &own);
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::InsufficientMemory, "needs 8 GB"));
        for (status, code) in [
            (401, ErrorCode::Unauthorized),
            (403, ErrorCode::Forbidden),
            (404, ErrorCode::NotFound),
            (400, ErrorCode::BadRequest),
            (409, ErrorCode::Conflict),
            (503, ErrorCode::ServerUnavailable),
            (500, ErrorCode::Internal),
        ] {
            assert_eq!(error_from_response(status, &Value::Null).code, code, "{status}");
        }
    }

    #[test]
    fn the_discovery_port_comes_first_then_the_default_and_a_hostile_file_is_ignored() {
        let d = pitwall_core::discovery::Discovery::new("0.1.0", 1, 12345, 0);
        let text = serde_json::to_string(&d).unwrap();
        let ports: Vec<u16> = candidate_ports(Some(&text)).iter().map(|p| p.get()).collect();
        assert_eq!(ports, vec![12345, DEFAULT_PORT]);
        let same = serde_json::to_string(&pitwall_core::discovery::Discovery::new("0.1.0", 1, DEFAULT_PORT, 0)).unwrap();
        assert_eq!(candidate_ports(Some(&same)).len(), 1);
        let evil = text.replace("127.0.0.1", "evil.example");
        assert_eq!(
            candidate_ports(Some(&evil)).iter().map(|p| p.get()).collect::<Vec<_>>(),
            vec![DEFAULT_PORT]
        );
        assert_eq!(candidate_ports(None).len(), 1);
    }

    #[test]
    fn only_snake_case_method_names_reach_the_url() {
        assert!(valid_method("set_serving"));
        assert!(!valid_method(""));
        assert!(!valid_method("a/b"));
        assert!(!valid_method("a b"));
        assert!(!valid_method("../x"));
        assert!(!valid_method(&"a".repeat(65)));
    }

    /// A one-shot HTTP server: reads the request, answers with `status` and `body` (plus the request head to `seen`).
    async fn serve_once(status: u16, content_type: &str, body: String) -> (u16, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let ct = content_type.to_string();
        let h = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap();
            let head = String::from_utf8_lossy(&buf[..n]).to_string();
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: {ct}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(reply.as_bytes()).await;
            let _ = s.shutdown().await;
            head
        });
        (port, h)
    }

    fn client_for(port: u16, key: &str) -> (tempfile::TempDir, LoopbackClient) {
        let d = tempfile::tempdir().unwrap();
        let disc = pitwall_core::discovery::Discovery::new("0.1.0", 1, port.max(1024), 0);
        std::fs::write(d.path().join("discovery.json"), serde_json::to_string(&disc).unwrap()).unwrap();
        let c = LoopbackClient::new(d.path().to_path_buf(), Arc::new(StaticKeys::new(key)));
        (d, c)
    }

    #[tokio::test]
    async fn a_call_posts_json_with_the_bearer_key_and_returns_the_answer() {
        let (port, seen) = serve_once(200, "application/json", r#"{"ok":true}"#.into()).await;
        let (_d, c) = client_for(port, "sekrit-key");
        let v = c
            .call("set_serving", json!({"paused": true}), Some(Duration::from_secs(5)))
            .await
            .unwrap();
        assert_eq!(v, json!({"ok": true}));
        let head = seen.await.unwrap();
        assert!(head.starts_with("POST /mgmt/v1/rpc/set_serving HTTP/1.1"), "{head}");
        assert!(head.to_lowercase().contains("authorization: bearer sekrit-key"), "{head}");
        assert!(head.contains(r#"{"paused":true}"#), "{head}");
    }

    #[tokio::test]
    async fn the_servers_error_body_reaches_the_caller() {
        let (port, _h) = serve_once(409, "application/json", r#"{"code":"conflict","message":"already loaded"}"#.into()).await;
        let (_d, c) = client_for(port, "k");
        let e = c
            .call("load_model", json!({"id": "m"}), Some(Duration::from_secs(5)))
            .await
            .unwrap_err();
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::Conflict, "already loaded"));
    }

    #[tokio::test]
    async fn a_server_that_is_not_running_is_unavailable_with_a_next_step() {
        // A port nobody listens on (bound then dropped).
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let (_d, c) = client_for(port, "k");
        // Hermetic: an installed Pitwall may listen on the well-known port, so only the discovery port is tried.
        let c = c.discovery_port_only();
        let e = c.call("overview", Value::Null, Some(Duration::from_secs(5))).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("Repair service"));
        let e = c.open_stream("/mgmt/v1/live", None, Duration::from_secs(5)).await.err().unwrap();
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
    }

    #[tokio::test]
    async fn a_server_that_does_not_answer_times_out_with_a_retry_hint() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let _held = tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(s);
        });
        let (_d, c) = client_for(port, "k");
        let started = std::time::Instant::now();
        let e = c.call("overview", Value::Null, Some(Duration::from_millis(300))).await.unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(e.code, ErrorCode::ServerUnavailable);
        assert!(e.message.contains("Retry"), "{}", e.message);
    }

    #[tokio::test]
    async fn a_stream_is_decoded_into_data_frames() {
        let body = "data: {\"kind\":\"memory\"}\n\n: keep-alive\n\ndata: {\"kind\":\"queue\"}\n\n";
        let (port, _h) = serve_once(200, "text/event-stream", body.into()).await;
        let (_d, c) = client_for(port, "k");
        let mut s = c.open_stream("/mgmt/v1/live", None, Duration::from_secs(5)).await.unwrap();
        assert_eq!(s.next_frame().await.unwrap().as_deref(), Some("{\"kind\":\"memory\"}"));
        assert_eq!(s.next_frame().await.unwrap().as_deref(), Some("{\"kind\":\"queue\"}"));
        assert_eq!(s.next_frame().await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_unknown_method_never_leaves_the_app() {
        let (_d, c) = client_for(11436, "k");
        let e = c.call("../etc", Value::Null, Some(Duration::from_secs(1))).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn the_fake_hangs_until_the_timeout_and_scripts_replies() {
        use fake::{FakeServer, Reply};
        let f = FakeServer::new();
        f.reply("overview", Reply::Ok(json!({"a": 1}))).reply("slow", Reply::Hang);
        assert_eq!(
            f.call("overview", Value::Null, Some(Duration::from_secs(1))).await.unwrap(),
            json!({"a": 1})
        );
        assert_eq!(
            f.call("other", Value::Null, Some(Duration::from_secs(1))).await.unwrap(),
            Value::Null
        );
        let e = f.call("slow", Value::Null, Some(Duration::from_millis(20))).await.unwrap_err();
        assert!(e.message.contains("Retry"));
        assert_eq!(f.calls().len(), 3);
        assert!(f.open_stream("/x", None, Duration::from_secs(1)).await.is_err());
        f.stream(Ok(vec!["a".into()]));
        let mut s = f.open_stream("/x", None, Duration::from_secs(1)).await.unwrap();
        assert_eq!(s.next_frame().await.unwrap().as_deref(), Some("a"));
        assert_eq!(s.next_frame().await.unwrap(), None);
    }
}
