//! The loopback transport: newline-delimited JSON over a TCP connection to 127.0.0.1, after a handshake line carrying the
//! client's key (ADR 0007 rule 1). Only loopback peers are served; no remote listener exists.
//!
//! ```text
//! client -> {"key":"pmk_..."}\n        server -> {"ok":true,"client":"Pitwall"}\n
//!                                        or {"ok":false,"code":"key"|"handshake"|"busy","error":"..."}\n and close
//! then the same lines as stdio.
//! ```
//!
//! A client says "the key was refused" only for `code` `key`; `handshake` means the key line did not arrive in a usable form
//! and `busy` that every place is taken.

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
/// How many refused connections are drained (their key line read before they close) at once. Not tied to the number of
/// places: a client that reconnects at once after a refusal is refused again before the drain of its previous connection
/// has run on a loaded machine, and with no drain place left its key line is left unread, which resets the connection (CI
/// run 7761). A drain is a thread blocked in a read for at most [`HANDSHAKE_TIMEOUT`]; beyond this bound a refused
/// connection is closed at once.
const DRAIN_PLACES: usize = 16;
/// How long one blocking read waits before the connection looks at the stop flag again. Shutting a socket down wakes a
/// blocked read on Linux and macOS, but not on Windows, where only the system's own connection timeout (about two minutes)
/// would end it; so reads are cut into slices and [`Serving::stop`] is noticed within one.
const STOP_SLICE: Duration = Duration::from_millis(100);
/// The most a refused client's input is read and dropped before its socket closes (ADR 0072 §7, "drain after a refusal").
/// A key line is under 1 KiB; a client that keeps writing beyond this is cut off (and may then see a reset).
const REFUSAL_DRAIN_BYTES: usize = 64 << 10;
/// How long a refusal's drain waits for the client's end once its key line was read (ADR 0072 §7).
const REFUSAL_DRAIN_TIME: Duration = Duration::from_secs(2);

/// The read half of a connection: reads in [`STOP_SLICE`]s, ends (as end of input) once the server stops, and fails with
/// `TimedOut` when its deadline (the handshake's) has passed. A read that a signal interrupted is read again. On Linux a
/// read on a socket with a receive timeout fails with EINTR when a signal handler runs on its thread, even under SA_RESTART
/// (signal(7)); macOS restarts it under SA_RESTART. Passing EINTR on ended connections and turned right keys down in the
/// Linux field case of 2026-11-07; which signal reached the connection's thread there is still to be confirmed.
struct Watched {
    stream: TcpStream,
    stop: Arc<AtomicBool>,
    deadline: Option<Instant>,
    /// The receive timeout set on the socket now: [`STOP_SLICE`], or less when the deadline is nearer.
    slice: Duration,
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
            slice: STOP_SLICE,
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
            // The deadline is checked before every read, an interrupted one included, and a slice never runs past it: a
            // storm of signals cannot stretch it.
            let slice = match self.deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                    left.min(STOP_SLICE)
                }
                None => STOP_SLICE,
            };
            if slice != self.slice {
                self.stream.set_read_timeout(Some(slice))?;
                self.slice = slice;
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
        // Runs once: `stop` and then `Drop` both come here, and a second wake connection would go to a port this listener
        // no longer holds (another process may hold it by then; on Windows a refused loopback connect retries for 2 s).
        let Some(accept) = self.accept.take() else {
            return;
        };
        self.stop.store(true, Ordering::SeqCst);
        // A blocking accept is woken by connecting to it. Only a running accept thread still holds the listener: one that
        // has already ended (a panic) closed it, and a connection would go to a closed port or to whoever took it since.
        if !accept.is_finished() {
            let _ = TcpStream::connect(self.addr);
        }
        let _ = accept.join();
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

/// Writes the refusal line in one write: written piece by piece (as `writeln!` of a JSON value does), a close in between left
/// the client a cut-off line.
fn refuse(stream: &mut impl Write, refusal: Refusal, why: &str) {
    let line = format!(
        "{}\n",
        json!({"ok": false, "code": refusal.code(), "error": why})
    );
    let _ = stream.write_all(line.as_bytes());
}

/// How a refused connection's drain ended (ADR 0072 §2 step 5), in the ADR's words for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrainEnd {
    /// The client ended its side: everything it sent was read, so the close cannot reset.
    Eof,
    /// [`REFUSAL_DRAIN_BYTES`] were read and the client was still writing.
    BytesCap,
    /// The drain's time ran out with the client still connected.
    TimeCap,
    /// The listener stopped.
    Stopped,
    /// The read failed (the client reset the connection).
    Failed(io::ErrorKind),
}

