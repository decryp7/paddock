//! A client connection over any reader and writer (ADR 0023 WP-8): requests with ids, one read loop that routes every reply to
//! the call waiting for it, paged `tools/list`, cancel through `notifications/cancelled`, a line limit, per-call timeouts
//! and typed errors whose text says what to do.
//!
//! Calls do not block each other: [`Connection::start`] returns a [`PendingCall`] at once, any thread may [`Canceller::cancel`]
//! it, and `wait` returns when the reply, the cancel, the timeout or the end of the connection comes first. Nothing here
//! times out a call unless the caller asks for a limit (ER-19: the UI shows "Still working" instead).

use crate::client::{
    call_tool, initialize, initialized, list_tools, parse_call, parse_initialize, parse_tools, ping,
};
use crate::jsonrpc::{Id, RpcError, notification_line, parse_response};
use crate::lines::{Line, read_line};
use crate::types::{CallToolResult, InitializeResult, PROTOCOL_VERSIONS, Tool};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The most pages of `tools/list` one listing follows, so a server that never ends its list cannot keep a client busy.
pub const MAX_LIST_PAGES: usize = 32;

/// What can go wrong on a connection. `Display` says what to do next.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// The connection ended (the app quit or restarted, AI access was turned off or the key revoked, a line was over the
    /// limit, or this side closed it). Every call still waiting gets this.
    Closed {
        /// What ended it.
        cause: String,
    },
    /// A call did not finish in the time the caller allowed; the server was told to stop it.
    Timeout {
        /// The method.
        method: String,
        /// The limit.
        after: Duration,
    },
    /// The call was cancelled by the caller.
    Cancelled,
    /// The server answered with a protocol error.
    Rpc {
        /// The JSON-RPC code.
        code: i64,
        /// The server's message.
        message: String,
    },
    /// The server's result was not the shape the method promises.
    BadResult {
        /// The method.
        method: String,
        /// What is wrong.
        detail: String,
    },
    /// The server speaks a protocol version this client does not know.
    UnsupportedVersion {
        /// The version it chose.
        server: String,
    },
    /// The server's tool list never ended.
    Paging {
        /// What is wrong.
        detail: String,
    },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Closed { cause } => write!(
                f,
                "The connection ended ({cause}). Check that the app is still open with AI access on, then connect again."
            ),
            ClientError::Timeout { method, after } => write!(
                f,
                "{method} did not finish in {} s and was cancelled. Try again, or ask for something smaller.",
                after.as_secs_f32()
            ),
            ClientError::Cancelled => f.write_str("Stopped."),
            ClientError::Rpc { code, message } => {
                write!(f, "The app refused the request ({code}): {message}")
            }
            ClientError::BadResult { method, detail } => write!(
                f,
                "The app's answer to {method} was not understood ({detail}). Update both apps to the same release."
            ),
            ClientError::UnsupportedVersion { server } => write!(
                f,
                "The app speaks MCP version {server}, which this app does not. Update both apps to the same release."
            ),
            ClientError::Paging { detail } => write!(
                f,
                "The app's tool list did not end ({detail}). Update both apps to the same release."
            ),
        }
    }
}

impl ClientError {
    /// A short stable name for logs.
    pub fn kind(&self) -> &'static str {
        match self {
            ClientError::Closed { .. } => "closed",
            ClientError::Timeout { .. } => "timeout",
            ClientError::Cancelled => "cancelled",
            ClientError::Rpc { .. } => "rpc-error",
            ClientError::BadResult { .. } => "bad-result",
            ClientError::UnsupportedVersion { .. } => "unsupported-version",
            ClientError::Paging { .. } => "paging",
        }
    }
}

impl std::error::Error for ClientError {}

/// How a [`Connection`] behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionConfig {
    /// The longest line read from the server; a longer one ends the connection (the server cuts its own results, so this is
    /// a guard, not a normal path).
    pub max_line_bytes: usize,
    /// How long `initialize`, `tools/list` and `ping` may take. A tool call has no default limit: the caller passes one.
    pub setup_timeout: Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        ConnectionConfig {
            max_line_bytes: 4 * 1024 * 1024,
            setup_timeout: Duration::from_secs(30),
        }
    }
}

