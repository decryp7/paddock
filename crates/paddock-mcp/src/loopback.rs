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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
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
/// `TimedOut` when its deadline (the handshake's) has passed. A read that a signal interrupted is read again: the app's
/// process has signal handlers (on Linux zbus' async-process handles SIGCHLD), and on Linux a socket with a receive timeout
/// returns EINTR even under SA_RESTART (signal(7)), which once ended connections and turned right keys down (2026-11-07).
struct Watched {
    stream: TcpStream,
    stop: Arc<AtomicBool>,
    deadline: Option<Instant>,
    /// How many reads a signal interrupted (and were read again), for the connection's log line. Shared so a test can wait
    /// until a signal has landed inside a read.
    interrupted: Arc<AtomicU64>,
}

impl Watched {
    fn new(stream: TcpStream, stop: Arc<AtomicBool>) -> io::Result<Watched> {
        stream.set_read_timeout(Some(STOP_SLICE))?;
        Ok(Watched {
            stream,
            stop,
            deadline: None,
            interrupted: Arc::default(),
        })
    }
}

impl Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(0);
            }
            let read = self.stream.read(buf);
            if read
                .as_ref()
                .is_err_and(|e| e.kind() == io::ErrorKind::Interrupted)
            {
                self.interrupted.fetch_add(1, Ordering::SeqCst);
                continue;
            }
            // A slice that ended without data is `WouldBlock` on Unix and `TimedOut` on Windows.
            let slice_ended = read.as_ref().is_err_and(|e| {
                let kind = e.kind();
                kind == io::ErrorKind::WouldBlock || kind == io::ErrorKind::TimedOut
            });
            if !slice_ended {
                return read;
            }
            if self.deadline.is_some_and(|d| Instant::now() >= d) {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
    }
}

/// The live connections, for [`Serving::stop`]. Each connection's thread owns the only strong handle to its socket, so
/// ending the thread closes the connection: it never depends on `shutdown`, which fails (ENOTCONN on macOS) once the
/// client has half-closed. Finished threads are pruned on the next accept.
type Conns = Arc<Mutex<Vec<(Weak<TcpStream>, JoinHandle<()>)>>>;

/// One of the `max_connections` places, held by a connection's thread and given back when it is dropped, which the thread
/// does before it ends the connection: a client that has seen its connection end can connect again at once. (Counting
/// finished threads instead freed the place only some time after the client saw the end, so a prompt reconnect was
/// refused with "too many".)
struct Place(Arc<AtomicUsize>);

impl Place {
    /// Takes a place if fewer than `max` are taken.
    fn take(taken: &Arc<AtomicUsize>, max: usize) -> Option<Place> {
        taken
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < max).then_some(n + 1)
            })
            .ok()
            .map(|_| Place(taken.clone()))
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

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
            // Wakes a blocked read on Linux and macOS; elsewhere the read notices `stop` within a slice. A connection whose
            // thread has already ended has no socket left to shut down.
            if let Some(stream) = stream.upgrade() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            let _ = handle.join();
        }
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Why a connection was turned down, as the refusal line's `code`: a client says "refused the key" only for [`Refusal::Key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// The key is missing, wrong or revoked.
    Key,
    /// The key line never arrived in a usable form (the connection ended, it was too long, it timed out or could not be read).
    Handshake,
    /// Every place is taken.
    Busy,
}

impl Refusal {
    fn code(self) -> &'static str {
        match self {
            Refusal::Key => "key",
            Refusal::Handshake => "handshake",
            Refusal::Busy => "busy",
        }
    }
}

fn refuse(stream: &mut impl Write, refusal: Refusal, why: &str) {
    let _ = writeln!(
        stream,
        "{}",
        json!({"ok": false, "code": refusal.code(), "error": why})
    );
}

