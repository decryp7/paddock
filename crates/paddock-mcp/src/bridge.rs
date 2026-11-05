//! The stdio side for hosts that start a program (ADR 0007 rule 1: "the host starts the app's mode"). The tools need the running
//! app (its link to the agent), so a host-started process cannot serve them itself: [`run`] reads the app's discovery file,
//! checks that the process it names is the one that started when the file says (rule 53: the file is only a hint, and a key is
//! never sent to a port nobody proved), connects to the loopback listener with the key, and carries lines both ways.

use crate::listener::Discovery;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::time::Duration;

/// Longest line the bridge forwards from the app (a result is already cut by the server's limits).
const MAX_LINE: u64 = 4 * 1024 * 1024;

/// Why the bridge could not start. The text says what to do.
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

/// Carries the host's lines (`input`) to the app and the app's replies to `output` until either ends.
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
    from_app
        .by_ref()
        .take(4096)
        .read_line(&mut first)
        .map_err(|e| BridgeError(format!("{app} did not answer the key: {e}")))?;
    let answer: serde_json::Value = serde_json::from_str(first.trim())
        .map_err(|_| BridgeError(format!("{app} did not answer like an MCP listener.")))?;
    if answer["ok"] != true {
        let why = answer["error"].as_str().unwrap_or("the key was refused");
        return Err(BridgeError(format!("{app} refused the key: {why}.")));
    }
    let writer = stream.try_clone().map_err(|e| BridgeError(e.to_string()))?;
    // Detached: a host that keeps stdin open must not keep the bridge alive once the app has ended the connection.
    {
        std::thread::spawn(move || {
            let mut writer = writer;
            let mut line = String::new();
            loop {
                line.clear();
                match input.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if writer
                            .write_all(line.trim_end_matches(['\r', '\n']).as_bytes())
                            .and_then(|()| writer.write_all(b"\n"))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
            // The host closed its side: let the app answer what is pending, then it ends the connection.
            let _ = writer.shutdown(Shutdown::Write);
        });
    }
    {
        let mut line = String::new();
        loop {
            line.clear();
            match from_app.by_ref().take(MAX_LINE).read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if output
                        .write_all(line.as_bytes())
                        .and_then(|()| output.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = stream.shutdown(Shutdown::Both);
    }
    Ok(())
}