impl DrainEnd {
    fn word(self) -> String {
        match self {
            DrainEnd::Eof => "eof".into(),
            DrainEnd::BytesCap => "bytesCap".into(),
            DrainEnd::TimeCap => "timeCap".into(),
            DrainEnd::Stopped => "stopped".into(),
            DrainEnd::Failed(kind) => format!("failed ({kind:?})"),
        }
    }
}

/// Reads and drops a refused client's input, its buffered part included, until the client's end, [`REFUSAL_DRAIN_BYTES`],
/// `until` or a stop. Closing a socket with received bytes unread resets the connection, and the reset can overtake or
/// discard the refusal (or, being one unacknowledged segment, never reach the client, which then waits for an end that never
/// comes); so a refused connection is half-closed, drained, and only then closed. Returns the bytes read and how it ended.
fn drain(reader: &mut BufReader<Watched>, until: Instant) -> (usize, DrainEnd) {
    reader.get_mut().deadline = Some(until);
    let mut buf = [0u8; 8192];
    let mut read = 0;
    loop {
        let room = REFUSAL_DRAIN_BYTES - read;
        if room == 0 {
            return (read, DrainEnd::BytesCap);
        }
        let take = room.min(buf.len());
        match reader.read(&mut buf[..take]) {
            Ok(0) if reader.get_ref().stop.load(Ordering::SeqCst) => {
                return (read, DrainEnd::Stopped);
            }
            Ok(0) => return (read, DrainEnd::Eof),
            Ok(n) => read += n,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => return (read, DrainEnd::TimeCap),
            Err(e) => return (read, DrainEnd::Failed(e.kind())),
        }
    }
}

/// Logs how a refused connection's drain ended (ER-27: a client that saw a reset is explained by the app's log).
fn log_drain(id: u64, (read, end): (usize, DrainEnd)) {
    let line = format!(
        "paddock_mcp: connection #{id} drained {read} byte(s) after its refusal: drain {}",
        end.word()
    );
    if end == DrainEnd::Eof {
        log::info!("{line}");
    } else {
        log::warn!("{line}; the client may see a reset");
    }
}

