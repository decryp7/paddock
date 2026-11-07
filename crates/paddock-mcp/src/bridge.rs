//! The stdio side for hosts that start a program (ADR 0007 rule 1: "the host starts the app's mode"). The tools need the running
//! app (its link to the agent), so a host-started process cannot serve them itself: [`run`] reads the app's discovery file,
//! checks that the process it names is the one that started when the file says (rule 53: the file is only a hint, and a key is
//! never sent to a port nobody proved), connects to the loopback listener with the key, and carries lines both ways.

use crate::client::{AGAIN, ConnectError, connect_loopback};
use crate::jsonrpc::{Id, Incoming, parse};
use crate::server::Limits;
use serde::Deserialize;
use serde::de::IgnoredAny;
use std::io::{BufRead, Read, Write};
use std::net::Shutdown;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// Longest line the bridge forwards from the app (a result is already cut by the server's limits).
const MAX_LINE: u64 = 4 * 1024 * 1024;

impl From<ConnectError> for BridgeError {
    fn from(e: ConnectError) -> Self {
        BridgeError(e.to_string())
    }
}

/// Why the bridge could not start, or why it ended before the app answered everything. The text says what to do.
#[derive(Debug)]
pub struct BridgeError(pub String);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BridgeError {}

/// The requests the bridge carried that the app has not answered yet, and whether the host closed its input (not set when
/// only the bridge's write to the app failed: the host is then still connected).
#[derive(Default)]
struct Pending {
    ids: Vec<Id>,
    host_closed: bool,
}

impl Pending {
    fn forget(&mut self, id: &Id) {
        if let Some(at) = self.ids.iter().position(|x| x == id) {
            self.ids.remove(at);
        }
    }
}

fn pending_lock(p: &Mutex<Pending>) -> std::sync::MutexGuard<'_, Pending> {
    p.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The id the server will answer `line` with, read by the server's own parser, so only an id the server accepts is waited
/// for: a line it cannot parse, an id it rejects (1.5, true, an object) or a line over its default size limit is answered
/// with id null, and a notification not at all.
fn request_id(line: &str) -> Option<Id> {
    if line.len() > Limits::default().max_line_bytes {
        return None;
    }
    match parse(line) {
        Ok(Incoming::Request { id, .. }) | Err((Some(id), _)) => Some(id),
        _ => None,
    }
}

/// The id of the request `line` cancels when it is `notifications/cancelled`.
fn cancelled_id(line: &str) -> Option<Id> {
    match parse(line) {
        Ok(Incoming::Notification { method, params }) if method == "notifications/cancelled" => {
            params
                .get("requestId")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        }
        _ => None,
    }
}

/// The two fields that tell a reply (no `method`) from a request the app sends. Only these are read, so a long result is
/// not built into a value.
#[derive(Deserialize)]
struct Probe {
    id: Option<Id>,
    method: Option<IgnoredAny>,
}

/// The id of `line` when it is a reply (a result or an error) with an id.
fn reply_id(line: &str) -> Option<Id> {
    serde_json::from_str::<Probe>(line)
        .ok()
        .filter(|p| p.method.is_none())
        .and_then(|p| p.id)
}

/// Carries the host's lines (`input`) to the app and the app's replies to `output` until either ends. When the app ends the
/// connection before it has answered every request the bridge carried (it restarted, stopped AI access, or failed), each of
/// them gets a JSON-RPC error and the bridge returns an error, so a host never waits for an answer that cannot come and the
/// program never ends with success after answering nothing.
pub fn run(
    discovery: &Path,
    key: &str,
    mut input: impl BufRead + Send + 'static,
    output: &mut (impl Write + Send),
) -> Result<(), BridgeError> {
    let link = connect_loopback(discovery, key)?;
    let app = link.app().to_string();
    let (stream, mut from_app) = link.into_parts();
    let pending = Arc::new(Mutex::new(Pending::default()));
    let writer = stream.clone();
    // Detached: a host that keeps stdin open must not keep the bridge alive once the app has ended the connection.
    {
        let pending = pending.clone();
        std::thread::spawn(move || {
            let mut writer = writer;
            let mut line = String::new();
            loop {
                line.clear();
                match input.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        pending_lock(&pending).host_closed = true;
                        break;
                    }
                    Ok(_) => {
                        let text = line.trim_end_matches(['\r', '\n']);
                        // Noted before it is sent, so an answer can never arrive before its request is known.
                        if let Some(id) = request_id(text) {
                            pending_lock(&pending).ids.push(id);
                        } else if let Some(id) = cancelled_id(text) {
                            // The server never answers a call the host cancelled (MCP), so it is no longer waited for.
                            pending_lock(&pending).forget(&id);
                        }
                        if writer
                            .write_all(text.as_bytes())
                            .and_then(|()| writer.write_all(b"\n"))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            // The host closed its side (or the app's is gone): let the app answer what is pending, then it ends the connection.
            writer.shutdown(Shutdown::Write);
        });
    }
    let mut line = String::new();
    let ended = loop {
        line.clear();
        match from_app.by_ref().take(MAX_LINE).read_line(&mut line) {
            Ok(0) => break None,
            Err(e) => break Some(e),
            Ok(_) => {
                if let Some(id) = reply_id(line.trim_end()) {
                    pending_lock(&pending).forget(&id);
                }
                if output
                    .write_all(line.as_bytes())
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    // The host is gone: nobody is left to answer.
                    stream.shutdown(Shutdown::Both);
                    return Ok(());
                }
            }
        }
    };
    stream.shutdown(Shutdown::Both);
    let (unanswered, host_closed) = {
        let mut p = pending_lock(&pending);
        (std::mem::take(&mut p.ids), p.host_closed)
    };
    if unanswered.is_empty() && host_closed {
        // The host closed its side and every request was answered: the normal end.
        return Ok(());
    }
    let cause = ended.map(|e| format!(" ({e})")).unwrap_or_default();
    for id in &unanswered {
        let reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32603, "message": format!("{app} ended the connection before answering{cause}. {AGAIN}")},
        });
        if writeln!(output, "{reply}")
            .and_then(|()| output.flush())
            .is_err()
        {
            break;
        }
    }
    Err(BridgeError(match unanswered.len() {
        0 => {
            format!("{app} ended the connection{cause} while the host was still connected. {AGAIN}")
        }
        n => format!("{app} ended the connection{cause} with {n} request(s) unanswered. {AGAIN}"),
    }))
}
