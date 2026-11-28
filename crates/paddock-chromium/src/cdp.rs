//! A minimal synchronous DevTools-protocol client: one thread owns the WebSocket, callers send a command and wait
//! for its reply; events go to a bounded log that callers wait on. Sessions and targets a caller watches are marked
//! gone when the browser reports them detached, destroyed or crashed (the user closed the window), whatever the log
//! has dropped since.

use crate::{Error, Result};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;

const MAX_MESSAGE: usize = 32 << 20;
const MAX_EVENTS: usize = 1024;

/// How [`Connection::wait_event_or_lost`] ended.
#[derive(Debug, Clone)]
pub enum Waited {
    /// The event.
    Found(Event),
    /// Events after the mark left the bounded log before the awaited one was seen: it may have been among them.
    Lost,
    /// The time ran out, or the connection ended.
    TimedOut,
}

/// A protocol event.
#[derive(Debug, Clone)]
pub struct Event {
    /// Method name, e.g. `Page.loadEventFired`.
    pub method: String,
    /// The session it belongs to, if any.
    pub session: Option<String>,
    /// Event parameters.
    pub params: Value,
}

#[derive(Default)]
struct Events {
    seq: u64,
    log: VecDeque<(u64, Event)>,
}

struct Inner {
    pending: Mutex<HashMap<u64, Sender<std::result::Result<Value, String>>>>,
    events: Mutex<Events>,
    cv: Condvar,
    closed: AtomicBool,
    /// Watched session and target ids; true once the browser said it is gone.
    watched: Mutex<HashMap<String, bool>>,
}

/// Events that end a page: its session detached, its target destroyed or crashed.
const GONE_EVENTS: &[&str] = &[
    "Target.detachedFromTarget",
    "Target.targetDestroyed",
    "Target.targetCrashed",
];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One WebSocket connection to the browser endpoint. Dropping it ends the I/O thread.
pub struct Connection {
    out: Sender<String>,
    inner: Arc<Inner>,
    next: AtomicU64,
    /// A handle on the same socket, to read its options and to end its blocked read when the connection is dropped.
    socket: TcpStream,
}

impl Connection {
    /// Connects to `ws://127.0.0.1:<port><path>` (loopback only).
    pub fn connect(port: u16, path: &str) -> Result<Connection> {
        let stream = TcpStream::connect(("127.0.0.1", port))
            .map_err(|e| Error::Connection(e.to_string()))?;
        let _ = stream.set_nodelay(true);
        let req = format!("ws://127.0.0.1:{port}{path}")
            .into_client_request()
            .map_err(|e| Error::Connection(e.to_string()))?;
        let mut cfg = WebSocketConfig::default();
        cfg.max_message_size = Some(MAX_MESSAGE);
        cfg.max_frame_size = Some(MAX_MESSAGE);
        let (ws, _) = tungstenite::client::client_with_config(req, stream, Some(cfg))
            .map_err(|e| Error::Connection(e.to_string()))?;
        let inner = Arc::new(Inner {
            pending: Mutex::new(HashMap::new()),
            events: Mutex::new(Events::default()),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
            watched: Mutex::new(HashMap::new()),
        });
        let socket = ws
            .get_ref()
            .try_clone()
            .map_err(|e| Error::Connection(e.to_string()))?;
        let writer = tungstenite::WebSocket::from_raw_socket(
            socket
                .try_clone()
                .map_err(|e| Error::Connection(e.to_string()))?,
            tungstenite::protocol::Role::Client,
            Some(cfg),
        );
        let (out, rx) = channel();
        let (i_read, i_write) = (inner.clone(), inner.clone());
        let (s_read, s_write) = (
            socket
                .try_clone()
                .map_err(|e| Error::Connection(e.to_string()))?,
            socket
                .try_clone()
                .map_err(|e| Error::Connection(e.to_string()))?,
        );
        std::thread::Builder::new()
            .name("cdp-read".into())
            .spawn(move || read_loop(ws, i_read, s_read))
            .map_err(|e| Error::Connection(e.to_string()))?;
        std::thread::Builder::new()
            .name("cdp-write".into())
            .spawn(move || write_loop(writer, rx, i_write, s_write))
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(Connection {
            out,
            inner,
            next: AtomicU64::new(1),
            socket,
        })
    }

    /// Whether the connection ended (the browser closed it, or it was dropped).
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Starts watching a session or target id for its end (see [`Connection::is_gone`]).
    pub fn watch(&self, id: &str) {
        lock(&self.inner.watched)
            .entry(id.to_string())
            .or_insert(false);
    }

