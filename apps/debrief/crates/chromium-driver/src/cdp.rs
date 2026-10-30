//! A minimal synchronous DevTools-protocol client: one thread owns the WebSocket, callers send a command and wait
//! for its reply; events go to a bounded log that callers wait on. Sessions and targets a caller watches are marked
//! gone when the browser reports them detached, destroyed or crashed (the user closed the window), whatever the log
//! has dropped since.

use crate::clock::Budget;
use crate::{Error, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::Message;

const MAX_MESSAGE: usize = 32 << 20;
const MAX_EVENTS: usize = 1024;

#[derive(Debug, Clone)]
pub struct Event {
    pub method: String,
    pub session: Option<String>,
    pub params: Value,
}

#[derive(Default)]
struct Events {
    seq: u64,
    log: VecDeque<(u64, Event)>,
}

/// A command waiting for its reply.
struct Pending {
    tx: Sender<std::result::Result<Value, String>>,
    /// A `Runtime.*` command: its reply shows that the page runs scripts again (no dialog holds it).
    script: bool,
}

struct Inner {
    pending: Mutex<HashMap<u64, Pending>>,
    events: Mutex<Events>,
    cv: Condvar,
    closed: AtomicBool,
    /// Watched session and target ids; true once the browser said it is gone.
    watched: Mutex<HashMap<String, bool>>,
    /// The JavaScript dialog (alert, confirm, prompt, "leave site?") each session's page shows, by its message.
    dialogs: Mutex<HashMap<String, String>>,
    /// The main frame of each session's page (from its last `Page.frameNavigated` without a parent).
    main_frames: Mutex<HashMap<String, String>>,
}

/// Events that end a page: its session detached, its target destroyed or crashed.
const GONE_EVENTS: &[&str] = &["Target.detachedFromTarget", "Target.targetDestroyed", "Target.targetCrashed"];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

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
            watched: Mutex::new(HashMap::new()),
            dialogs: Mutex::new(HashMap::new()),
            main_frames: Mutex::new(HashMap::new()),
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

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Starts watching a session or target id for its end (see [`Connection::is_gone`]).
    pub fn watch(&self, id: &str) {
        lock(&self.inner.watched).entry(id.to_string()).or_insert(false);
    }

    /// Whether the browser reported a watched session or target detached, destroyed or crashed.
    pub fn is_gone(&self, id: &str) -> bool {
        lock(&self.inner.watched).get(id).copied().unwrap_or(false)
    }

    /// The message of the JavaScript dialog the page of `session` shows, if it shows one (the page answers no script
    /// meanwhile). Seen only on a session with the `Page` domain enabled.
    pub fn dialog(&self, session: &str) -> Option<String> {
        lock(&self.inner.dialogs).get(session).cloned()
    }

    /// Sends a command and waits for its reply (`timeout` counts only time this process ran: [`Budget`]).
    pub fn call(&self, session: Option<&str>, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        self.call_unless(session, method, params, timeout, || None)
    }

    /// [`Connection::call`] that gives up early with [`Error::Dialog`] once `blocked` names a reason the reply will
    /// not come (it is asked between short waits).
    pub fn call_unless(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        timeout: Duration,
        blocked: impl Fn() -> Option<String>,
    ) -> Result<Value> {
        if self.is_closed() {
            return Err(Error::Closed);
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        let script = method.starts_with("Runtime.");
        lock(&self.inner.pending).insert(id, Pending { tx, script });
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        if self.out.send(msg.to_string()).is_err() {
            lock(&self.inner.pending).remove(&id);
            return Err(Error::Closed);
        }
        let mut budget = Budget::new(timeout);
        let r = loop {
            match budget.wait(|d| rx.recv_timeout(d)) {
                None => break Err(RecvTimeoutError::Timeout),
                Some(Err(RecvTimeoutError::Timeout)) => {
                    if let Some(why) = blocked() {
                        lock(&self.inner.pending).remove(&id);
                        return Err(Error::Dialog(why));
                    }
                }
                Some(other) => break other,
            }
        };
        match r {
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

    /// Waits for an event after `since` that `pred` accepts (`timeout` counts only time this process ran:
    /// [`Budget`]).
    pub fn wait_event(&self, since: u64, timeout: Duration, pred: impl Fn(&Event) -> bool) -> Option<Event> {
        let mut budget = Budget::new(timeout);
        let mut g = lock(&self.inner.events);
        loop {
            if let Some((_, e)) = g.log.iter().find(|(s, e)| *s > since && pred(e)) {
                return Some(e.clone());
            }
            if self.is_closed() {
                return None;
            }
            let wait = budget.next_wait()?;
            g = self.inner.cv.wait_timeout(g, wait).unwrap_or_else(|e| e.into_inner()).0;
            budget.waited();
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
            // A read interrupted by a signal is not the end of the connection: Linux does not restart a socket read
            // with a timeout after one (signal(7)), and the stop and continue of a stalled process or a paused
            // container is one.
            Err(tungstenite::Error::Io(e)) if is_transient(e.kind()) => {}
            Err(_) => break,
        }
    }
    inner.closed.store(true, Ordering::Release);
    lock(&inner.pending).clear();
    inner.cv.notify_all();
    let _ = ws.close(None);
}

/// A socket read or write that ended without an error of the connection (a timeout, or a signal): try again.
pub(crate) fn is_transient(kind: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind::{Interrupted, TimedOut, WouldBlock};
    matches!(kind, WouldBlock | TimedOut | Interrupted)
}

fn dispatch(inner: &Inner, text: &str) {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return };
    if let Some(id) = v.get("id").and_then(Value::as_u64) {
        if let Some(p) = lock(&inner.pending).remove(&id) {
            // A page that ran a script is not held by a dialog (in case its `javascriptDialogClosed` was missed). Only
            // a script's reply says so: other commands (`Page.*`) are answered while a dialog is open.
            if let (true, Some(s)) = (p.script, v.get("sessionId").and_then(Value::as_str)) {
                lock(&inner.dialogs).remove(s);
            }
            let r = match v.get("error") {
                Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string()),
                None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
            };
            let _ = p.tx.send(r);
        }
    } else if let Some(m) = v.get("method").and_then(Value::as_str) {
        if GONE_EVENTS.contains(&m) {
            mark_gone(inner, v.get("params").unwrap_or(&Value::Null));
        }
        if let Some(s) = v.get("sessionId").and_then(Value::as_str) {
            match m {
                "Page.javascriptDialogOpening" => {
                    let message = v.pointer("/params/message").and_then(Value::as_str).unwrap_or("");
                    let message: String = message.chars().take(200).collect();
                    lock(&inner.dialogs).insert(s.to_string(), message);
                }
                "Page.javascriptDialogClosed" => {
                    lock(&inner.dialogs).remove(s);
                }
                // The page moved on: a new main document has no dialog of the old one. A frame inside the page
                // (one with a parent) navigates while the page's dialog stays.
                "Page.frameNavigated" => {
                    let frame = v.pointer("/params/frame");
                    let main = frame.is_some_and(|f| f.get("parentId").is_none());
                    if let (true, Some(id)) = (main, frame.and_then(|f| f.get("id")).and_then(Value::as_str)) {
                        lock(&inner.main_frames).insert(s.to_string(), id.to_string());
                        lock(&inner.dialogs).remove(s);
                    }
                }
                // A new default script context of the main frame: a new document in it.
                "Runtime.executionContextCreated" => {
                    let aux = v.pointer("/params/context/auxData");
                    let default = aux.and_then(|a| a.get("isDefault")).and_then(Value::as_bool) == Some(true);
                    let frame = aux.and_then(|a| a.get("frameId")).and_then(Value::as_str);
                    let main = frame.is_some() && lock(&inner.main_frames).get(s).map(String::as_str) == frame;
                    if default && main {
                        lock(&inner.dialogs).remove(s);
                    }
                }
                _ => {}
            }
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

fn mark_gone(inner: &Inner, params: &Value) {
    let ids: HashSet<&str> = ["sessionId", "targetId"]
        .iter()
        .filter_map(|k| params.get(*k).and_then(Value::as_str))
        .collect();
    let mut w = lock(&inner.watched);
    let mut dialogs = lock(&inner.dialogs);
    let mut main_frames = lock(&inner.main_frames);
    for id in ids {
        if let Some(gone) = w.get_mut(id) {
            *gone = true;
        }
        dialogs.remove(id);
        main_frames.remove(id);
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
            dialogs: Mutex::new(HashMap::new()),
            main_frames: Mutex::new(HashMap::new()),
        }
    }

    /// A command `id` on the way, as `call_unless` registers it (the receiver is dropped: only the bookkeeping counts).
    fn pending(i: &Inner, id: u64, method: &str) {
        let (tx, _) = channel();
        lock(&i.pending).insert(
            id,
            Pending {
                tx,
                script: method.starts_with("Runtime."),
            },
        );
    }

    #[test]
    fn a_dialog_is_seen_per_session_until_it_closes() {
        let i = inner();
        dispatch(
            &i,
            r#"{"method":"Page.javascriptDialogOpening","sessionId":"S1","params":{"message":"Leave site?","type":"beforeunload"}}"#,
        );
        assert_eq!(lock(&i.dialogs).get("S1").map(String::as_str), Some("Leave site?"));
        assert!(!lock(&i.dialogs).contains_key("S2"));
        dispatch(
            &i,
            r#"{"method":"Page.javascriptDialogClosed","sessionId":"S1","params":{"result":false}}"#,
        );
        assert!(lock(&i.dialogs).is_empty());
    }

    /// Review: a missed `javascriptDialogClosed` must not hold the page for good. Anything that shows the page moved on
    /// clears its dialog: a reply on its session, a navigation, a new script context, or the session's end.
    #[test]
    fn a_dialog_is_forgotten_once_the_page_shows_it_moved_on() {
        let main = r#"{"method":"Page.frameNavigated","sessionId":"S1","params":{"frame":{"id":"F1"}}}"#;
        let open = r#"{"method":"Page.javascriptDialogOpening","sessionId":"S1","params":{"message":"Hi"}}"#;
        for moved_on in [
            r#"{"id":7,"sessionId":"S1","result":{}}"#,
            main,
            r#"{"method":"Runtime.executionContextCreated","sessionId":"S1","params":{"context":{"auxData":{"isDefault":true,"frameId":"F1"}}}}"#,
            r#"{"method":"Target.detachedFromTarget","params":{"sessionId":"S1","targetId":"T1"}}"#,
        ] {
            let i = inner();
            dispatch(&i, main);
            dispatch(&i, open);
            pending(&i, 7, "Runtime.evaluate");
            pending(&i, 8, "Runtime.evaluate");
            dispatch(&i, r#"{"id":8,"sessionId":"S2","result":{}}"#);
            assert!(lock(&i.dialogs).contains_key("S1"), "another session's reply is not ours");
            dispatch(&i, moved_on);
            assert!(lock(&i.dialogs).is_empty(), "{moved_on}");
        }
    }

    /// Review: while the user answers a dialog (during the wait for a check), the page still answers `Page.*` commands
    /// and its frames still navigate; none of that closes the dialog.
    #[test]
    fn a_page_command_reply_or_a_subframe_does_not_forget_an_open_dialog() {
        let i = inner();
        dispatch(
            &i,
            r#"{"method":"Page.frameNavigated","sessionId":"S1","params":{"frame":{"id":"F1"}}}"#,
        );
        dispatch(
            &i,
            r#"{"method":"Page.javascriptDialogOpening","sessionId":"S1","params":{"message":"Hi"}}"#,
        );
        pending(&i, 7, "Page.getNavigationHistory");
        for still_open in [
            r#"{"id":7,"sessionId":"S1","result":{}}"#,
            r#"{"id":99,"sessionId":"S1","result":{}}"#,
            r#"{"method":"Page.frameNavigated","sessionId":"S1","params":{"frame":{"id":"F2","parentId":"F1"}}}"#,
            r#"{"method":"Runtime.executionContextCreated","sessionId":"S1","params":{"context":{"auxData":{"isDefault":true,"frameId":"F2"}}}}"#,
            r#"{"method":"Runtime.executionContextCreated","sessionId":"S1","params":{"context":{"auxData":{"isDefault":false,"frameId":"F1"}}}}"#,
        ] {
            dispatch(&i, still_open);
            assert_eq!(lock(&i.dialogs).get("S1").map(String::as_str), Some("Hi"), "{still_open}");
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
        assert!(!lock(&i.watched).contains_key("T9"), "unwatched ids are not kept");
    }

    #[test]
    fn a_destroyed_or_crashed_target_is_gone() {
        for m in ["Target.targetDestroyed", "Target.targetCrashed"] {
            let i = inner();
            lock(&i.watched).insert("T1".into(), false);
            dispatch(&i, &format!(r#"{{"method":"{m}","params":{{"targetId":"T1"}}}}"#));
            assert!(lock(&i.watched)["T1"], "{m}");
        }
    }
}