type Reply = Result<Value, ClientError>;

#[derive(Default)]
struct State {
    pending: HashMap<Id, Sender<Reply>>,
    /// Why the connection ended, once it has.
    closed: Option<String>,
}

struct Shared {
    state: Mutex<State>,
    writer: Mutex<Box<dyn Write + Send>>,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// Ends the connection for every waiting call. The first cause stays.
    fn close(&self, cause: &str) {
        let waiting: Vec<Sender<Reply>> = {
            let mut st = locked(&self.state);
            if st.closed.is_none() {
                st.closed = Some(cause.to_string());
            }
            st.pending.drain().map(|(_, tx)| tx).collect()
        };
        let cause = locked(&self.state).closed.clone().unwrap_or_default();
        for tx in waiting {
            let _ = tx.send(Err(ClientError::Closed {
                cause: cause.clone(),
            }));
        }
    }

    fn send_line(&self, line: &str) -> Result<(), ClientError> {
        let result = {
            let mut w = locked(&self.writer);
            w.write_all(line.as_bytes())
                .and_then(|()| w.write_all(b"\n"))
                .and_then(|()| w.flush())
        };
        if let Err(e) = result {
            let cause = format!("could not write to the app: {e}");
            self.close(&cause);
            return Err(ClientError::Closed { cause });
        }
        Ok(())
    }

    /// Stops waiting for `id` and tells the server (MCP: it then never answers). False when the call had already finished.
    fn abandon(&self, id: &Id, reason: &str, with: Option<ClientError>) -> bool {
        let tx = locked(&self.state).pending.remove(id);
        let Some(tx) = tx else { return false };
        if let Some(e) = with {
            let _ = tx.send(Err(e));
        }
        let line = notification_line(
            "notifications/cancelled",
            Some(json!({"requestId": id, "reason": reason})),
        );
        // A failed write already closed the connection and says so to everyone else.
        let _ = self.send_line(&line);
        true
    }

    fn dispatch(&self, line: &str) {
        match parse_response(line) {
            Ok((Some(id), result)) => {
                let tx = locked(&self.state).pending.remove(&id);
                match tx {
                    Some(tx) => {
                        let _ = tx.send(result.map_err(|e: RpcError| ClientError::Rpc {
                            code: e.code,
                            message: e.message,
                        }));
                    }
                    // A reply to a call that was cancelled or timed out meanwhile.
                    None => log::debug!("paddock_mcp client: reply to a call nobody waits for"),
                }
            }
            // A notification or request from the server, or an error without an id: nothing here sends requests that need it.
            _ => log::debug!("paddock_mcp client: ignored a line that answers no call"),
        }
    }
}

/// The read loop: one thread per connection, ends with the connection.
fn read_loop(shared: &Shared, mut reader: Box<dyn BufRead + Send>, max_line: usize) {
    let cause = loop {
        match read_line(&mut reader, max_line) {
            Ok(Line::Text(t)) => shared.dispatch(&t),
            Ok(Line::TooLong) => break format!("the app sent a line over {max_line} bytes"),
            Ok(Line::Eof) => break "the app ended the connection".to_string(),
            Err(e) => break format!("reading from the app failed: {e}"),
        }
    };
    log::info!("paddock_mcp client: connection ended: {cause}");
    shared.close(&cause);
}

/// A connection to an MCP server: requests with ids over a reader and a writer.
pub struct Connection {
    shared: Arc<Shared>,
    next_id: AtomicI64,
    closer: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    config: ConnectionConfig,
}

