//! The loopback transport: newline-delimited JSON over a TCP connection to 127.0.0.1, after a handshake line carrying the
//! client's key (ADR 0007 rule 1). Only loopback peers are served; no remote listener exists.
//!
//! ```text
//! client -> {"key":"pmk_..."}\n        server -> {"ok":true,"client":"Pitwall"}\n   (or {"ok":false,"error":"..."} and close)
//! then the same lines as stdio.
//! ```

use crate::keys::KeyVerifier;
use crate::lines::{Line, read_line, serve_lines};
use crate::server::{Server, Session};
use serde_json::{Value, json};
use std::io::{self, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Longest handshake line.
const HANDSHAKE_MAX: usize = 512;
/// A client that does not send its key within this is dropped.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long one blocking read waits before the connection looks at the stop flag again. Shutting a socket down wakes a
/// blocked read on Linux and macOS, but not on Windows, where only the system's own connection timeout (about two minutes)
/// would end it; so reads are cut into slices and [`Serving::stop`] is noticed within one.
const STOP_SLICE: Duration = Duration::from_millis(100);

/// The read half of a connection: reads in [`STOP_SLICE`]s, ends (as end of input) once the server stops, and fails with
/// `TimedOut` when its deadline (the handshake's) has passed.
struct Watched {
    stream: TcpStream,
    stop: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

impl Watched {
    fn new(stream: TcpStream, stop: Arc<AtomicBool>) -> io::Result<Watched> {
        stream.set_read_timeout(Some(STOP_SLICE))?;
        Ok(Watched {
            stream,
            stop,
            deadline: None,
        })
    }
}

impl Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(0);
            }
            match self.stream.read(buf) {
                // A slice ended without data (`WouldBlock` on Unix, `TimedOut` on Windows).
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if self.deadline.is_some_and(|d| Instant::now() >= d) {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                }
                other => return other,
            }
        }
    }
}

type Conns = Arc<Mutex<Vec<(TcpStream, JoinHandle<()>)>>>;

/// A running loopback server. Dropping it (or [`Serving::stop`]) closes the listener and every connection and joins every thread.
pub struct Serving {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    conns: Conns,
}

impl Serving {
    /// Where it listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stops serving and waits for every thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A blocking accept is woken by connecting to it.
        let _ = TcpStream::connect(self.addr);
        if let Some(a) = self.accept.take() {
            let _ = a.join();
        }
        let conns = std::mem::take(
            &mut *self
                .conns
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for (stream, handle) in conns {
            let _ = stream.shutdown(Shutdown::Both);
            let _ = handle.join();
        }
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn refuse(stream: &mut TcpStream, why: &str) {
    let _ = writeln!(stream, "{}", json!({"ok": false, "error": why}));
}

fn handshake(
    stream: &mut TcpStream,
    reader: &mut BufReader<Watched>,
    keys: &dyn KeyVerifier,
) -> Option<String> {
    reader.get_mut().deadline = Some(Instant::now() + HANDSHAKE_TIMEOUT);
    let line = match read_line(reader, HANDSHAKE_MAX) {
        Ok(Line::Text(t)) => t,
        _ => {
            refuse(stream, "send {\"key\": \"...\"} as the first line");
            return None;
        }
    };
    let key = serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|v| v.get("key").and_then(Value::as_str).map(str::to_string));
    let Some(client) = key.as_deref().and_then(|k| keys.verify(k)) else {
        refuse(
            stream,
            "the key is missing, wrong or revoked; issue a new one in the app's MCP settings",
        );
        return None;
    };
    reader.get_mut().deadline = None;
    let _ = writeln!(stream, "{}", json!({"ok": true, "client": client}));
    Some(client)
}

/// Refuses any address that is not loopback: there is no remote MCP listener.
fn check_loopback(addr: &SocketAddr) -> io::Result<()> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the MCP listener must be bound to 127.0.0.1 (local only)",
        ))
    }
}

/// Serves `server` on `listener`, which must be bound to a loopback address. At most `max_connections` clients at once.
pub fn serve_loopback(
    server: Arc<Server>,
    keys: Arc<dyn KeyVerifier>,
    listener: TcpListener,
    max_connections: usize,
) -> io::Result<Serving> {
    let addr = listener.local_addr()?;
    check_loopback(&addr)?;
    let stop = Arc::new(AtomicBool::new(false));
    let conns: Conns = Arc::default();
    let (stop2, conns2) = (stop.clone(), conns.clone());
    let accept = std::thread::spawn(move || {
        for stream in listener.incoming() {
            if stop2.load(Ordering::SeqCst) {
                return;
            }
            let Ok(mut stream) = stream else { continue };
            if !stream.peer_addr().is_ok_and(|p| p.ip().is_loopback()) {
                continue;
            }
            let mut list = conns2
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let (done, live): (Vec<_>, Vec<_>) = std::mem::take(&mut *list)
                .into_iter()
                .partition(|(_, h)| h.is_finished());
            *list = live;
            for (_, h) in done {
                let _ = h.join();
            }
            if list.len() >= max_connections {
                refuse(&mut stream, "too many MCP clients are connected");
                continue;
            }
            let Ok(kept) = stream.try_clone() else {
                continue;
            };
            let (server, keys, stop) = (server.clone(), keys.clone(), stop2.clone());
            let handle = std::thread::spawn(move || {
                if let Ok(read_half) = stream.try_clone().and_then(|s| Watched::new(s, stop)) {
                    let mut reader = BufReader::new(read_half);
                    if let Some(client) = handshake(&mut stream, &mut reader, keys.as_ref()) {
                        let _ =
                            serve_lines(&server, &Session::new(&client), &mut reader, &mut stream);
                    }
                }
                // The accept loop keeps a second handle to the socket, so closing this one is not enough to end the connection.
                let _ = stream.shutdown(Shutdown::Both);
            });
            list.push((kept, handle));
        }
    });
    Ok(Serving {
        addr,
        stop,
        accept: Some(accept),
        conns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connected pair on loopback: (the server's end, the client's end).
    fn pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        (l.accept().unwrap().0, client)
    }

    #[test]
    fn a_watched_read_ends_once_the_server_stops_and_fails_after_its_deadline() {
        let (server_end, mut client) = pair();
        let stop = Arc::new(AtomicBool::new(false));
        let mut w = Watched::new(server_end, stop.clone()).unwrap();
        let mut buf = [0u8; 8];
        // Data is read as usual.
        client.write_all(b"hi").unwrap();
        assert_eq!(w.read(&mut buf).unwrap(), 2);
        // A deadline that has passed fails the read after one empty slice.
        w.deadline = Some(Instant::now());
        assert_eq!(
            w.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        // Once the server stops, the read ends as end of input although the client is still connected.
        w.deadline = None;
        stop.store(true, Ordering::SeqCst);
        assert_eq!(w.read(&mut buf).unwrap(), 0);
    }

    #[test]
    fn only_loopback_addresses_are_accepted() {
        for ok in ["127.0.0.1:1", "[::1]:1", "127.5.5.5:9"] {
            assert!(check_loopback(&ok.parse().unwrap()).is_ok(), "{ok}");
        }
        for bad in ["0.0.0.0:1", "192.168.1.5:1", "[::]:1", "8.8.8.8:53"] {
            assert!(
                check_loopback(&bad.parse().unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains("local only"),
                "{bad}"
            );
        }
    }
}