/// Reads the key line and answers it. `Err` holds the cause for the log (never the key).
fn handshake(
    stream: &mut impl Write,
    reader: &mut BufReader<Watched>,
    keys: &dyn KeyVerifier,
) -> Result<String, String> {
    const SEND: &str = "send {\"key\": \"...\"} as the first line";
    reader.get_mut().deadline = Some(Instant::now() + HANDSHAKE_TIMEOUT);
    let (why, cause) = match read_line(reader, HANDSHAKE_MAX) {
        Ok(Line::Text(t)) => {
            let key = serde_json::from_str::<Value>(&t)
                .ok()
                .and_then(|v| v.get("key").and_then(Value::as_str).map(str::to_string));
            let Some(client) = key.as_deref().and_then(|k| keys.verify(k)) else {
                refuse(
                    stream,
                    Refusal::Key,
                    "the key is missing, wrong or revoked; issue a new one in the app's MCP settings",
                );
                return Err(if key.is_some() {
                    "the key is wrong or revoked".into()
                } else {
                    "the first line holds no key".into()
                });
            };
            reader.get_mut().deadline = None;
            let _ = writeln!(stream, "{}", json!({"ok": true, "client": client}));
            return Ok(client);
        }
        Ok(Line::Eof) if reader.get_ref().stop.load(Ordering::SeqCst) => (
            "the app is stopping AI access".to_string(),
            "the listener stopped before the key line".to_string(),
        ),
        Ok(Line::Eof) => (
            format!("the connection ended before the key line; {SEND}"),
            "the client sent no key line before closing its side".to_string(),
        ),
        Ok(Line::TooLong) => (
            format!("the first line is longer than {HANDSHAKE_MAX} bytes; {SEND}"),
            format!("the first line was longer than {HANDSHAKE_MAX} bytes"),
        ),
        Err(e) if e.kind() == io::ErrorKind::TimedOut => (
            format!(
                "no key line within {} s; {SEND}",
                HANDSHAKE_TIMEOUT.as_secs()
            ),
            format!("no key line within {} s", HANDSHAKE_TIMEOUT.as_secs()),
        ),
        Err(e) => (
            "the app could not read the key line; try again".to_string(),
            format!("reading the key line failed: {e} ({:?})", e.kind()),
        ),
    };
    refuse(stream, Refusal::Handshake, &why);
    Err(cause)
}