    /// Whether the browser reported a watched session or target detached, destroyed or crashed.
    pub fn is_gone(&self, id: &str) -> bool {
        lock(&self.inner.watched).get(id).copied().unwrap_or(false)
    }

    /// Sends a command and waits for its reply.
    pub fn call(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.send(session, method, params)?
            .wait(timeout)
            .unwrap_or(Err(Error::Timeout("a command")))
    }

    /// Sends a command; its reply is waited for with [`Pending::wait`], for as long as the caller judges right.
    pub fn send(&self, session: Option<&str>, method: &str, params: Value) -> Result<Pending<'_>> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        lock(&self.inner.pending).insert(id, tx);
        let pending = Pending { conn: self, id, rx };
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        // The I/O thread ended (it clears what is pending as it goes).
        self.out.send(msg.to_string()).map_err(|_| Error::Closed)?;
        Ok(pending)
    }

    /// The sequence number to pass to [`Connection::wait_event`] so only later events count.
    pub fn mark(&self) -> u64 {
        lock(&self.inner.events).seq
    }

    /// The events after `since` still in the bounded log, oldest first, with their sequence numbers.
    pub fn events_since(&self, since: u64) -> Vec<(u64, Event)> {
        lock(&self.inner.events)
            .log
            .iter()
            .filter(|(s, _)| *s > since)
            .cloned()
            .collect()
    }

    /// [`Connection::wait_event`] that also ends, with [`Waited::Lost`], once events after `since` have left the bounded
    /// log, so the one awaited may be among them: a caller that cannot tell then decides what that means.
    pub fn wait_event_or_lost(
        &self,
        since: u64,
        timeout: Duration,
        pred: impl Fn(&Event) -> bool,
    ) -> Waited {
        let end = Instant::now() + timeout;
        let mut g = lock(&self.inner.events);
        loop {
            if let Some((_, e)) = g.log.iter().find(|(s, e)| *s > since && pred(e)) {
                return Waited::Found(e.clone());
            }
            if g.log.front().is_some_and(|(s, _)| *s > since + 1) {
                return Waited::Lost;
            }
            if self.is_closed() {
                return Waited::TimedOut;
            }
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                return Waited::TimedOut;
            };
            g = self
                .inner
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Waits for an event after `since` that `pred` accepts.
    pub fn wait_event(
        &self,
        since: u64,
        timeout: Duration,
        pred: impl Fn(&Event) -> bool,
    ) -> Option<Event> {
        let end = Instant::now() + timeout;
        let mut g = lock(&self.inner.events);
        loop {
            if let Some((_, e)) = g.log.iter().find(|(s, e)| *s > since && pred(e)) {
                return Some(e.clone());
            }
            if self.is_closed() {
                return None;
            }
            let left = end.checked_duration_since(Instant::now())?;
            g = self
                .inner
                .cv
                .wait_timeout(g, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

/// A command sent with [`Connection::send`] whose reply has not been taken yet. Dropping it forgets the command (a late
/// reply is then ignored).
pub struct Pending<'a> {
    conn: &'a Connection,
    id: u64,
    rx: Receiver<std::result::Result<Value, String>>,
}

impl Pending<'_> {
    /// Waits up to `timeout` for the reply: `None` while there is none yet (the command stays pending), else the reply, or
    /// [`Error::Closed`] once the connection has ended.
    pub fn wait(&self, timeout: Duration) -> Option<Result<Value>> {
        match self.rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Some(Ok(v)),
            Ok(Err(m)) => Some(Err(Error::Protocol(m))),
            Err(RecvTimeoutError::Disconnected) => Some(Err(Error::Closed)),
            Err(RecvTimeoutError::Timeout) if self.conn.is_closed() => Some(Err(Error::Closed)),
            Err(RecvTimeoutError::Timeout) => None,
        }
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        lock(&self.conn.inner.pending).remove(&self.id);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Release);
        // Ends the reader's blocked read at once (and with it the connection).
        let _ = self.socket.shutdown(std::net::Shutdown::Both);
    }
}

type Ws = tungstenite::WebSocket<TcpStream>;

