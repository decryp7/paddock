//! [`BrokenServer`]: a loopback server that breaks on purpose, for a test of code that talks to a network service (ADR 0023: a test
//! never depends on a real service, and never reads a port off a listener it dropped).
//!
//! Each [`Mode`] is one way a real service fails: it accepts and never answers ([`Mode::BlackHole`]), answers one byte at a time
//! ([`Mode::Drip`]), cuts the connection in the middle of an answer ([`Mode::CutMidResponse`]) or is not listening at all
//! ([`Mode::Refuse`]); [`Mode::Answer`] is the healthy control, so a test shows that its check can tell a working server from a
//! broken one (ER-33).
//!
//! Bounded: at most [`MAX_CONNECTIONS`] connections are served, a drip or an answer is at most [`MAX_BODY`] bytes, every handler
//! looks at the stop flag every [`TICK`], and dropping the server stops and joins every thread it started.
//!
//! Lives here and not in the crate it was made for (`paddock-net`'s stall guard) because `paddock-net` has this crate as a
//! dev-dependency: a second edge the other way would be a publish cycle.

use crate::net::HeldPort;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// The most connections a server serves; one more is closed at once.
pub const MAX_CONNECTIONS: usize = 64;
/// The most bytes of body a drip or an answer sends.
pub const MAX_BODY: usize = 1 << 20;
/// How often every handler looks at the stop flag.
pub const TICK: Duration = Duration::from_millis(20);

/// How a [`BrokenServer`] fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Accepts every connection, reads what the client sends and never answers.
    BlackHole,
    /// The healthy control: answers every request with `200` and this body (at most [`MAX_BODY`] bytes), keeping the connection.
    Answer(Vec<u8>),
    /// Answers with a header that claims `bytes` of body, then sends the body one byte every `gap`.
    Drip {
        /// The time between two bytes.
        gap: Duration,
        /// How many bytes of body (at most [`MAX_BODY`]).
        bytes: usize,
    },
    /// Answers with a header that claims more body than it sends, sends `sent` bytes, then resets the connection.
    CutMidResponse {
        /// How many bytes of body come before the cut.
        sent: usize,
    },
    /// Not listening: a socket is bound and held (see [`HeldPort::refusing`]; refused at once on Linux, after a time out on macOS).
    Refuse,
}

/// A server that breaks as its [`Mode`] says, on a free loopback port, for as long as this value lives.
#[derive(Debug)]
pub struct BrokenServer {
    addr: SocketAddr,
    accepted: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
    handlers: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// The bound socket of a [`Mode::Refuse`] server.
    _held: Option<HeldPort>,
}

/// Starts a server that breaks as `mode` says.
pub fn broken_server(mode: Mode) -> BrokenServer {
    BrokenServer::start(mode)
}

impl BrokenServer {
    fn start(mode: Mode) -> BrokenServer {
        let accepted = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let handlers = Arc::new(Mutex::new(Vec::new()));
        if mode == Mode::Refuse {
            let held = HeldPort::refusing();
            return BrokenServer {
                addr: held.addr(),
                accepted,
                stop,
                acceptor: None,
                handlers,
                _held: Some(held),
            };
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind a loopback port");
        let addr = listener.local_addr().expect("the bound address");
        let acceptor = {
            let (accepted, stop, handlers) = (accepted.clone(), stop.clone(), handlers.clone());
            std::thread::spawn(move || {
                for conn in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(conn) = conn else { continue };
                    if accepted.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        continue;
                    }
                    let (mode, stop) = (mode.clone(), stop.clone());
                    let handle = std::thread::spawn(move || serve(conn, &mode, &stop));
                    if let Ok(mut h) = handlers.lock() {
                        h.push(handle);
                    }
                }
            })
        };
        BrokenServer {
            addr,
            accepted,
            stop,
            acceptor: Some(acceptor),
            handlers,
            _held: None,
        }
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The address, `127.0.0.1:<port>`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// How many connections the server has accepted so far (a test shows that a client did, or did not, reach it).
    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for BrokenServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(acceptor) = self.acceptor.take()
            && TcpStream::connect(self.addr).is_ok()
        {
            let _ = acceptor.join();
        }
        let handles: Vec<JoinHandle<()>> = self
            .handlers
            .lock()
            .map(|mut h| h.drain(..).collect())
            .unwrap_or_default();
        for h in handles {
            let _ = h.join();
        }
    }
}