impl Connection {
    /// Starts the read loop over `reader`. `closer` is run when the connection is dropped, to end a blocked read (a socket
    /// shutdown).
    pub fn new(
        reader: Box<dyn BufRead + Send>,
        writer: Box<dyn Write + Send>,
        closer: Option<Box<dyn FnOnce() + Send>>,
        config: ConnectionConfig,
    ) -> Connection {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            writer: Mutex::new(writer),
        });
        let for_loop = shared.clone();
        let max = config.max_line_bytes;
        // Detached: it ends when the reader does, which `closer` or the app's end of the connection brings about.
        std::thread::spawn(move || read_loop(&for_loop, reader, max));
        Connection {
            shared,
            next_id: AtomicI64::new(1),
            closer: Mutex::new(closer),
            config,
        }
    }

    /// Why the connection ended, or `None` while it is open.
    pub fn closed(&self) -> Option<String> {
        locked(&self.shared.state).closed.clone()
    }

    /// Sends a request built by `line_of` with a fresh id and returns at once.
    fn start_with(
        &self,
        method: &str,
        line_of: impl FnOnce(i64) -> String,
    ) -> Result<PendingCall, ClientError> {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = Id::Num(n);
        let (tx, rx) = channel();
        {
            let mut st = locked(&self.shared.state);
            if let Some(cause) = &st.closed {
                return Err(ClientError::Closed {
                    cause: cause.clone(),
                });
            }
            st.pending.insert(id.clone(), tx);
        }
        if let Err(e) = self.shared.send_line(&line_of(n)) {
            locked(&self.shared.state).pending.remove(&id);
            return Err(e);
        }
        Ok(PendingCall {
            id,
            method: method.to_string(),
            rx,
            shared: self.shared.clone(),
            started: Instant::now(),
        })
    }

    /// Sends `tools/call` and returns at once; [`PendingCall::wait_tool`] collects the result and [`PendingCall::canceller`]
    /// stops it from another thread.
    ///
    /// # Errors
    /// [`ClientError::Closed`] when the connection has ended.
    pub fn start_call_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<PendingCall, ClientError> {
        self.start_with("tools/call", |id| call_tool(id, name, arguments))
    }

    /// Calls a tool and waits for the result. `timeout` is the caller's limit, `None` waits as long as it takes. The server
    /// reports a failing tool as a result with `is_error`, which is not an error here.
    ///
    /// # Errors
    /// A [`ClientError`]; [`ClientError::Timeout`] has already told the server to stop the call.
    pub fn call_tool(
        &self,
        name: &str,
        arguments: Value,
        timeout: Option<Duration>,
    ) -> Result<CallToolResult, ClientError> {
        self.start_call_tool(name, arguments)?.wait_tool(timeout)
    }

    /// `initialize`, the version check and `notifications/initialized`.
    ///
    /// # Errors
    /// A [`ClientError`]; [`ClientError::UnsupportedVersion`] when the server chose a version this crate does not know.
    pub fn initialize(
        &self,
        client_name: &str,
        client_version: &str,
    ) -> Result<InitializeResult, ClientError> {
        let value = self
            .start_with("initialize", |id| {
                initialize(id, client_name, client_version)
            })?
            .wait(Some(self.config.setup_timeout))?;
        let init = parse_initialize(value).map_err(|detail| ClientError::BadResult {
            method: "initialize".into(),
            detail,
        })?;
        if !PROTOCOL_VERSIONS.contains(&init.protocol_version.as_str()) {
            return Err(ClientError::UnsupportedVersion {
                server: init.protocol_version,
            });
        }
        self.shared.send_line(&initialized())?;
        log::info!(
            "paddock_mcp client: initialized with {} {}",
            init.server_info.name,
            init.server_info.version
        );
        Ok(init)
    }

    /// Every tool the server lists, following the pages (at most [`MAX_LIST_PAGES`]).
    ///
    /// # Errors
    /// A [`ClientError`]; [`ClientError::Paging`] when the list repeats a cursor or does not end.
    pub fn list_tools(&self) -> Result<Vec<Tool>, ClientError> {
        let started = Instant::now();
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let used = cursor.clone();
            let value = self
                .start_with("tools/list", |id| list_tools(id, used.as_deref()))?
                .wait(Some(self.config.setup_timeout))?;
            let page = parse_tools(value).map_err(|detail| ClientError::BadResult {
                method: "tools/list".into(),
                detail,
            })?;
            tools.extend(page.tools);
            match page.next_cursor {
                None => {
                    log::info!(
                        "paddock_mcp client: listed {} tools in {} ms",
                        tools.len(),
                        started.elapsed().as_millis()
                    );
                    return Ok(tools);
                }
                Some(next) if Some(&next) == cursor.as_ref() => {
                    return Err(ClientError::Paging {
                        detail: "a page repeated its cursor".into(),
                    });
                }
                Some(next) => cursor = Some(next),
            }
        }
        Err(ClientError::Paging {
            detail: format!("more than {MAX_LIST_PAGES} pages"),
        })
    }

    /// `ping`.
    ///
    /// # Errors
    /// A [`ClientError`].
    pub fn ping(&self) -> Result<(), ClientError> {
        self.start_with("ping", ping)?
            .wait(Some(self.config.setup_timeout))
            .map(|_| ())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shared.close("this side closed the connection");
        if let Some(closer) = locked(&self.closer).take() {
            closer();
        }
    }
}