/// The reader: one blocking read at a time, with no receive timeout. Winsock documents that a receive that times out leaves
/// the connection "in an indeterminate state" that "should be closed" (`SO_RCVTIMEO`, learn.microsoft.com, read
/// 2026-11-28), so the connection never polls with one; it is ended by closing the socket ([`Connection`]'s drop,
/// the writer's failure), which wakes this read. Chrome sends no pings, so the reader never has a reply to write.
fn read_loop(mut ws: Ws, inner: Arc<Inner>, socket: TcpStream) {
    let ended = loop {
        match ws.read() {
            Ok(Message::Text(t)) => dispatch(&inner, t.as_str()),
            Ok(Message::Close(_)) => break "the browser closed it".to_string(),
            Ok(_) => {}
            Err(e) => break format!("a read failed: {e}"),
        }
    };
    end(&inner, &socket, &ended);
}

/// The writer: sends what callers queued, in order, until the connection ends or its owner is gone.
fn write_loop(mut ws: Ws, rx: Receiver<String>, inner: Arc<Inner>, socket: TcpStream) {
    while let Ok(m) = rx.recv() {
        if let Err(e) = ws.send(Message::text(m)) {
            end(&inner, &socket, &format!("a write failed: {e}"));
            return;
        }
    }
}

/// Marks the connection ended (once), wakes every waiter and closes the socket, saying why when nobody asked for it.
fn end(inner: &Inner, socket: &TcpStream, why: &str) {
    if !inner.closed.swap(true, Ordering::AcqRel) {
        log::info!("The DevTools connection ended: {why}");
    }
    lock(&inner.pending).clear();
    inner.cv.notify_all();
    let _ = socket.shutdown(std::net::Shutdown::Both);
}

/// Whether an event goes into the bounded log. Network events are only wanted for documents (a failed load's trace,
/// `page::trace`): a page's own requests would flood the log, so the rest is dropped here.
fn kept(method: &str, params: Option<&Value>) -> bool {
    !method.starts_with("Network.")
        || params.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("Document")
}

fn dispatch(inner: &Inner, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return;
    };
    if let Some(id) = v.get("id").and_then(Value::as_u64) {
        if let Some(tx) = lock(&inner.pending).remove(&id) {
            let r = match v.get("error") {
                Some(e) => Err(e
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("error")
                    .to_string()),
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send(r);
        }
    } else if let Some(m) = v.get("method").and_then(Value::as_str) {
        if GONE_EVENTS.contains(&m) {
            mark_gone(inner, v.get("params").unwrap_or(&Value::Null));
        }
        if !kept(m, v.get("params")) {
            return;
        }
        let mut g = lock(&inner.events);
        g.seq += 1;
        let seq = g.seq;
        g.log.push_back((
            seq,
            Event {
                method: m.to_string(),
                session: v
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                params: v.get("params").cloned().unwrap_or(Value::Null),
            },
        ));
        if g.log.len() > MAX_EVENTS {
            g.log.pop_front();
        }
        drop(g);
        inner.cv.notify_all();
    }
}