/// Reads the key line, waiting at most `timeout` ([`HANDSHAKE_TIMEOUT`] when serving), and answers it. `Err` holds the cause
/// for the log (never the key).
fn handshake(
    stream: &mut impl Write,
    reader: &mut BufReader<Watched>,
    keys: &dyn KeyVerifier,
    timeout: Duration,
) -> Result<String, String> {
    const SEND: &str = "send {\"key\": \"...\"} as the first line";
    reader.get_mut().deadline = Some(Instant::now() + timeout);
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
            format!("no key line within {} s; {SEND}", timeout.as_secs()),
            format!("no key line within {} s", timeout.as_secs()),
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
///
/// A refused connection gives its place back, half-closes and is drained on this thread under a place of the drain pool
/// (`drains`); with none left it closes at once. A paired one gives its place back when it ends; the caller then closes it.
fn serve_connection(
    id: u64,
    server: &Server,
    stream: &TcpStream,
    keys: &dyn KeyVerifier,
    stop: Arc<AtomicBool>,
    place: Place,
    drains: &Arc<AtomicUsize>,
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
    let client = match handshake(&mut writer, &mut reader, keys, HANDSHAKE_TIMEOUT) {
        Ok(client) => client,
        Err(cause) => {
            log::warn!(
                "paddock_mcp: connection #{id} refused after {} ms: {cause}; {} read(s) interrupted by a signal and read again",
                began.elapsed().as_millis(),
                reader.get_ref().interrupted.load(Ordering::SeqCst)
            );
            // The place goes back before the client can see the end; the drain holds a drain place instead.
            let draining = Place::take(drains, DRAIN_PLACES);
            drop(place);
            if draining.is_none() {
                log::warn!(
                    "paddock_mcp: connection #{id}: all {DRAIN_PLACES} drains are busy; closed without reading what the client sent after its key line (the client may see a reset)"
                );
            }
            let _ = stream.shutdown(Shutdown::Write);
            if let Some(draining) = draining {
                log_drain(id, drain(&mut reader, Instant::now() + REFUSAL_DRAIN_TIME));
                drop(draining);
            }
            return;
        }
    };
    log::info!("paddock_mcp: connection #{id} paired with client {client}");
    let ended = serve_lines(server, &Session::new(&client), &mut reader, &mut writer);
    // Given back before the caller closes the connection: a client that has seen its end can connect again at once.
    drop(place);
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

/// Drains a connection refused before its key line was read (busy): the key line has [`HANDSHAKE_TIMEOUT`] to arrive, as in
/// a handshake, and everything up to the client's end is read under the [`drain`] bounds.
fn drain_unread(id: u64, stream: &TcpStream, stop: Arc<AtomicBool>) {
    match stream.try_clone().and_then(|s| Watched::new(s, stop)) {
        Ok(read_half) => log_drain(
            id,
            drain(
                &mut BufReader::new(read_half),
                Instant::now() + HANDSHAKE_TIMEOUT,
            ),
        ),
        Err(e) => log::warn!(
            "paddock_mcp: connection #{id} could not be drained after its refusal: {e} ({:?}); the client may see a reset",
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
    let drains = Arc::new(AtomicUsize::new(0));
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
                // The client sees the refusal and then the end at once, but the socket is closed only once the client's
                // key line is read: a socket closed with unread data is reset, and the reset can reach the client before
                // the refusal (2026-11-07). The drain has its own thread, at most [`DRAIN_PLACES`] at a time, each ended
                // by the key line, the client's end, the handshake timeout or a stop.
                // Logged before the client can see the end of its connection, so the log line is there by the time it does.
                let drain = Place::take(&drains, DRAIN_PLACES);
                if drain.is_none() {
                    log::warn!(
                        "paddock_mcp: connection #{id} from port {peer}: all {DRAIN_PLACES} drains are busy; closed without reading its key line (the client may see a reset)"
                    );
                }
                let _ = stream.shutdown(Shutdown::Write);
                if let Some(draining) = drain {
                    let stream = Arc::new(stream);
                    let kept = Arc::downgrade(&stream);
                    let stop = stop2.clone();
                    let handle = std::thread::spawn(move || {
                        drain_unread(id, &stream, stop);
                        drop(draining);
                        drop(stream);
                    });
                    list.push((kept, handle));
                }
                continue;
            };
            log::info!("paddock_mcp: connection #{id} from port {peer} accepted");
            let stream = Arc::new(stream);
            let kept = Arc::downgrade(&stream);
            let (server, keys, stop, drains) =
                (server.clone(), keys.clone(), stop2.clone(), drains.clone());
            let handle = std::thread::spawn(move || {
                // Gives the place back before the client can see the end of its connection; then the last handle (the read
                // half is already closed) is closed here, which ends it.
                serve_connection(id, &server, &stream, keys.as_ref(), stop, place, &drains);
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

    /// Signal tests share SIGUSR2 and its handler, so they take turns.
    #[cfg(unix)]
    static SIGNAL_TESTS: Mutex<()> = Mutex::new(());

    /// A SIGUSR2 handler with `flags`, put back to the one before when dropped.
    #[cfg(unix)]
    struct Handler {
        before: libc::sigaction,
    }

    #[cfg(unix)]
    static HANDLED: AtomicUsize = AtomicUsize::new(0);

    #[cfg(unix)]
    extern "C" fn on_signal(_: libc::c_int) {
        HANDLED.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(unix)]
    impl Handler {
        fn install(flags: libc::c_int) -> Handler {
            // SAFETY: a handler that only touches an atomic, for a signal nothing else in this test binary uses; the
            // previous action is kept and put back on drop.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
                action.sa_flags = flags;
                libc::sigemptyset(&mut action.sa_mask);
                let mut before: libc::sigaction = std::mem::zeroed();
                assert_eq!(libc::sigaction(libc::SIGUSR2, &action, &mut before), 0);
                Handler { before }
            }
        }
    }

    #[cfg(unix)]
    impl Drop for Handler {
        fn drop(&mut self) {
            // SAFETY: puts back the action read in `install`.
            unsafe { libc::sigaction(libc::SIGUSR2, &self.before, std::ptr::null_mut()) };
        }
    }

    /// Sends SIGUSR2 to `thread` until `done` holds, each once the previous one was handled (conditions, not times). The bound
    /// is only a hang guard.
    #[cfg(unix)]
    fn signal_until(thread: libc::pthread_t, done: impl Fn() -> bool) {
        let base = HANDLED.load(Ordering::SeqCst);
        for n in 1..=10_000_000 {
            if done() {
                return;
            }
            // SAFETY: the caller's thread is not joined yet, so its handle is valid.
            unsafe { libc::pthread_kill(thread, libc::SIGUSR2) };
            while HANDLED.load(Ordering::SeqCst) < base + n && !done() {
                std::thread::yield_now();
            }
        }
    }

    /// A read slice with a handler of `flags` running on the reading thread: the signal is not the end of the input, and the
    /// key line sent afterwards is read.
    #[cfg(unix)]
    fn a_signal_during_a_read_slice_is_survived(flags: libc::c_int) {
        use std::os::unix::thread::JoinHandleExt;
        let _turn = SIGNAL_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _handler = Handler::install(flags);
        let (server_end, mut client) = pair();
        let w = Watched::new(server_end, Arc::new(AtomicBool::new(false))).unwrap();
        let interrupted = w.interrupted.clone();
        let reader = std::thread::spawn(move || {
            let mut r = BufReader::new(w);
            read_line(&mut r, HANDSHAKE_MAX).map_err(|e| e.kind())
        });
        signal_until(reader.as_pthread_t(), || {
            reader.is_finished() || interrupted.load(Ordering::SeqCst) > 0
        });
        client.write_all(b"{\"key\":\"pmk_x\"}\n").unwrap();
        let line = reader.join().unwrap();
        assert!(
            interrupted.load(Ordering::SeqCst) > 0,
            "precondition (ER-33): no signal landed inside a read, so this run did not test the case"
        );
        assert_eq!(
            line,
            Ok(Line::Text("{\"key\":\"pmk_x\"}".into())),
            "a signal during a read is not the end of the input"
        );
    }

    /// The mechanism of the Linux field case of 2026-11-07: a signal handled while a connection's thread waits in a read
    /// slice made the read fail with `Interrupted`, which the handshake took for "no key line" and the serving loop for the
    /// end of the connection. Without SA_RESTART every Unix interrupts the read.
    #[cfg(unix)]
    #[test]
    fn a_signal_that_lands_during_a_read_slice_is_not_the_end_of_the_input() {
        a_signal_during_a_read_slice_is_survived(0);
    }

    /// A storm of signals does not stretch the handshake's deadline: an interrupted read checks it too, so a client that
    /// sends nothing is timed out on time however often the reading thread is interrupted (review of 31f2bb60b, P2).
    #[cfg(unix)]
    #[test]
    fn a_storm_of_signals_does_not_stretch_the_handshake_deadline() {
        use std::os::unix::thread::JoinHandleExt;
        let _turn = SIGNAL_TESTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _handler = Handler::install(0);
        let (server_end, _client) = pair();
        let stop = Arc::new(AtomicBool::new(false));
        let mut w = Watched::new(server_end, stop.clone()).unwrap();
        w.deadline = Some(Instant::now() + STOP_SLICE * 3);
        let reader = std::thread::spawn(move || {
            let mut r = BufReader::new(w);
            read_line(&mut r, HANDSHAKE_MAX).map_err(|e| e.kind())
        });
        // The storm goes on until the read ends, or (only a hang guard) far past the deadline.
        let guard = Instant::now() + Duration::from_secs(5);
        signal_until(reader.as_pthread_t(), || {
            reader.is_finished() || Instant::now() > guard
        });
        let ended_by_itself = reader.is_finished();
        stop.store(true, Ordering::SeqCst);
        let line = reader.join().unwrap();
        assert!(
            ended_by_itself,
            "the read went on past its deadline while signals came"
        );
        assert_eq!(line, Err(io::ErrorKind::TimedOut));
    }

    /// Linux interrupts a read with a receive timeout even under SA_RESTART (signal(7)), which most handlers use.
    #[cfg(target_os = "linux")]
    #[test]
    fn under_sa_restart_linux_still_interrupts_a_read_slice_and_it_is_survived() {
        a_signal_during_a_read_slice_is_survived(libc::SA_RESTART);
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
            assert!(handshake(&mut out, &mut reader, &keys, HANDSHAKE_TIMEOUT).is_err());
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
            handshake(&mut out, &mut reader, &keys, HANDSHAKE_TIMEOUT)
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

    use crate::test_log::{LOGGED, install as capture_logs};

    /// The captured line about connection `id` that holds `text`, if any.
    fn logged(id: u64, text: &str) -> Option<String> {
        let prefix = format!("paddock_mcp: connection #{id} ");
        LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|l| l.starts_with(&prefix) && l.contains(text))
            .cloned()
    }

    /// The id the listener gave the connection whose client end is `client` (from its "accepted" line).
    fn id_of(client: &TcpStream) -> u64 {
        let port = client.local_addr().unwrap().port();
        let needle = format!(" from port {port} accepted");
        let lines = LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let line = lines
            .iter()
            .find(|l| l.ends_with(&needle))
            .unwrap_or_else(|| panic!("no accepted line for port {port}"));
        line["paddock_mcp: connection #".len()..]
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    fn test_server() -> Arc<Server> {
        Arc::new(Server::new(
            "demo",
            "1",
            Arc::new(paddock_jobs::FakeClock::at_wall_ms(5)),
            Arc::new(crate::audit::AuditLog::new(10)),
            Arc::new(paddock_log::Redactor::new()),
        ))
    }

    /// A client that sends `first` and reads every line until the listener ends the connection.
    fn talk_until_the_end(addr: SocketAddr, first: &str) -> (TcpStream, Vec<String>) {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("{first}\n").as_bytes()).unwrap();
        s.shutdown(Shutdown::Write).unwrap();
        let lines = std::io::BufRead::lines(BufReader::new(s.try_clone().unwrap()))
            .map_while(Result::ok)
            .collect();
        (s, lines)
    }

    /// Stopping wakes the accept with one connection to the listener's own address, and only while the listener is still
    /// open. `stop` used to run the shutdown a second time from `Drop`, whose wake connection went to the closed port: on
    /// Windows a refused loopback connect is retried for about 2 s (every listener stop took 2 s there, CI run 7810), and a
    /// process that had meanwhile taken the port got a stranger's connection.
    #[test]
    fn a_stopped_listener_never_connects_to_its_old_port_again() {
        let serving = serve_loopback(
            test_server(),
            Arc::new(crate::keys::KeyStore::new()),
            TcpListener::bind("127.0.0.1:0").unwrap(),
            4,
        )
        .unwrap();
        let addr = serving.addr();
        let mut serving = serving;
        serving.shutdown();
        // Whoever binds the freed port next (here the test) must not be reached by this listener's stop.
        let next_owner =
            TcpListener::bind(addr).expect("the stopped listener's port is free again");
        next_owner.set_nonblocking(true).unwrap();
        drop(serving);
        let reached = next_owner.accept();
        assert!(
            matches!(&reached, Err(e) if e.kind() == io::ErrorKind::WouldBlock),
            "the stop of a stopped listener connected to its old port: {reached:?}"
        );
    }

    /// An accept thread that has already ended (here: it saw the stop flag on a connection, as after a panic or an error it
    /// would have ended too) holds no listener, so the stop must not make its wake connection: it would go to a closed port,
    /// about 2 s on Windows, or to whoever took the port since.
    #[test]
    fn a_listener_whose_accept_already_ended_is_stopped_without_a_connection() {
        let mut serving = serve_loopback(
            test_server(),
            Arc::new(crate::keys::KeyStore::new()),
            TcpListener::bind("127.0.0.1:0").unwrap(),
            4,
        )
        .unwrap();
        let addr = serving.addr();
        serving.stop.store(true, Ordering::SeqCst);
        drop(TcpStream::connect(addr).unwrap());
        paddock_testkit::wait_until("the accept thread ended by itself", || {
            serving.accept.as_ref().is_some_and(JoinHandle::is_finished)
        });
        let next_owner =
            TcpListener::bind(addr).expect("the ended accept thread's port is free again");
        next_owner.set_nonblocking(true).unwrap();
        serving.shutdown();
        let reached = next_owner.accept();
        assert!(
            matches!(&reached, Err(e) if e.kind() == io::ErrorKind::WouldBlock),
            "the stop connected to a port its listener no longer holds: {reached:?}"
        );
    }

    #[test]
    fn every_connection_leaves_a_log_line_that_says_how_it_ended() {
        capture_logs();
        let keys = Arc::new(crate::keys::KeyStore::new());
        let (_, closer) = keys.issue("log-test closer", 1).unwrap();
        let (_, stays) = keys.issue("log-test stays", 1).unwrap();
        let serving = serve_loopback(
            test_server(),
            keys,
            TcpListener::bind("127.0.0.1:0").unwrap(),
            4,
        )
        .unwrap();
        // A wrong key: refused, and the log says it was the key (never the key itself).
        let (wrong, replies) = talk_until_the_end(serving.addr(), r#"{"key":"pmk_wrong"}"#);
        assert!(replies[0].contains("\"code\":\"key\""), "{replies:?}");
        let line = logged(id_of(&wrong), "refused after").unwrap();
        assert!(line.contains("the key is wrong or revoked"), "{line}");
        assert!(!line.contains("pmk_"), "no key material in the log: {line}");
        // A client that closes its side after pairing.
        let (closed, replies) =
            talk_until_the_end(serving.addr(), &json!({"key": closer}).to_string());
        assert!(replies[0].contains("\"ok\":true"), "{replies:?}");
        let id = id_of(&closed);
        assert!(logged(id, "paired with client log-test closer").is_some());
        let line = logged(id, "(log-test closer) ended after").unwrap();
        assert!(line.contains("the client closed its side"), "{line}");
        assert!(line.contains("0 read(s) interrupted"), "{line}");
        // A client still connected when the listener stops.
        let mut open = TcpStream::connect(serving.addr()).unwrap();
        open.write_all(format!("{}\n", json!({"key": stays})).as_bytes())
            .unwrap();
        let mut ok = String::new();
        std::io::BufRead::read_line(&mut BufReader::new(open.try_clone().unwrap()), &mut ok)
            .unwrap();
        assert!(ok.contains("\"ok\":true"), "{ok}");
        let id = id_of(&open);
        serving.stop();
        let line = logged(id, "(log-test stays) ended after").unwrap();
        assert!(line.contains("the listener stopped"), "{line}");
    }

    /// Makes closing `s` reset the connection (SO_LINGER 0), as a client that crashes does.
    #[cfg(unix)]
    fn reset_on_close(s: &TcpStream) {
        use std::os::fd::AsRawFd;
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        // SAFETY: a valid socket and a correctly sized option value.
        let r = unsafe {
            libc::setsockopt(
                s.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                (&raw const linger).cast::<libc::c_void>(),
                libc::socklen_t::try_from(std::mem::size_of::<libc::linger>()).unwrap(),
            )
        };
        assert_eq!(r, 0, "SO_LINGER");
    }

    #[cfg(unix)]
    #[test]
    fn a_connection_reset_by_its_client_is_logged_as_failed_with_the_cause() {
        capture_logs();
        let keys = Arc::new(crate::keys::KeyStore::new());
        let (_, key) = keys.issue("log-test reset", 1).unwrap();
        let serving = serve_loopback(
            test_server(),
            keys,
            TcpListener::bind("127.0.0.1:0").unwrap(),
            4,
        )
        .unwrap();
        let mut s = TcpStream::connect(serving.addr()).unwrap();
        s.write_all(format!("{}\n", json!({"key": key})).as_bytes())
            .unwrap();
        let mut ok = String::new();
        std::io::BufRead::read_line(&mut BufReader::new(s.try_clone().unwrap()), &mut ok).unwrap();
        assert!(ok.contains("\"ok\":true"), "{ok}");
        let id = id_of(&s);
        reset_on_close(&s);
        drop(s);
        // Waits for the condition (the connection's line), not a time; stopping first could end the read before the reset
        // arrives. The deadline is only a hang guard.
        let guard = Instant::now() + Duration::from_secs(10);
        let line = loop {
            if let Some(line) = logged(id, "(log-test reset) ") {
                break line;
            }
            assert!(Instant::now() < guard, "no line for the reset connection");
            std::thread::yield_now();
        };
        serving.stop();
        assert!(line.contains("failed after"), "{line}");
        assert!(line.contains("ConnectionReset"), "{line}");
    }

    #[cfg(unix)]
    #[test]
    fn a_key_line_that_cannot_be_read_is_refused_as_a_handshake_problem() {
        let keys = crate::keys::KeyStore::new();
        let (server_end, client) = pair();
        // Set up the read side first: a socket that was already reset may refuse its options (macOS: EINVAL).
        let mut reader =
            BufReader::new(Watched::new(server_end, Arc::new(AtomicBool::new(false))).unwrap());
        reset_on_close(&client);
        (&client).write_all(b"{").unwrap();
        drop(client);
        let mut out = Vec::new();
        let cause = handshake(&mut out, &mut reader, &keys, HANDSHAKE_TIMEOUT).unwrap_err();
        assert!(cause.contains("reading the key line failed"), "{cause}");
        let refusal: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(refusal["code"], "handshake", "{refusal}");
        assert!(
            refusal["error"]
                .as_str()
                .unwrap()
                .contains("could not read the key line"),
            "{refusal}"
        );
    }

    #[test]
    fn a_client_that_sends_no_key_line_in_time_is_refused_as_a_handshake_problem() {
        let keys = crate::keys::KeyStore::new();
        let (server_end, _client) = pair();
        let mut reader =
            BufReader::new(Watched::new(server_end, Arc::new(AtomicBool::new(false))).unwrap());
        let mut out = Vec::new();
        // A zero timeout ends the wait after the first empty slice.
        let cause = handshake(&mut out, &mut reader, &keys, Duration::ZERO).unwrap_err();
        assert!(cause.contains("no key line within 0 s"), "{cause}");
        let refusal: Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(refusal["code"], "handshake", "{refusal}");
    }

    /// More refused clients than drain places: the extra one is closed at once, and with nothing unread it still reads its
    /// refusal and the end. Silent clients hold the one place and every drain (each waits for its client's key line).
    #[test]
    fn a_refused_client_beyond_the_drains_still_reads_its_refusal() {
        capture_logs();
        let keys = Arc::new(crate::keys::KeyStore::new());
        let (_, key) = keys.issue("holder", 1).unwrap();
        let serving = serve_loopback(
            test_server(),
            keys,
            TcpListener::bind("127.0.0.1:0").unwrap(),
            1,
        )
        .unwrap();
        let mut holder = TcpStream::connect(serving.addr()).unwrap();
        holder
            .write_all(format!("{}\n", json!({"key": key})).as_bytes())
            .unwrap();
        let mut ok = String::new();
        std::io::BufRead::read_line(&mut BufReader::new(holder.try_clone().unwrap()), &mut ok)
            .unwrap();
        assert!(ok.contains("\"ok\":true"), "{ok}");
        let read_refusal = |s: &TcpStream| {
            let lines: Vec<String> =
                std::io::BufRead::lines(BufReader::new(s.try_clone().unwrap()))
                    .map_while(Result::ok)
                    .collect();
            assert_eq!(lines.len(), 1, "the refusal, then the end: {lines:?}");
            assert!(lines[0].contains("\"code\":\"busy\""), "{lines:?}");
        };
        // The first refused clients keep every drain busy: they send nothing and stay connected.
        let drained: Vec<TcpStream> = (0..DRAIN_PLACES)
            .map(|_| {
                let s = TcpStream::connect(serving.addr()).unwrap();
                read_refusal(&s);
                s
            })
            .collect();
        let extra = TcpStream::connect(serving.addr()).unwrap();
        read_refusal(&extra);
        // The log says so (ER-27): a client closed without a drain may see a reset, which its own logs cannot explain.
        let port = format!(" from port {}: ", extra.local_addr().unwrap().port());
        let said = LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|l| l.contains(&port) && l.contains("drains are busy"));
        assert!(
            said,
            "no log line about the connection closed without a drain"
        );
        drop(drained);
        drop(serving);
    }

    /// Regression (CI run 7761, macOS, under load): a client reconnecting at once after its refusal was refused again while
    /// the drain of its previous connection had not yet run, so with the one drain place taken the new connection was closed
    /// with its key line unread, and the client read a reset after the refusal instead of the end. A drain that has not run
    /// yet is a silent refused client here, which holds a drain place the same way.
    #[test]
    fn a_refused_client_that_sends_its_key_while_an_earlier_drain_is_pending_reads_the_refusal_then_the_end()
     {
        let keys = Arc::new(crate::keys::KeyStore::new());
        let (_, key) = keys.issue("holder", 1).unwrap();
        let serving = serve_loopback(
            test_server(),
            keys,
            TcpListener::bind("127.0.0.1:0").unwrap(),
            1,
        )
        .unwrap();
        let hello = format!("{}\n", json!({"key": key}));
        let mut holder = TcpStream::connect(serving.addr()).unwrap();
        holder.write_all(hello.as_bytes()).unwrap();
        let mut ok = String::new();
        std::io::BufRead::read_line(&mut BufReader::new(holder.try_clone().unwrap()), &mut ok)
            .unwrap();
        assert!(ok.contains("\"ok\":true"), "{ok}");
        let pending_drain = TcpStream::connect(serving.addr()).unwrap();
        let mut first = String::new();
        std::io::BufRead::read_line(
            &mut BufReader::new(pending_drain.try_clone().unwrap()),
            &mut first,
        )
        .unwrap();
        assert!(first.contains("\"code\":\"busy\""), "{first}");
        for attempt in 0..20 {
            let mut s = TcpStream::connect(serving.addr()).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            s.write_all(hello.as_bytes()).unwrap();
            let mut reader = BufReader::new(s);
            let mut line = String::new();
            let read = std::io::BufRead::read_line(&mut reader, &mut line);
            assert!(
                read.is_ok(),
                "attempt {attempt}: the refusal was lost: {read:?}"
            );
            assert!(
                line.contains("\"code\":\"busy\""),
                "attempt {attempt}: {line}"
            );
            line.clear();
            let end = std::io::BufRead::read_line(&mut reader, &mut line);
            assert!(
                matches!(end, Ok(0)),
                "attempt {attempt}: then the end, not {end:?}"
            );
        }
        drop(pending_drain);
        drop(serving);
    }

    /// Regression (CI run 7868, macOS, `client-keeps-writing` of the ending harness): a client refused for its key that kept
    /// writing had its bytes left unread when the listener closed the connection, so the close was a reset. A reset is one
    /// unacknowledged segment: the client usually read a cut-off refusal and `ConnectionReset`, and when the reset did not
    /// reach it, nothing more until its own guard ran out ("unanswered", 2 in 2400 local runs under load). Refusing now
    /// half-closes and drains (ADR 0072 §2, I18): the client reads the whole refusal and then the end, and the log says how
    /// the drain ended (the line did not exist before, so this test fails without the drain whatever the race does).
    #[test]
    fn a_refused_client_that_keeps_writing_reads_the_whole_refusal_then_the_end() {
        capture_logs();
        let serving = serve_loopback(
            test_server(),
            Arc::new(crate::keys::KeyStore::new()),
            TcpListener::bind("127.0.0.1:0").unwrap(),
            4,
        )
        .unwrap();
        let mut s = TcpStream::connect(serving.addr()).unwrap();
        // A hang guard only: every read below waits for something that must already have happened.
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        // The key line and, without waiting for the answer, more bytes (a pipelined request), under the drain's byte cap.
        s.write_all(b"{\"key\":\"pmk_wrong\"}\n").unwrap();
        let junk = [0x5au8; 4096];
        for _ in 0..12 {
            if s.write_all(&junk).is_err() {
                break; // The old close resets under a writing client; the read below reports it.
            }
        }
        let mut received = Vec::new();
        let end = s.read_to_end(&mut received);
        // The accepted line is logged before the connection is served, so it is there once anything was read.
        let id = id_of(&s);
        let text = String::from_utf8_lossy(&received);
        assert!(
            end.is_ok(),
            "the end after the refusal, not {end:?} after {text:?} (a reset: the client's bytes were left unread)"
        );
        assert!(
            text.ends_with('\n') && text.lines().count() == 1,
            "one whole line: {text:?}"
        );
        assert!(text.contains("\"code\":\"key\""), "{text}");
        // The client has read the end; it ends its side, which ends the listener's drain.
        s.shutdown(Shutdown::Write).unwrap();
        // Waits for the condition (the drain's line), not a time; the deadline is only a hang guard.
        let guard = Instant::now() + Duration::from_secs(20);
        let line = loop {
            if let Some(line) = logged(id, "drained") {
                break line;
            }
            assert!(
                Instant::now() < guard,
                "no log line for the refused connection's drain"
            );
            std::thread::yield_now();
        };
        assert!(line.contains("drain eof"), "{line}");
        drop(serving);
    }
}