/// A request that was sent and has not finished.
pub struct PendingCall {
    id: Id,
    method: String,
    rx: Receiver<Reply>,
    shared: Arc<Shared>,
    started: Instant,
}

impl PendingCall {
    /// A handle that cancels this call from any thread.
    pub fn canceller(&self) -> Canceller {
        Canceller {
            id: self.id.clone(),
            shared: self.shared.clone(),
        }
    }

    /// Waits for the reply. On a timeout the server is told to stop the call.
    ///
    /// # Errors
    /// A [`ClientError`].
    pub fn wait(self, timeout: Option<Duration>) -> Result<Value, ClientError> {
        let reply = match timeout {
            None => self.rx.recv().ok(),
            Some(limit) => match self.rx.recv_timeout(limit) {
                Ok(reply) => Some(reply),
                Err(_) => return self.after_limit(limit),
            },
        };
        let result = reply.unwrap_or_else(|| {
            Err(ClientError::Closed {
                cause: "the call was dropped".into(),
            })
        });
        let outcome = match &result {
            Ok(_) => "ok",
            Err(e) => e.kind(),
        };
        log::info!(
            "paddock_mcp client: {} {outcome} in {} ms",
            self.method,
            self.started.elapsed().as_millis()
        );
        result
    }

    /// The limit passed: the call is cancelled, unless its reply (or the end of the connection) got in first.
    fn after_limit(&self, limit: Duration) -> Reply {
        if self.shared.abandon(&self.id, "timed out", None) {
            log::warn!(
                "paddock_mcp client: {} timed out after {} ms and was cancelled",
                self.method,
                limit.as_millis()
            );
            return Err(ClientError::Timeout {
                method: self.method.clone(),
                after: limit,
            });
        }
        self.rx.try_recv().unwrap_or_else(|_| {
            Err(ClientError::Closed {
                cause: "the call was dropped".into(),
            })
        })
    }

    /// [`PendingCall::wait`], read as a `tools/call` result.
    ///
    /// # Errors
    /// A [`ClientError`]; [`ClientError::BadResult`] when the reply is not a tool result.
    pub fn wait_tool(self, timeout: Option<Duration>) -> Result<CallToolResult, ClientError> {
        let value = self.wait(timeout)?;
        parse_call(value).map_err(|detail| ClientError::BadResult {
            method: "tools/call".into(),
            detail,
        })
    }
}

/// Cancels one call from any thread.
#[derive(Clone)]
pub struct Canceller {
    id: Id,
    shared: Arc<Shared>,
}