fn mark_gone(inner: &Inner, params: &Value) {
    let ids: HashSet<&str> = ["sessionId", "targetId"]
        .iter()
        .filter_map(|k| params.get(*k).and_then(Value::as_str))
        .collect();
    let mut w = lock(&inner.watched);
    for id in ids {
        if let Some(gone) = w.get_mut(id) {
            *gone = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DevTools-like WebSocket server on loopback: accepts one client and hands its socket to `serve`.
    fn serve_one(
        serve: impl FnOnce(tungstenite::WebSocket<TcpStream>) + Send + 'static,
    ) -> (u16, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            serve(tungstenite::accept(s).unwrap());
        });
        (port, h)
    }

    /// Microsoft's Winsock documentation for `SO_RCVTIMEO` (read 2026-11-28, learn.microsoft.com, "Socket options"):
    /// "If a blocking receive call times out, the connection is in an indeterminate state and should be closed." The
    /// connection polled its socket with a 20 ms receive timeout and kept using it, so on Windows a poll that raced a
    /// reply could lose bytes and end the connection (`Error::Closed`, `real_browser::a_new_tab_loads_a_page_at_once`,
    /// CI run 9276). The mechanism: no receive timeout is ever set.
    #[test]
    fn the_socket_has_no_receive_timeout() {
        let (port, server) = serve_one(|mut ws| {
            let _ = ws.read();
        });
        let conn = Connection::connect(port, "/").unwrap();
        assert_eq!(conn.socket.read_timeout().unwrap(), None);
        drop(conn);
        server.join().unwrap();
    }

    /// A frame that arrives in pieces with long pauses between them is read whole, and a reply is matched to its command:
    /// the reader waits on the socket as long as it takes (no poll of its own to race the bytes).
    #[test]
    fn a_frame_in_slow_pieces_is_read_whole_and_a_command_gets_its_reply() {
        use std::io::Write;
        let (port, server) = serve_one(|mut ws| {
            let Ok(Message::Text(cmd)) = ws.read() else {
                panic!("the client sends its command first");
            };
            let id = serde_json::from_str::<Value>(cmd.as_str()).unwrap()["id"].clone();
            let event = json!({"method": "Page.loadEventFired", "params": {}, "sessionId": "S1"})
                .to_string();
            let reply = json!({"id": id, "result": {"ok": true}}).to_string();
            // Unmasked text frames (a server's), each cut in three with a pause, far longer than the old 20 ms poll.
            let raw = ws.get_mut();
            for payload in [event, reply] {
                let mut frame = vec![0x81, payload.len() as u8];
                frame.extend_from_slice(payload.as_bytes());
                for part in frame.chunks(frame.len() / 3 + 1) {
                    raw.write_all(part).unwrap();
                    raw.flush().unwrap();
                    std::thread::sleep(Duration::from_millis(80));
                }
            }
            let _ = ws.read();
        });
        let conn = Connection::connect(port, "/").unwrap();
        let mark = conn.mark();
        let reply = conn
            .call(None, "Test.command", json!({}), Duration::from_secs(30))
            .unwrap();
        assert_eq!(reply["ok"], true);
        let seen = conn.wait_event(mark, Duration::from_secs(30), |e| {
            e.method == "Page.loadEventFired" && e.session.as_deref() == Some("S1")
        });
        assert!(seen.is_some(), "the event cut in pieces arrived whole");
        drop(conn);
        server.join().unwrap();
    }

    /// The connection ends when the browser closes it: pending commands fail with `Closed` at once and later ones too.
    #[test]
    fn a_connection_the_browser_closes_ends_what_is_pending() {
        let (port, server) = serve_one(|mut ws| {
            let _ = ws.read();
            let _ = ws.close(None);
            while ws.read().is_ok() {}
        });
        let conn = Connection::connect(port, "/").unwrap();
        let r = conn.call(None, "Test.command", json!({}), Duration::from_secs(30));
        assert!(matches!(r, Err(Error::Closed)), "{r:?}");
        assert!(matches!(
            conn.send(None, "Test.later", json!({})),
            Err(Error::Closed)
        ));
        drop(conn);
        server.join().unwrap();
    }

    fn inner() -> Inner {
        Inner {
            pending: Mutex::new(HashMap::new()),
            events: Mutex::new(Events::default()),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
            watched: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn a_watched_session_or_target_is_gone_once_the_browser_says_so() {
        let i = inner();
        lock(&i.watched).insert("S1".into(), false);
        lock(&i.watched).insert("T1".into(), false);
        dispatch(
            &i,
            r#"{"method":"Target.detachedFromTarget","params":{"sessionId":"OTHER","targetId":"T9"}}"#,
        );
        assert!(!lock(&i.watched)["S1"], "another page closing is not ours");
        dispatch(
            &i,
            r#"{"method":"Target.detachedFromTarget","params":{"sessionId":"S1","targetId":"T1"}}"#,
        );
        assert!(lock(&i.watched)["S1"] && lock(&i.watched)["T1"]);
        assert!(
            !lock(&i.watched).contains_key("T9"),
            "unwatched ids are not kept"
        );
    }

    #[test]
    fn only_documents_network_events_are_logged() {
        let i = inner();
        for t in [
            r#"{"method":"Network.loadingFailed","params":{"type":"Document","canceled":true}}"#,
            r#"{"method":"Network.loadingFailed","params":{"type":"Script"}}"#,
            r#"{"method":"Network.dataReceived","params":{}}"#,
            r#"{"method":"Network.requestWillBeSentExtraInfo"}"#,
            r#"{"method":"Page.frameNavigated","params":{}}"#,
        ] {
            dispatch(&i, t);
        }
        let g = lock(&i.events);
        let kept: Vec<&str> = g.log.iter().map(|(_, e)| e.method.as_str()).collect();
        assert_eq!(kept, ["Network.loadingFailed", "Page.frameNavigated"]);
        assert_eq!(g.log[0].1.params["type"], "Document");
    }

    #[test]
    fn a_destroyed_or_crashed_target_is_gone() {
        for m in ["Target.targetDestroyed", "Target.targetCrashed"] {
            let i = inner();
            lock(&i.watched).insert("T1".into(), false);
            dispatch(
                &i,
                &format!(r#"{{"method":"{m}","params":{{"targetId":"T1"}}}}"#),
            );
            assert!(lock(&i.watched)["T1"], "{m}");
        }
    }
}