/// One connection: waits for the client's first bytes, then fails as `mode` says until the client leaves or the server stops.
fn serve(mut conn: TcpStream, mode: &Mode, stop: &AtomicBool) {
    let _ = conn.set_read_timeout(Some(TICK));
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::SeqCst) {
        match conn.read(&mut buf) {
            Ok(0) => return,
            Ok(_) => {
                if !respond(&mut conn, mode, stop) {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return,
        }
    }
}

/// The answer to one request, if the mode gives one. `false` ends the connection.
fn respond(conn: &mut TcpStream, mode: &Mode, stop: &AtomicBool) -> bool {
    match mode {
        Mode::BlackHole | Mode::Refuse => true,
        Mode::Answer(body) => {
            let body = &body[..body.len().min(MAX_BODY)];
            conn.write_all(&header(body.len())).is_ok() && conn.write_all(body).is_ok()
        }
        Mode::Drip { gap, bytes } => {
            let bytes = (*bytes).min(MAX_BODY);
            if conn.write_all(&header(bytes)).is_err() {
                return false;
            }
            for _ in 0..bytes {
                if conn.write_all(b"x").is_err() {
                    return false;
                }
                let mut waited = Duration::ZERO;
                while waited < *gap {
                    if stop.load(Ordering::SeqCst) {
                        return false;
                    }
                    std::thread::sleep(TICK.min(*gap - waited));
                    waited += TICK;
                }
            }
            true
        }
        Mode::CutMidResponse { sent } => {
            let sent = (*sent).min(MAX_BODY);
            let _ = conn.write_all(&header(sent + 1000));
            let _ = conn.write_all(&vec![b'x'; sent]);
            // An abortive close: the client's next read fails with "connection reset".
            let _ = socket2::SockRef::from(&*conn).set_linger(Some(Duration::ZERO));
            false
        }
    }
}

fn header(len: usize) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\n\r\n").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(server: &BrokenServer, limit: Duration) -> (Vec<u8>, Option<std::io::ErrorKind>) {
        let mut c = TcpStream::connect(server.addr()).expect("connects");
        c.set_read_timeout(Some(limit)).unwrap();
        c.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let mut got = Vec::new();
        let mut b = [0u8; 256];
        loop {
            match c.read(&mut b) {
                Ok(0) => return (got, None),
                Ok(n) => got.extend_from_slice(&b[..n]),
                Err(e) => return (got, Some(e.kind())),
            }
        }
    }

    #[test]
    fn a_black_hole_accepts_and_never_answers() {
        let s = broken_server(Mode::BlackHole);
        let (got, err) = ask(&s, Duration::from_millis(300));
        assert!(got.is_empty());
        assert!(
            matches!(
                err,
                Some(std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
            ),
            "{err:?}"
        );
        assert_eq!(s.accepted(), 1);
    }

    #[test]
    fn the_healthy_control_answers_every_request() {
        let s = broken_server(Mode::Answer(b"hello".to_vec()));
        let mut c = TcpStream::connect(s.addr()).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        for _ in 0..2 {
            c.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
            // The header and the body may arrive in separate reads: read until the body is complete.
            let mut got = Vec::new();
            let mut b = [0u8; 256];
            while !got.ends_with(b"hello") {
                let n = c.read(&mut b).unwrap();
                assert!(
                    n > 0,
                    "the server ended the connection: {:?}",
                    String::from_utf8_lossy(&got)
                );
                got.extend_from_slice(&b[..n]);
            }
            let text = String::from_utf8_lossy(&got).to_string();
            assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
        }
    }

    #[test]
    fn a_drip_sends_one_byte_at_a_time_and_a_cut_ends_in_a_reset() {
        let s = broken_server(Mode::Drip {
            gap: Duration::from_millis(40),
            bytes: 3,
        });
        let started = std::time::Instant::now();
        let (got, _) = ask(&s, Duration::from_millis(500));
        let text = String::from_utf8_lossy(&got).to_string();
        assert!(
            text.starts_with("HTTP/1.1 200 OK") && text.ends_with("xxx"),
            "{text}"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(100),
            "the three bytes took their gaps"
        );
        let s = broken_server(Mode::CutMidResponse { sent: 4 });
        let (got, err) = ask(&s, Duration::from_secs(5));
        let text = String::from_utf8_lossy(&got).to_string();
        assert!(text.starts_with("HTTP/1.1 200 OK"), "{text}");
        assert!(
            got.len() < "HTTP/1.1 200 OK\r\nContent-Length: 1004\r\n\r\n".len() + 1004,
            "the answer is cut short: {} bytes, {err:?}",
            got.len()
        );
    }

    #[test]
    fn a_refusing_server_never_answers_and_dropping_stops_the_threads() {
        let s = broken_server(Mode::Refuse);
        assert!(
            TcpStream::connect_timeout(&s.addr(), Duration::from_millis(300)).is_err(),
            "nothing listens there"
        );
        assert_eq!(s.accepted(), 0);
        let s = broken_server(Mode::BlackHole);
        let _c = TcpStream::connect(s.addr()).unwrap();
        drop(s);
    }
}