impl Canceller {
    /// Stops waiting for the call (its `wait` returns [`ClientError::Cancelled`]) and sends `notifications/cancelled` with
    /// `reason`. False when the call had already finished.
    pub fn cancel(&self, reason: &str) -> bool {
        self.shared
            .abandon(&self.id, reason, Some(ClientError::Cancelled))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{self, Cursor, Read};
    use std::sync::mpsc;

    /// The server's bytes, fed from the test.
    struct ChanReader {
        rx: Receiver<io::Result<Vec<u8>>>,
        buf: Cursor<Vec<u8>>,
    }

    impl Read for ChanReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            loop {
                let n = self.buf.read(out)?;
                if n > 0 {
                    return Ok(n);
                }
                match self.rx.recv() {
                    Ok(Ok(bytes)) => self.buf = Cursor::new(bytes),
                    Ok(Err(e)) => return Err(e),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    /// What the client wrote, line by line.
    struct ChanWriter {
        tx: Sender<String>,
        part: Vec<u8>,
        fail: bool,
    }

    impl Write for ChanWriter {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            if self.fail {
                return Err(io::Error::other("broken pipe"));
            }
            self.part.extend_from_slice(b);
            while let Some(i) = self.part.iter().position(|c| *c == b'\n') {
                let line: Vec<u8> = self.part.drain(..=i).collect();
                let _ = self
                    .tx
                    .send(String::from_utf8_lossy(&line[..i]).into_owned());
            }
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Rig {
        conn: Connection,
        /// What the client sent.
        sent: Receiver<String>,
        /// The server's side: bytes to the client.
        feed: Sender<io::Result<Vec<u8>>>,
    }

    impl Rig {
        fn new(config: ConnectionConfig) -> Rig {
            Rig::with(config, false, None)
        }

        fn with(
            config: ConnectionConfig,
            fail: bool,
            closer: Option<Box<dyn FnOnce() + Send>>,
        ) -> Rig {
            listen();
            let (feed, rx) = mpsc::channel();
            let (tx, sent) = mpsc::channel();
            let reader = io::BufReader::new(ChanReader {
                rx,
                buf: Cursor::new(vec![]),
            });
            let writer = ChanWriter {
                tx,
                part: vec![],
                fail,
            };
            Rig {
                conn: Connection::new(Box::new(reader), Box::new(writer), closer, config),
                sent,
                feed,
            }
        }

        fn say(&self, line: &str) {
            self.feed
                .send(Ok(format!("{line}\n").into_bytes()))
                .unwrap();
        }

        fn reply(&self, id: i64, result: Value) {
            self.say(&json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string());
        }

        /// The next line the client sent, as JSON.
        fn next(&self) -> Value {
            serde_json::from_str(&self.sent.recv().unwrap()).unwrap()
        }

        /// A server thread that answers each request with what `f` makes of it (`None`: no answer).
        fn serve(
            self,
            f: impl Fn(&Value) -> Option<Value> + Send + 'static,
        ) -> (
            Connection,
            mpsc::Receiver<Value>,
            Sender<io::Result<Vec<u8>>>,
        ) {
            let (seen_tx, seen) = mpsc::channel();
            let feed = self.feed.clone();
            let sent = self.sent;
            let conn = self.conn;
            let feed_for_thread = feed.clone();
            std::thread::spawn(move || {
                while let Ok(line) = sent.recv() {
                    let v: Value = serde_json::from_str(&line).unwrap();
                    if let (Some(id), Some(result)) = (v.get("id"), f(&v)) {
                        let reply = json!({"jsonrpc": "2.0", "id": id, "result": result});
                        let _ = feed_for_thread.send(Ok(format!("{reply}\n").into_bytes()));
                    }
                    let _ = seen_tx.send(v);
                }
            });
            (conn, seen, feed)
        }
    }

    /// A logger that listens, so the log lines' arguments are built (they are not when no logger listens).
    fn listen() {
        struct Quiet;
        impl log::Log for Quiet {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, _: &log::Record<'_>) {}
            fn flush(&self) {}
        }
        let _ = log::set_logger(&Quiet);
        log::set_max_level(log::LevelFilter::Trace);
    }

    fn init_result(version: &str) -> Value {
        json!({"protocolVersion": version, "capabilities": {}, "serverInfo": {"name": "Dyno", "version": "1.1.0"}})
    }

    fn tool(name: &str) -> Value {
        json!({"name": name, "description": "d", "inputSchema": {"type": "object"}})
    }

    #[test]
    fn initialize_checks_the_version_and_sends_initialized() {
        let (conn, seen, _feed) = Rig::new(ConnectionConfig::default())
            .serve(|v| (v["method"] == "initialize").then(|| init_result("2025-06-18")));
        let init = conn.initialize("Pitwall", "1.1.0").unwrap();
        assert_eq!(init.server_info.name, "Dyno");
        let first = seen.recv().unwrap();
        assert_eq!(first["params"]["clientInfo"]["name"], "Pitwall");
        assert_eq!(seen.recv().unwrap()["method"], "notifications/initialized");
        assert!(conn.closed().is_none());
    }

    #[test]
    fn initialize_refuses_an_unknown_version_and_a_wrong_shape() {
        let (conn, _seen, _f) =
            Rig::new(ConnectionConfig::default()).serve(|_| Some(init_result("2099-01-01")));
        let err = conn.initialize("P", "1").unwrap_err();
        assert_eq!(
            err,
            ClientError::UnsupportedVersion {
                server: "2099-01-01".into()
            }
        );
        assert!(err.to_string().contains("Update both apps"), "{err}");

        let (conn, _seen, _f) = Rig::new(ConnectionConfig::default()).serve(|_| Some(json!(3)));
        let err = conn.initialize("P", "1").unwrap_err();
        assert!(
            matches!(err, ClientError::BadResult { ref method, .. } if method == "initialize"),
            "{err:?}"
        );
    }

    #[test]
    fn list_tools_follows_the_pages() {
        let (conn, seen, _f) =
            Rig::new(ConnectionConfig::default()).serve(|v| match v["params"]["cursor"].as_str() {
                None => Some(json!({"tools": [tool("a")], "nextCursor": "p2"})),
                Some(_) => Some(json!({"tools": [tool("b")]})),
            });
        let tools = conn.list_tools().unwrap();
        assert_eq!(
            tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(seen.recv().unwrap()["params"], json!({}));
        assert_eq!(seen.recv().unwrap()["params"]["cursor"], "p2");
    }

    #[test]
    fn a_list_that_repeats_a_cursor_or_never_ends_is_an_error_not_a_loop() {
        let (conn, _s, _f) = Rig::new(ConnectionConfig::default())
            .serve(|_| Some(json!({"tools": [], "nextCursor": "same"})));
        let err = conn.list_tools().unwrap_err();
        assert!(matches!(err, ClientError::Paging { .. }), "{err:?}");
        assert!(err.to_string().contains("repeated its cursor"), "{err}");

        let counter = std::sync::atomic::AtomicUsize::new(0);
        let (conn, _s, _f) = Rig::new(ConnectionConfig::default()).serve(move |_| {
            let n = counter.fetch_add(1, Ordering::Relaxed);
            Some(json!({"tools": [], "nextCursor": format!("c{n}")}))
        });
        let err = conn.list_tools().unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("more than {MAX_LIST_PAGES} pages")),
            "{err}"
        );

        let (conn, _s, _f) = Rig::new(ConnectionConfig::default()).serve(|_| Some(json!("nope")));
        assert!(matches!(
            conn.list_tools().unwrap_err(),
            ClientError::BadResult { .. }
        ));
    }

    #[test]
    fn a_tool_result_a_failing_tool_and_a_protocol_error_are_told_apart() {
        let (conn, seen, feed) =
            Rig::new(ConnectionConfig::default()).serve(|v| match v["params"]["name"].as_str() {
                Some("ok") => Some(json!({"content": [{"type": "text", "text": "fine"}]})),
                Some("bad") => {
                    Some(json!({"content": [{"type": "text", "text": "no"}], "isError": true}))
                }
                Some("shape") => Some(json!([1])),
                _ => None,
            });
        assert_eq!(
            conn.call_tool("ok", json!({"x": 1}), None).unwrap().text(),
            "fine"
        );
        assert_eq!(seen.recv().unwrap()["params"]["arguments"], json!({"x": 1}));
        assert!(conn.call_tool("bad", json!({}), None).unwrap().is_error);
        assert!(matches!(
            conn.call_tool("shape", json!({}), None).unwrap_err(),
            ClientError::BadResult { .. }
        ));
        // A protocol error from the server: answered by id 4 (the fourth request).
        let pending = conn.start_call_tool("unknown", json!({})).unwrap();
        feed.send(Ok(b"{\"jsonrpc\":\"2.0\",\"id\":4,\"error\":{\"code\":-32602,\"message\":\"unknown tool\"}}\n".to_vec())).unwrap();
        let err = pending.wait_tool(None).unwrap_err();
        assert_eq!(
            err,
            ClientError::Rpc {
                code: -32602,
                message: "unknown tool".into()
            }
        );
        assert!(err.to_string().contains("unknown tool"), "{err}");
    }

    #[test]
    fn a_call_that_outlasts_its_limit_is_cancelled_at_the_server() {
        let rig = Rig::new(ConnectionConfig::default());
        let err = rig
            .conn
            .call_tool("slow", json!({}), Some(Duration::from_millis(30)))
            .unwrap_err();
        assert!(
            matches!(err, ClientError::Timeout { ref method, .. } if method == "tools/call"),
            "{err:?}"
        );
        assert!(err.to_string().contains("was cancelled"), "{err}");
        let request = rig.next();
        let cancel = rig.next();
        assert_eq!(cancel["method"], "notifications/cancelled");
        assert_eq!(cancel["params"]["requestId"], request["id"]);
        assert_eq!(cancel["params"]["reason"], "timed out");
        // The late reply is ignored, and the connection still works.
        rig.reply(1, json!({"content": []}));
        let ping = std::thread::scope(|s| {
            let h = s.spawn(|| rig.conn.ping());
            let req = rig.next();
            rig.reply(req["id"].as_i64().unwrap(), json!({}));
            h.join().unwrap()
        });
        assert!(ping.is_ok());
    }

    #[test]
    fn another_thread_can_cancel_a_running_call() {
        let rig = Rig::new(ConnectionConfig::default());
        let pending = rig.conn.start_call_tool("long", json!({})).unwrap();
        let canceller = pending.canceller();
        let request = rig.next();
        let waiter = std::thread::spawn(move || pending.wait(None));
        assert!(canceller.cancel("user pressed Stop"));
        assert_eq!(waiter.join().unwrap().unwrap_err(), ClientError::Cancelled);
        let cancel = rig.next();
        assert_eq!(
            cancel["params"],
            json!({"requestId": request["id"], "reason": "user pressed Stop"})
        );
        assert!(
            !canceller.cancel("again"),
            "a finished call cannot be cancelled twice"
        );
        assert_eq!(ClientError::Cancelled.to_string(), "Stopped.");
    }

    #[test]
    fn a_reply_that_beat_the_limit_is_returned_not_timed_out() {
        let rig = Rig::new(ConnectionConfig::default());
        let pending = rig.conn.start_call_tool("t", json!({})).unwrap();
        // The reply got in after the wait gave up but before the call was abandoned.
        locked(&rig.conn.shared.state).pending.remove(&pending.id);
        let (tx, rx) = channel();
        tx.send(Ok(json!({"content": []}))).unwrap();
        let racing = PendingCall { rx, ..pending };
        assert_eq!(
            racing.after_limit(Duration::from_secs(1)).unwrap(),
            json!({"content": []})
        );
        // Nothing came at all.
        let (_tx, rx) = channel::<Reply>();
        let empty = PendingCall {
            id: Id::Num(99),
            method: "m".into(),
            rx,
            shared: rig.conn.shared.clone(),
            started: Instant::now(),
        };
        assert!(matches!(
            empty.after_limit(Duration::from_secs(1)).unwrap_err(),
            ClientError::Closed { .. }
        ));
        // A sender that vanished without a reply.
        let (tx, rx) = channel::<Reply>();
        drop(tx);
        let gone = PendingCall {
            id: Id::Num(98),
            method: "m".into(),
            rx,
            shared: rig.conn.shared.clone(),
            started: Instant::now(),
        };
        assert!(matches!(
            gone.wait(None).unwrap_err(),
            ClientError::Closed { .. }
        ));
    }

    #[test]
    fn the_end_of_the_connection_fails_every_waiting_call_and_later_ones() {
        let rig = Rig::new(ConnectionConfig::default());
        let a = rig.conn.start_call_tool("a", json!({})).unwrap();
        let b = rig.conn.start_call_tool("b", json!({})).unwrap();
        let Rig {
            conn,
            feed,
            sent: _sent,
        } = rig;
        drop(feed);
        for p in [a, b] {
            let err = p.wait(None).unwrap_err();
            assert!(
                matches!(err, ClientError::Closed { ref cause } if cause.contains("ended the connection")),
                "{err:?}"
            );
            assert!(err.to_string().contains("connect again"), "{err}");
        }
        assert!(conn.closed().is_some());
        assert!(matches!(
            conn.start_call_tool("c", json!({})).err(),
            Some(ClientError::Closed { .. })
        ));
    }

    #[test]
    fn a_line_over_the_limit_and_a_read_error_end_the_connection_with_their_cause() {
        let rig = Rig::new(ConnectionConfig {
            max_line_bytes: 64,
            ..ConnectionConfig::default()
        });
        let pending = rig.conn.start_call_tool("a", json!({})).unwrap();
        rig.say(&"x".repeat(200));
        let err = pending.wait(None).unwrap_err();
        assert!(
            matches!(&err, ClientError::Closed { cause } if cause.contains("over 64 bytes")),
            "{err:?}"
        );
        let rig = Rig::new(ConnectionConfig::default());
        let pending = rig.conn.start_call_tool("a", json!({})).unwrap();
        rig.feed.send(Err(io::Error::other("reset"))).unwrap();
        let err = pending.wait(None).unwrap_err();
        assert!(
            matches!(&err, ClientError::Closed { cause } if cause.contains("reading from the app failed")),
            "{err:?}"
        );
    }

    #[test]
    fn a_failed_write_closes_the_connection_and_says_so() {
        let rig = Rig::with(ConnectionConfig::default(), true, None);
        let err = rig.conn.start_call_tool("a", json!({})).err().unwrap();
        assert!(
            matches!(err, ClientError::Closed { ref cause } if cause.contains("could not write")),
            "{err:?}"
        );
        assert!(rig.conn.closed().is_some());
        assert!(locked(&rig.conn.shared.state).pending.is_empty());
    }

    #[test]
    fn lines_that_answer_no_call_are_ignored() {
        let rig = Rig::new(ConnectionConfig::default());
        rig.say("not json at all");
        rig.say(r#"{"jsonrpc":"2.0","method":"notifications/message","params":{}}"#);
        rig.say(r#"{"jsonrpc":"2.0","id":77,"result":{}}"#);
        rig.say(r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"x"}}"#);
        let pending = rig.conn.start_call_tool("t", json!({})).unwrap();
        let id = rig.next()["id"].as_i64().unwrap();
        rig.reply(
            id,
            json!({"content": [{"type": "text", "text": "after the noise"}]}),
        );
        assert_eq!(pending.wait_tool(None).unwrap().text(), "after the noise");
    }

    #[test]
    fn dropping_the_connection_runs_its_closer_and_fails_the_waiting_call() {
        let (done_tx, done) = mpsc::channel();
        let rig = Rig::with(
            ConnectionConfig::default(),
            false,
            Some(Box::new(move || done_tx.send(()).unwrap())),
        );
        let pending = rig.conn.start_call_tool("t", json!({})).unwrap();
        drop(rig.conn);
        done.recv().unwrap();
        let err = pending.wait(None).unwrap_err();
        assert!(
            matches!(&err, ClientError::Closed { cause } if cause.contains("this side closed")),
            "{err:?}"
        );
    }

    #[test]
    fn every_error_names_a_next_step_or_what_happened() {
        for e in [
            ClientError::Closed { cause: "c".into() },
            ClientError::Timeout {
                method: "tools/call".into(),
                after: Duration::from_secs(2),
            },
            ClientError::Cancelled,
            ClientError::Rpc {
                code: 1,
                message: "m".into(),
            },
            ClientError::BadResult {
                method: "m".into(),
                detail: "d".into(),
            },
            ClientError::UnsupportedVersion { server: "v".into() },
            ClientError::Paging { detail: "d".into() },
        ] {
            assert!(!e.to_string().is_empty() && !e.kind().is_empty());
        }
        assert_eq!(
            ConnectionConfig::default().setup_timeout,
            Duration::from_secs(30)
        );
    }
}