/// Serves one accepted connection: the handshake, then the lines, and one log line for how it ended (ER-27: a client that saw
/// nothing must be explainable from the app's log alone). The log names the client by its key's label, never the key.
fn serve_connection(
    id: u64,
    server: &Server,
    stream: &TcpStream,
    keys: &dyn KeyVerifier,
    stop: Arc<AtomicBool>,
) {
    let began = Instant::now();
    let read_half = match stream
        .try_clone()
        .and_then(|s| Watched::new(s, stop.clone()))
    {
        Ok(r) => r,
        Err(e) => {
            log::warn!(
                "paddock_mcp: connection #{id} could not be served: {e} ({:?})",
                e.kind()
            );
            return;
        }
    };
    let mut reader = BufReader::new(read_half);
    let mut writer: &TcpStream = stream;
    let client = match handshake(&mut writer, &mut reader, keys) {
        Ok(client) => client,
        Err(cause) => {
            log::warn!(
                "paddock_mcp: connection #{id} refused after {} ms: {cause}; {} read(s) interrupted by a signal and read again",
                began.elapsed().as_millis(),
                reader.get_ref().interrupted.load(Ordering::SeqCst)
            );
            return;
        }
    };
    log::info!("paddock_mcp: connection #{id} paired with client {client}");
    let ended = serve_lines(server, &Session::new(&client), &mut reader, &mut writer);
    let interrupted = reader.get_ref().interrupted.load(Ordering::SeqCst);
    let ms = began.elapsed().as_millis();
    match ended {
        Ok(()) => log::info!(
            "paddock_mcp: connection #{id} ({client}) ended after {ms} ms: {}; {interrupted} read(s) interrupted by a signal and read again",
            if stop.load(Ordering::SeqCst) {
                "the listener stopped"
            } else {
                "the client closed its side"
            }
        ),
        Err(e) => log::warn!(
            "paddock_mcp: connection #{id} ({client}) failed after {ms} ms: {e} ({:?}); {interrupted} read(s) interrupted by a signal and read again",
            e.kind()
        ),
    }
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
    let taken = Arc::new(AtomicUsize::new(0));
    let ids = Arc::new(AtomicU64::new(0));
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
            let id = ids.fetch_add(1, Ordering::SeqCst) + 1;
            let peer = stream.peer_addr().map_or(0, |p| p.port());
            let Some(place) = Place::take(&taken, max_connections) else {
                log::warn!(
                    "paddock_mcp: connection #{id} from port {peer} refused: all {max_connections} places are taken"
                );
                refuse(
                    &mut stream,
                    Refusal::Busy,
                    "too many MCP clients are connected",
                );
                continue;
            };
            log::info!("paddock_mcp: connection #{id} from port {peer} accepted");
            let stream = Arc::new(stream);
            let kept = Arc::downgrade(&stream);
            let (server, keys, stop) = (server.clone(), keys.clone(), stop2.clone());
            let handle = std::thread::spawn(move || {
                serve_connection(id, &server, &stream, keys.as_ref(), stop);
                // Give the place back before the client can see the end of its connection, then close the last handle
                // (the read half is already closed), which ends it.
                drop(place);
                drop(stream);
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
        // Empty slices go on until the deadline (here within the second slice), then the read fails.
        w.deadline = Some(Instant::now() + STOP_SLICE + STOP_SLICE / 2);
        assert_eq!(
            w.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        // Once the server stops, the read ends as end of input although the client is still connected.
        w.deadline = None;
        stop.store(true, Ordering::SeqCst);
        assert_eq!(w.read(&mut buf).unwrap(), 0);
    }

    /// The Linux field case of 2026-11-07: a signal that lands while a connection's thread waits in a read slice (the app's
    /// SIGCHLD handler, installed by zbus' async-process, runs whenever a child such as `systemctl` ends, which the app's agent
    /// link does while the agent restarts) made the read fail with `Interrupted`. The handshake took that for "no key line"
    /// and the serving loop for the end of the connection. On Linux a socket with a receive timeout returns EINTR even under
    /// SA_RESTART (signal(7)); the handler here has no SA_RESTART so that macOS shows it too.
    #[cfg(unix)]
    #[test]
    fn a_signal_that_lands_during_a_read_slice_is_not_the_end_of_the_input() {
        use std::os::unix::thread::JoinHandleExt;
        static HANDLED: AtomicUsize = AtomicUsize::new(0);
        extern "C" fn on_signal(_: libc::c_int) {
            HANDLED.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: a handler that only touches an atomic, installed for a signal nothing else in this test binary uses.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            assert_eq!(
                libc::sigaction(libc::SIGUSR2, &action, std::ptr::null_mut()),
                0
            );
        }
        let (server_end, mut client) = pair();
        let w = Watched::new(server_end, Arc::new(AtomicBool::new(false))).unwrap();
        let interrupted = w.interrupted.clone();
        let reader = std::thread::spawn(move || {
            let mut r = BufReader::new(w);
            read_line(&mut r, HANDSHAKE_MAX).map_err(|e| e.kind())
        });
        let thread = reader.as_pthread_t();
        let base = HANDLED.load(Ordering::SeqCst);
        // Signals until one has landed inside a read (the reader counts it) or the read has ended: conditions, not times.
        // Each is sent once the previous one was handled. The bound is only a hang guard.
        for n in 1..=100_000 {
            if reader.is_finished() || interrupted.load(Ordering::SeqCst) > 0 {
                break;
            }
            // SAFETY: the thread is not joined yet, so its handle is valid.
            unsafe { libc::pthread_kill(thread, libc::SIGUSR2) };
            while HANDLED.load(Ordering::SeqCst) < base + n && !reader.is_finished() {
                std::thread::yield_now();
            }
        }
        client.write_all(b"{\"key\":\"pmk_x\"}\n").unwrap();
        let line = reader.join().unwrap();
        let interrupted = interrupted.load(Ordering::SeqCst);
        assert!(
            interrupted > 0,
            "precondition (ER-33): no signal landed inside a read, so this run did not test the case"
        );
        assert_eq!(
            line,
            Ok(Line::Text("{\"key\":\"pmk_x\"}".into())),
            "a signal during a read is not the end of the input"
        );
    }

    /// Field case C: a handshake whose key line could not be read was refused with "send {\"key\": ...} as the first line",
    /// and the bridge reported that as a refused key. A refusal says whether it was the key.
    #[test]
    fn a_handshake_tells_a_lost_key_line_from_a_wrong_key() {
        let keys = crate::keys::KeyStore::new();
        keys.issue("host", 1).unwrap();
        let refusal = |client_sends: &[u8]| -> Value {
            let (server_end, mut client) = pair();
            client.write_all(client_sends).unwrap();
            client.shutdown(Shutdown::Write).unwrap();
            let mut reader =
                BufReader::new(Watched::new(server_end, Arc::new(AtomicBool::new(false))).unwrap());
            let mut out = Vec::new();
            assert!(handshake(&mut out, &mut reader, &keys).is_err());
            serde_json::from_slice(&out).unwrap()
        };
        let lost = refusal(b"");
        assert_eq!(lost["ok"], false);
        assert_eq!(lost["code"], "handshake", "{lost}");
        assert!(
            lost["error"]
                .as_str()
                .unwrap()
                .contains("ended before the key line"),
            "{lost}"
        );
        let long = refusal(&[b'x'; HANDSHAKE_MAX + 1]);
        assert_eq!(long["code"], "handshake", "{long}");
        assert!(
            long["error"].as_str().unwrap().contains("longer than"),
            "{long}"
        );
        let wrong = refusal(b"{\"key\":\"pmk_wrong\"}\n");
        assert_eq!(wrong["code"], "key", "{wrong}");
        let no_key = refusal(b"{\"hello\":1}\n");
        assert_eq!(no_key["code"], "key", "{no_key}");
        // A listener that is stopping says so instead of asking for a key.
        let (server_end, _client) = pair();
        let mut reader =
            BufReader::new(Watched::new(server_end, Arc::new(AtomicBool::new(true))).unwrap());
        let mut out = Vec::new();
        assert!(
            handshake(&mut out, &mut reader, &keys)
                .unwrap_err()
                .contains("listener stopped")
        );
        let stopping: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(stopping["code"], "handshake", "{stopping}");
        assert!(
            stopping["error"].as_str().unwrap().contains("stopping"),
            "{stopping}"
        );
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
