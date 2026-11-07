//! The stdio side for hosts that start a program (ADR 0007 rule 1: "the host starts the app's mode"). The tools need the running
//! app (its link to the agent), so a host-started process cannot serve them itself: [`run`] reads the app's discovery file,
//! checks that the process it names is the one that started when the file says (rule 53: the file is only a hint, and a key is
//! never sent to a port nobody proved), connects to the loopback listener with the key, and carries lines both ways.

use crate::listener::Discovery;
use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Longest line the bridge forwards from the app (a result is already cut by the server's limits).
const MAX_LINE: u64 = 4 * 1024 * 1024;

/// The next step after the app ended or refused a connection for a reason that is not the key.
const AGAIN: &str =
    "It may be restarting; check that it is open with AI access on, then start this again.";

/// Why the bridge could not start, or why it ended before the app answered everything. The text says what to do.
#[derive(Debug)]
pub struct BridgeError(pub String);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BridgeError {}

fn not_open(app: &str) -> BridgeError {
    BridgeError(format!(
        "{app} isn't open, or AI access is off or has no key. Open the app and turn on Settings > AI access with a key, then start this again."
    ))
}

/// The requests the bridge carried that the app has not answered yet, and whether the host's input has ended.
#[derive(Default)]
struct Pending {
    ids: Vec<Value>,
    input_ended: bool,
}

fn pending_lock(p: &Mutex<Pending>) -> std::sync::MutexGuard<'_, Pending> {
    p.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The two fields that tell a request (`method` and an `id`) from a notification (no `id`) and from a reply (no `method`).
#[derive(Deserialize)]
struct Probe {
    id: Option<Value>,
    method: Option<IgnoredAny>,
}

fn probe(line: &str) -> Option<Probe> {
    serde_json::from_str::<Probe>(line).ok()
}

/// The id of `line` when it is a request that expects an answer.
fn request_id(line: &str) -> Option<Value> {
    probe(line)
        .filter(|p| p.method.is_some())
        .and_then(|p| p.id)
}

/// The id of `line` when it is a reply (a result or an error).
fn reply_id(line: &str) -> Option<Value> {
    probe(line)
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
    let hint = Discovery::read(discovery)
        .filter(Discovery::writer_alive)
        .ok_or_else(|| not_open("The app"))?;
    let app = hint.app.clone();
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], hint.port).into(), Duration::from_secs(5))
            .map_err(|e| {
                log::warn!(
                    "paddock_mcp bridge: connect to port {} failed: {e}",
                    hint.port
                );
                not_open(&app)
            })?;
    let _ = stream.set_nodelay(true);
    writeln!(stream, "{}", serde_json::json!({"key": key}))
        .map_err(|e| BridgeError(format!("Could not reach {app}: {e}")))?;
    let mut from_app = BufReader::new(stream.try_clone().map_err(|e| BridgeError(e.to_string()))?);
    let mut first = String::new();
    let read = from_app
        .by_ref()
        .take(4096)
        .read_line(&mut first)
        .map_err(|e| BridgeError(format!("{app} did not answer the key ({e}). {AGAIN}")))?;
    if read == 0 {
        return Err(BridgeError(format!(
            "{app} closed the connection before answering the key. {AGAIN}"
        )));
    }
    let answer: serde_json::Value = serde_json::from_str(first.trim())
        .map_err(|_| BridgeError(format!("{app} did not answer like an MCP listener.")))?;
    if answer["ok"] != true {
        let why = answer["error"].as_str().unwrap_or("no reason given");
        // Only a key refusal may say so: anything else sent users to issue a new key for nothing (2026-11-07).
        return Err(BridgeError(if answer["code"] == "key" {
            format!("{app} refused the key: {why}.")
        } else {
            format!("{app} turned the connection down: {why}.")
        }));
    }
    let pending = Arc::new(Mutex::new(Pending::default()));
    let writer = stream.try_clone().map_err(|e| BridgeError(e.to_string()))?;
    // Detached: a host that keeps stdin open must not keep the bridge alive once the app has ended the connection.
    {
        let pending = pending.clone();
        std::thread::spawn(move || {
            let mut writer = writer;
            let mut line = String::new();
            loop {
                line.clear();
                match input.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = line.trim_end_matches(['\r', '\n']);
                        // Noted before it is sent, so an answer can never arrive before its request is known.
                        if let Some(id) = request_id(text) {
                            pending_lock(&pending).ids.push(id);
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
            pending_lock(&pending).input_ended = true;
            // The host closed its side: let the app answer what is pending, then it ends the connection.
            let _ = writer.shutdown(Shutdown::Write);
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
                    let mut p = pending_lock(&pending);
                    if let Some(at) = p.ids.iter().position(|x| *x == id) {
                        p.ids.remove(at);
                    }
                }
                if output
                    .write_all(line.as_bytes())
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    // The host is gone: nobody is left to answer.
                    let _ = stream.shutdown(Shutdown::Both);
                    return Ok(());
                }
            }
        }
    };
    let _ = stream.shutdown(Shutdown::Both);
    let (unanswered, input_ended) = {
        let mut p = pending_lock(&pending);
        (std::mem::take(&mut p.ids), p.input_ended)
    };
    if unanswered.is_empty() && input_ended {
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
    Err(BridgeError(format!(
        "{app} ended the connection{cause} with {} request(s) unanswered. {AGAIN}",
        unanswered.len()
    )))
}
