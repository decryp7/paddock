//! A minimal synchronous DevTools-protocol client: one thread owns the WebSocket, callers send a command and wait
//! for its reply; events go to a bounded log that callers wait on.

use crate::{Error, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::Message;

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
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One WebSocket connection to the browser endpoint.
pub struct Connection {
    out: Sender<String>,
    inner: Arc<Inner>,
    next: AtomicU64,
}

impl Connection {
    /// Connects to `ws://127.0.0.1:<port><path>` (loopback only).
    pub fn connect(port: u16, path: &str) -> Result<Connection> {
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| Error::Connection(e.to_string()))?;
        let _ = stream.set_nodelay(true);
        let req = format!("ws://127.0.0.1:{port}{path}")
            .into_client_request()
            .map_err(|e| Error::Connection(e.to_string()))?;
        let mut cfg = WebSocketConfig::default();
        cfg.max_message_size = Some(MAX_MESSAGE);
        cfg.max_frame_size = Some(MAX_MESSAGE);
        let (ws, _) = tungstenite::client::client_with_config(req, stream, Some(cfg)).map_err(|e| Error::Connection(e.to_string()))?;
        let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(20)));
        let inner = Arc::new(Inner {
            pending: Mutex::new(HashMap::new()),
            events: Mutex::new(Events::default()),
            cv: Condvar::new(),
            closed: AtomicBool::new(false),
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

    /// Whether the connection ended.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Sends a command and waits for its reply.
    pub fn call(&self, session: Option<&str>, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        lock(&self.inner.pending).insert(id, tx);
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        if self.out.send(msg.to_string()).is_err() {
            lock(&self.inner.pending).remove(&id);
            return Err(Error::Closed);
        }
        match rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(m)) => Err(Error::Protocol(m)),
            Err(_) => {
                lock(&self.inner.pending).remove(&id);
                if self.is_closed() {
                    Err(Error::Closed)
                } else {
                    Err(Error::Timeout("a command"))
                }
            }
        }
    }

    /// The sequence number to pass to [`Connection::wait_event`] so only later events count.
    pub fn mark(&self) -> u64 {
        lock(&self.inner.events).seq
    }

    /// The events after `since` still in the bounded log, oldest first, with their sequence numbers.
    pub fn events_since(&self, since: u64) -> Vec<(u64, Event)> {
        lock(&self.inner.events).log.iter().filter(|(s, _)| *s > since).cloned().collect()
    }

    /// [`Connection::wait_event`] that also ends, with [`Waited::Lost`], once events after `since` have left the bounded
    /// log, so the one awaited may be among them: a caller that cannot tell then decides what that means.
    pub fn wait_event_or_lost(&self, since: u64, timeout: Duration, pred: impl Fn(&Event) -> bool) -> Waited {
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
            g = self.inner.cv.wait_timeout(g, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Waits for an event after `since` that `pred` accepts.
    pub fn wait_event(&self, since: u64, timeout: Duration, pred: impl Fn(&Event) -> bool) -> Option<Event> {
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
            g = self.inner.cv.wait_timeout(g, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::Release);
    }
}

type Ws = tungstenite::WebSocket<TcpStream>;

fn io_loop(mut ws: Ws, rx: Receiver<String>, inner: Arc<Inner>) {
    'outer: while !inner.closed.load(Ordering::Acquire) {
        while let Ok(m) = rx.try_recv() {
            match ws.send(Message::text(m)) {
                Ok(()) => {}
                // The frame is queued; the next read or send writes it (tungstenite's `send`).
                Err(tungstenite::Error::Io(e)) if is_transient(e.kind()) => {}
                Err(_) => break 'outer,
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => dispatch(&inner, t.as_str()),
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            // A read interrupted by a signal is not the end of the connection: Linux does not restart a socket read with
            // a timeout after one (signal(7)), and the stop and continue of a stalled process is one.
            Err(tungstenite::Error::Io(e)) if is_transient(e.kind()) => {}
            Err(_) => break,
        }
    }
    inner.closed.store(true, Ordering::Release);
    lock(&inner.pending).clear();
    inner.cv.notify_all();
    let _ = ws.close(None);
}

/// A socket read or write that ended without an error of the connection: a timeout, or a signal. EOF and reset errors
/// are not transient.
pub(crate) fn is_transient(kind: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind::{Interrupted, TimedOut, WouldBlock};
    matches!(kind, WouldBlock | TimedOut | Interrupted)
}

/// Whether an event goes into the bounded log. Network events are only wanted for documents (a failed load's trace,
/// `page::trace`): a page's own requests would flood the log, so the rest is dropped here.
fn kept(method: &str, params: Option<&Value>) -> bool {
    !method.starts_with("Network.") || params.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("Document")
}

fn dispatch(inner: &Inner, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    if let Some(id) = v.get("id").and_then(Value::as_u64) {
        if let Some(tx) = lock(&inner.pending).remove(&id) {
            let r = match v.get("error") {
                Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string()),
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = tx.send(r);
        }
    } else if let Some(m) = v.get("method").and_then(Value::as_str) {
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
                session: v.get("sessionId").and_then(Value::as_str).map(str::to_string),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_documents_network_events_are_kept() {
        assert!(kept("Page.frameNavigated", None));
        assert!(kept("Network.loadingFailed", Some(&json!({"type": "Document"}))));
        assert!(!kept("Network.loadingFailed", Some(&json!({"type": "Script"}))));
        assert!(!kept("Network.dataReceived", Some(&json!({}))));
        assert!(!kept("Network.requestWillBeSentExtraInfo", None));
    }
}
