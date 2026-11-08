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
/// Refused clients drained at once. Not tied to `max_connections`: a drain ends only after the refused client's key line is
/// read, which is after the client has seen the refusal and the end, so with one drain place a client that refused again at
/// once found the place still taken, its connection was closed with the key line unread and it was reset (macOS, 2026-11-09).
const MAX_DRAINING: usize = 64;
/// Most bytes read from a refused client before it is closed anyway (a client that sends more than its key line and a first
/// request is not waiting to read the refusal).
const DRAIN_MAX_BYTES: usize = 4096;
/// How long one blocking read waits before the connection looks at the stop flag again. Shutting a socket down wakes a
/// blocked read on Linux and macOS, but not on Windows, where only the system's own connection timeout (about two minutes)
/// would end it; so reads are cut into slices and [`Serving::stop`] is noticed within one.
const STOP_SLICE: Duration = Duration::from_millis(100);

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
    let client = match handshake(&mut writer, &mut reader, keys, HANDSHAKE_TIMEOUT) {
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

/// How the reading of a refused client's unread bytes ended (logged: ER-27).
#[derive(Debug, PartialEq, Eq)]
enum DrainEnd {
    /// The client closed its side: nothing more can arrive.
    ClientClosed,
    /// More than [`DRAIN_MAX_BYTES`] arrived: closed anyway.
    ByteCap,
    /// The client neither sent nor closed within [`HANDSHAKE_TIMEOUT`].
    TimedOut,
    /// The listener stopped.
    Stopped,
    /// Nothing more is queued now, the client is still connected (read on, or close and accept a reset).
    StillOpen,
    /// The read failed.
    Failed(io::ErrorKind),
}

impl DrainEnd {
    fn describe(&self) -> String {
        match self {
            DrainEnd::ClientClosed => "the client closed its side".into(),
            DrainEnd::ByteCap => format!("more than {DRAIN_MAX_BYTES} bytes arrived"),
            DrainEnd::TimedOut => "the client sent nothing more in time".into(),
            DrainEnd::Stopped => "the listener stopped".into(),
            DrainEnd::StillOpen => "the client is still connected".into(),
            DrainEnd::Failed(kind) => format!("the read failed ({kind:?})"),
        }
    }
}

/// Reads what is queued on a refused client's socket without waiting, up to [`DRAIN_MAX_BYTES`]: closing a socket with
/// unread data resets it, and the key line is normally queued already, so this usually costs no thread. Returns the bytes
/// read and why it stopped.
fn drain_queued(stream: &TcpStream) -> (usize, DrainEnd) {
    if let Err(e) = stream.set_nonblocking(true) {
        return (0, DrainEnd::Failed(e.kind()));
    }
    let mut total = 0;
    let mut buf = [0u8; 512];
    let end = loop {
        match (&*stream).read(&mut buf) {
            Ok(0) => break DrainEnd::ClientClosed,
            Ok(n) => {
                total += n;
                if total >= DRAIN_MAX_BYTES {
                    break DrainEnd::ByteCap;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break DrainEnd::StillOpen,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => break DrainEnd::Failed(e.kind()),
        }
    };
    // The flag belongs to the open file, which every handle shares: the serving side reads with timeouts, blocking.
    let _ = stream.set_nonblocking(false);
    (total, end)
}

/// Reads a refused client's bytes until it closes its side (it does so on seeing the refusal and the end), bounded by
/// [`DRAIN_MAX_BYTES`] in all (`already` were read before), [`HANDSHAKE_TIMEOUT`] and `stop`.
fn drain_until_closed(
    stream: &TcpStream,
    stop: Arc<AtomicBool>,
    already: usize,
    timeout: Duration,
) -> (usize, DrainEnd) {
    let stopped = stop.clone();
    let Ok(mut read_half) = stream.try_clone().and_then(|s| Watched::new(s, stop)) else {
        return (already, DrainEnd::Failed(io::ErrorKind::Other));
    };
    read_half.deadline = Some(Instant::now() + timeout);
    let mut total = already;
    let mut buf = [0u8; 512];
    loop {
        match read_half.read(&mut buf) {
            Ok(0) if stopped.load(Ordering::SeqCst) => return (total, DrainEnd::Stopped),
            Ok(0) => return (total, DrainEnd::ClientClosed),
            Ok(n) => {
                total += n;
                if total >= DRAIN_MAX_BYTES {
                    return (total, DrainEnd::ByteCap);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::TimedOut => return (total, DrainEnd::TimedOut),
            Err(e) => return (total, DrainEnd::Failed(e.kind())),
        }
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
    serve_loopback_with(server, keys, listener, max_connections, MAX_DRAINING)
}

/// [`serve_loopback`] with the number of refused clients that may be drained at once (a test makes it small).
fn serve_loopback_with(
    server: Arc<Server>,
    keys: Arc<dyn KeyVerifier>,
    listener: TcpListener,
    max_connections: usize,
    max_draining: usize,
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
                // The client sees the refusal and then the end at once, but a socket closed with unread data is reset, and
                // the reset can reach the client before the refusal (2026-11-07). So what the client sent is read first,
                // without waiting; if it is still connected, a thread (at most `max_draining`) reads on until it closes,
                // the byte cap, the handshake timeout or a stop. With no place or no thread, the socket is closed after
                // the inline read and the log says so.
                let _ = stream.shutdown(Shutdown::Write);
                let (queued, first) = drain_queued(&stream);
                if first != DrainEnd::StillOpen {
                    log::info!(
                        "paddock_mcp: connection #{id} from port {peer} drained {queued} byte(s) inline: {}",
                        first.describe()
                    );
                    continue;
                }
                let stream = Arc::new(stream);
                let started = Place::take(&drains, max_draining)
                    .ok_or_else(|| {
                        format!("{max_draining} refused clients are already being drained")
                    })
                    .and_then(|draining| {
                        let (held, stop) = (stream.clone(), stop2.clone());
                        std::thread::Builder::new()
                            .name("mcp-drain".into())
                            .spawn(move || {
                                let began = Instant::now();
                                let (bytes, end) = drain_until_closed(&held, stop, queued, HANDSHAKE_TIMEOUT);
                                log::info!(
                                    "paddock_mcp: connection #{id} from port {peer} drained {bytes} byte(s) in {} ms: {}",
                                    began.elapsed().as_millis(),
                                    end.describe()
                                );
                                drop(draining);
                                drop(held);
                            })
                            .map_err(|e| format!("no thread to drain with: {e}"))
                    });
                match started {
                    Ok(handle) => list.push((Arc::downgrade(&stream), handle)),
                    Err(why) => log::warn!(
                        "paddock_mcp: connection #{id} from port {peer} closed after an inline drain of {queued} byte(s): {why}; it may see a reset instead of the refusal"
                    ),
                }
                continue;
            };
            log::info!("paddock_mcp: connection #{id} from port {peer} accepted");
            let stream = Arc::new(stream);
            let kept = Arc::downgrade(&stream);
            let (server, keys, stop) = (server.clone(), keys.clone(), stop2.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("mcp-conn-{id}"))
                .spawn(move || {
                    serve_connection(id, &server, &stream, keys.as_ref(), stop);
                    // Give the place back before the client can see the end of its connection, then close the last
                    // handle (the read half is already closed), which ends it.
                    drop(place);
                    drop(stream);
                });
            match spawned {
                Ok(handle) => list.push((kept, handle)),
                // The closure, with the socket and the place, is dropped: the client sees its connection end.
                Err(e) => log::warn!(
                    "paddock_mcp: connection #{id} from port {peer} closed: no thread to serve it with: {e}"
                ),
            }
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

    /// The app log, captured, so a test can check what a connection's lines say (ER-27).
    struct Capture;
    static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            LOGGED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.args().to_string());
        }
        fn flush(&self) {}
    }

    fn capture_logs() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = log::set_logger(&Capture);
            log::set_max_level(log::LevelFilter::Info);
        });
        assert!(log::logger().enabled(&log::Metadata::builder().build()));
        log::logger().flush();
    }

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

    /// A listener with the one place taken by a holder that has been answered.
    fn full_listener(max_draining: usize) -> (Serving, TcpStream, String) {
        capture_logs();
        let keys = Arc::new(crate::keys::KeyStore::new());
        let (_, key) = keys.issue("holder", 1).unwrap();
        let serving = serve_loopback_with(
            test_server(),
            keys,
            TcpListener::bind("127.0.0.1:0").unwrap(),
            1,
            max_draining,
        )
        .unwrap();
        let hello = format!("{}\n", json!({"key": key}));
        let mut holder = TcpStream::connect(serving.addr()).unwrap();
        holder.write_all(hello.as_bytes()).unwrap();
        let mut ok = String::new();
        std::io::BufRead::read_line(&mut BufReader::new(holder.try_clone().unwrap()), &mut ok)
            .unwrap();
        assert!(ok.contains("\"ok\":true"), "{ok}");
        (serving, holder, hello)
    }

    /// The captured line about the connection whose client end is `client` that holds `text`, if any (ids restart with every
    /// listener, ports do not repeat within a test run).
    fn logged_for(client: &TcpStream, text: &str) -> Option<String> {
        let needle = format!(" from port {} ", client.local_addr().unwrap().port());
        LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|l| l.contains(&needle) && l.contains(text))
            .cloned()
    }

    /// Regression (2026-11-09, macOS "Connection reset by peer"): every refused client is drained, whatever other refused
    /// clients are doing. The first refused client sends nothing and stays connected, so it holds a drain place for as long
    /// as it likes (with the places tied to `max_connections`, the one place); the second sends its key line and must
    /// still be drained, or its socket is closed with the key line unread and reset. Judged on the log, not on a socket
    /// read: on Linux the client reads the end even when a reset follows it.
    #[test]
    fn a_silent_refused_client_does_not_stop_the_next_one_from_being_drained() {
        let (serving, holder, hello) = full_listener(MAX_DRAINING);
        let silent = TcpStream::connect(serving.addr()).unwrap();
        let mut talker = TcpStream::connect(serving.addr()).unwrap();
        talker.write_all(hello.as_bytes()).unwrap();
        let lines: Vec<String> =
            std::io::BufRead::lines(BufReader::new(talker.try_clone().unwrap()))
                .map_while(Result::ok)
                .collect();
        assert_eq!(lines.len(), 1, "the refusal, then the end: {lines:?}");
        // Stopping ends the silent client's drain and joins every thread, so every drain has logged its end.
        drop(serving);
        drop((silent, holder));
        let drained =
            logged_for(&talker, "drained").expect("the second refused client was not drained");
        assert!(!drained.contains("closed after"), "{drained}");
    }

    /// With no drain place left, the refused client's queued bytes are still read inline and the log says what happened.
    #[test]
    fn a_refused_client_beyond_the_drains_is_read_inline_and_logged() {
        let (serving, holder, hello) = full_listener(1);
        // The first refused client keeps the one drain busy: it sends nothing and stays connected.
        let silent = TcpStream::connect(serving.addr()).unwrap();
        let mut extra = TcpStream::connect(serving.addr()).unwrap();
        extra.write_all(hello.as_bytes()).unwrap();
        let lines: Vec<String> =
            std::io::BufRead::lines(BufReader::new(extra.try_clone().unwrap()))
                .map_while(Result::ok)
                .collect();
        assert_eq!(lines.len(), 1, "the refusal, then the end: {lines:?}");
        assert!(lines[0].contains("\"code\":\"busy\""), "{lines:?}");
        drop(serving);
        drop((silent, holder));
        let line =
            logged_for(&extra, "closed after an inline drain").expect("no inline-drain line");
        assert!(
            line.contains("1 refused clients are already being drained"),
            "{line}"
        );
    }

    /// A refused client that closes before the refusal is read is drained inline: no thread, and the log line says why.
    #[test]
    fn a_refused_client_that_already_closed_is_drained_inline() {
        let (serving, holder, hello) = full_listener(MAX_DRAINING);
        let mut gone = TcpStream::connect(serving.addr()).unwrap();
        gone.write_all(hello.as_bytes()).unwrap();
        gone.shutdown(Shutdown::Write).unwrap();
        let mut rest = String::new();
        let _ = gone.read_to_string(&mut rest);
        drop(serving);
        drop(holder);
        assert!(logged_for(&gone, "drained").is_some(), "no drain line");
    }

    #[cfg(unix)]
    #[test]
    fn a_drain_of_a_reset_connection_ends_as_failed() {
        // The blocking read waits for the reset; a peek first would take the error and leave end of input (Linux).
        let (server_end, client) = pair();
        reset_on_close(&client);
        drop(client);
        let go = Arc::new(AtomicBool::new(false));
        let (_, end) = drain_until_closed(&server_end, go, 0, HANDSHAKE_TIMEOUT);
        assert!(matches!(end, DrainEnd::Failed(_)), "{end:?}");
    }

    #[test]
    fn a_drain_stops_at_the_byte_cap() {
        let (server_end, mut client) = pair();
        client.write_all(&vec![b'x'; DRAIN_MAX_BYTES + 10]).unwrap();
        // Delivery on loopback is not instant on every OS: wait (peek blocks) until more has arrived, then read what is queued.
        let mut total = 0;
        while total < DRAIN_MAX_BYTES {
            server_end.peek(&mut [0u8; 1]).unwrap();
            let (n, end) = drain_queued(&server_end);
            assert!(
                matches!(end, DrainEnd::StillOpen | DrainEnd::ByteCap),
                "{end:?}"
            );
            total += n;
        }
        let (server_end, client) = pair();
        drop(client);
        server_end.peek(&mut [0u8; 1]).unwrap();
        assert_eq!(drain_queued(&server_end), (0, DrainEnd::ClientClosed));
        // The blocking drain stops at the cap too, and reports a stop and a timeout in words.
        let (server_end, mut client) = pair();
        client.write_all(&vec![b'x'; DRAIN_MAX_BYTES + 10]).unwrap();
        let (n, end) = drain_until_closed(
            &server_end,
            Arc::new(AtomicBool::new(false)),
            0,
            HANDSHAKE_TIMEOUT,
        );
        assert_eq!((end, n >= DRAIN_MAX_BYTES), (DrainEnd::ByteCap, true));
        let (server_end, _client) = pair();
        let stop = Arc::new(AtomicBool::new(true));
        assert_eq!(
            drain_until_closed(&server_end, stop, 0, HANDSHAKE_TIMEOUT),
            (0, DrainEnd::Stopped)
        );
        // A zero timeout ends the wait after the first empty slice.
        let (server_end, _client) = pair();
        let go = Arc::new(AtomicBool::new(false));
        assert_eq!(
            drain_until_closed(&server_end, go, 0, Duration::ZERO),
            (0, DrainEnd::TimedOut)
        );
        for end in [
            DrainEnd::ClientClosed,
            DrainEnd::ByteCap,
            DrainEnd::TimedOut,
            DrainEnd::Stopped,
            DrainEnd::StillOpen,
            DrainEnd::Failed(io::ErrorKind::Other),
        ] {
            assert!(!end.describe().is_empty());
        }
    }
}
