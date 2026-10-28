//! [`HeldPort`]: a loopback port that no server of the test's own answers on, held for as long as the test needs it
//! (ADR 0023, check C39).
//!
//! A test that wants "nothing useful listens there" must not read a port off a listener and drop the listener: the port is
//! free again, and the OS may hand it to another process (on a shared CI runner, a concurrent test's fake server) before
//! the test connects, so the test talks to that server. A [`HeldPort`] keeps the port taken and answers every connection by
//! closing it, so a client sees a connection that ends without an answer, on every OS and at once.
//!
//! No held port can *refuse* a connection portably: a socket bound without listening is refused on Linux and Windows, but
//! macOS drops the connection attempt and the client times out. A test that needs "refused" injects a connector that
//! refuses (the code under test has a connector port) or tests its error mapping with a constructed
//! `ErrorKind::ConnectionRefused`.

use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// How a [`HeldPort`] ends each connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// An orderly close (FIN): the client reads the end of the stream.
    HangUp,
    /// An abortive close (`SO_LINGER` 0, a TCP reset): the client's next read or write fails with "connection reset".
    Reset,
}

/// A loopback port taken for the life of this value, where every connection is accepted and closed at once. Dropping it
/// stops the accept loop, joins its thread and frees the port.
#[derive(Debug)]
pub struct HeldPort {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

impl HeldPort {
    /// A port that hangs up: each connection is closed in order, so the client reads the end of the stream without an answer.
    pub fn hang_up() -> HeldPort {
        HeldPort::start(Ending::HangUp)
    }

    /// A port that resets: each connection is aborted (`SO_LINGER` 0), so the client gets "connection reset" at once.
    pub fn reset() -> HeldPort {
        HeldPort::start(Ending::Reset)
    }

    fn start(ending: Ending) -> HeldPort {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind a loopback port");
        let addr = listener.local_addr().expect("the bound address");
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let acceptor = std::thread::spawn(move || {
            for conn in listener.incoming() {
                if s.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(conn) = conn {
                    end(conn, ending);
                }
            }
        });
        HeldPort {
            addr,
            stop,
            acceptor: Some(acceptor),
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

    /// `http://127.0.0.1:<port>`, for a test that needs a URL.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

fn end(conn: TcpStream, ending: Ending) {
    if ending == Ending::Reset {
        let _ = socket2::SockRef::from(&conn).set_linger(Some(Duration::ZERO));
    }
    drop(conn);
}

impl Drop for HeldPort {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocked `accept` with one connection: the listener is still open, so it lands in the backlog at once.
        if let Some(acceptor) = self.acceptor.take()
            && TcpStream::connect(self.addr).is_ok()
        {
            let _ = acceptor.join();
        }
    }
}
