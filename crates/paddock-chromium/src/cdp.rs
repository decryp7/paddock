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
        let _ = ws
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)));
        let inner = Arc::new(Inner {
            pending: Mutex::new(HashMap::new()),
            events: Mutex::new(Events::default()),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
            watched: Mutex::new(HashMap::new()),
        });
        let (out, rx) = channel();
        let i2 = inner.clone();
        std::thread::Builder::new()
            .name("cdp-io".into())
            .spawn(move || io_loop(ws, rx, i2))
            .map_err(|e| Error::Connection(e.to_string()))?;
        Ok(Connection {
            out,
            inner,
            next: AtomicU64::new(1),
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
    }
}

type Ws = tungstenite::WebSocket<TcpStream>;

fn io_loop(mut ws: Ws, rx: Receiver<String>, inner: Arc<Inner>) {
    // Sends what callers queued; a failed send ends the loop like a failed read.
    let flush = |ws: &mut Ws| {
        std::iter::from_fn(|| rx.try_recv().ok()).all(|m| ws.send(Message::text(m)).is_ok())
    };
    while !inner.closed.load(Ordering::Acquire) && flush(&mut ws) {
        match ws.read() {
            Ok(Message::Text(t)) => dispatch(&inner, t.as_str()),
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(e) if idle(&e) => {}
            Err(_) => break,
        }
    }
    inner.closed.store(true, Ordering::Release);
    lock(&inner.pending).clear();
    inner.cv.notify_all();
    let _ = ws.close(None);
}

/// A read that found nothing within the poll time (not an error).
fn idle(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
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
