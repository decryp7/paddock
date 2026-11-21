//! The thin adapter of the stall guard: sockets and threads around the pure rules of [`super::rules`].

use super::rules::{Breaker, Limits, MAX_CONNECTIONS, STOP_MAGIC, Verdict, Watch, is_stop};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How often the loops look at the clock.
const TICK: Duration = Duration::from_millis(50);

/// What can go wrong when the guard starts.
#[derive(Debug)]
pub enum StallGuardError {
    /// A loopback port could not be bound.
    Bind(std::io::Error),
    /// The listener could not be made non-blocking.
    Listener(std::io::Error),
}

impl std::fmt::Display for StallGuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StallGuardError::Bind(e) => write!(f, "cannot listen on 127.0.0.1: {e}"),
            StallGuardError::Listener(e) => write!(f, "cannot set up the listener: {e}"),
        }
    }
}

impl std::error::Error for StallGuardError {}

/// One line of the guard's log (a cut, a refusal, the end): never a request, a header or a key.
pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

/// A guard listening on a free loopback port, in front of `upstream`.
#[derive(Debug)]
pub struct Guard {
    listener: TcpListener,
    upstream: String,
    limits: Limits,
}

/// What every connection thread shares.
struct Shared {
    upstream: String,
    limits: Limits,
    breaker: Mutex<Breaker>,
    stop: AtomicBool,
    active: AtomicUsize,
    log: Log,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A thread that panicked while holding the lock left plain data behind: recover it rather than take the guard down.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Guard {
    /// Binds a free port of 127.0.0.1 for a guard in front of `upstream` (`host:port`).
    ///
    /// # Errors
    /// [`StallGuardError::Bind`] when no port can be bound, [`StallGuardError::Listener`] when it cannot be set up.
    pub fn bind(upstream: impl Into<String>, limits: Limits) -> Result<Guard, StallGuardError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(StallGuardError::Bind)?;
        listener
            .set_nonblocking(true)
            .map_err(StallGuardError::Listener)?;
        Ok(Guard {
            listener,
            upstream: upstream.into(),
            limits,
        })
    }

    /// The port clients connect to.
    pub fn port(&self) -> u16 {
        self.listener.local_addr().map_or(0, |a| a.port())
    }

    /// Serves until a stop request ([`stop`]) or [`Limits::idle_exit`] without a connection. Blocks.
    pub fn serve(self, log: Log) {
        let shared = Arc::new(Shared {
            upstream: self.upstream,
            limits: self.limits,
            breaker: Mutex::new(Breaker::default()),
            stop: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            log,
        });
        let mut last_connection = Instant::now();
        while !shared.stop.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((client, _)) => {
                    last_connection = Instant::now();
                    if shared.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                        shared.active.fetch_sub(1, Ordering::SeqCst);
                        continue;
                    }
                    let shared = shared.clone();
                    std::thread::spawn(move || {
                        handle(client, &shared);
                        shared.active.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_connection.elapsed() > shared.limits.idle_exit {
                        (shared.log)("no connection for a long time: the guard ends");
                        return;
                    }
                    std::thread::sleep(TICK);
                }
                Err(_) => std::thread::sleep(TICK),
            }
        }
    }
}

/// Sends the stop request to the guard on `port`. Returns whether the request was delivered; a missing guard is not an error.
pub fn stop(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_secs(1))
        .is_ok_and(|mut c| c.write_all(STOP_MAGIC).is_ok())
}

fn handle(client: TcpStream, shared: &Shared) {
    let limits = shared.limits;
    // An accepted socket may inherit the listener's non-blocking mode (BSD, macOS).
    let _ = client.set_nonblocking(false);
    // The first bytes tell a stop request from traffic; a client sends its request at once, so a quiet connection is dropped.
    let _ = client.set_read_timeout(Some(limits.stall));
    let mut head = [0u8; 32];
    let Ok(n) = client.peek(&mut head) else {
        return;
    };
    if is_stop(&head[..n]) {
        shared.stop.store(true, Ordering::SeqCst);
        return;
    }
    let now = Instant::now();
    if lock(&shared.breaker).is_open(now) {
        // Fast failure: the request errors at once and the caller goes on without the store.
        return;
    }
    let Some(upstream) = connect(&shared.upstream, limits.connect) else {
        lock(&shared.breaker).trip(now, limits.cooldown);
        (shared.log)(&format!(
            "the store {} did not accept a connection within {} s: refusing connections for {} s",
            shared.upstream,
            limits.connect.as_secs(),
            limits.cooldown.as_secs()
        ));
        return;
    };
    let watch = Mutex::new(Watch::new(Instant::now()));
    let done = AtomicBool::new(false);
    let (Ok(client2), Ok(upstream2)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    std::thread::scope(|s| {
        s.spawn(|| pump(client, upstream2, &watch, &done, shared, true));
        s.spawn(|| pump(upstream, client2, &watch, &done, shared, false));
    });
}

/// Copies `from` to `to` until either ends or the watch says to cut; then closes both sockets.
fn pump(
    mut from: TcpStream,
    mut to: TcpStream,
    watch: &Mutex<Watch>,
    done: &AtomicBool,
    shared: &Shared,
    from_client: bool,
) {
    let limits = shared.limits;
    let _ = from.set_read_timeout(Some(TICK));
    let _ = to.set_write_timeout(Some(limits.stall));
    let mut buf = [0u8; 16 * 1024];
    while !done.load(Ordering::SeqCst) {
        match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                {
                    let mut w = lock(watch);
                    if from_client {
                        w.client_bytes(Instant::now());
                    } else {
                        w.upstream_bytes(Instant::now());
                    }
                }
                if to.write_all(&buf[..n]).is_err() {
                    if from_client {
                        // The store did not take the request within a stall: it hangs.
                        lock(&shared.breaker).trip(Instant::now(), limits.cooldown);
                    }
                    break;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                let now = Instant::now();
                let verdict = lock(watch).verdict(now, limits.stall);
                match verdict {
                    Verdict::Continue => {}
                    Verdict::CutQuiet => {
                        // Only the first of the two pumps to see it says so.
                        if !done.swap(true, Ordering::SeqCst) {
                            (shared.log)(&format!(
                                "a connection to {} was quiet for {} s and is closed",
                                shared.upstream,
                                limits.stall.as_secs()
                            ));
                        }
                        break;
                    }
                    Verdict::CutUnanswered => {
                        if !done.swap(true, Ordering::SeqCst) {
                            lock(&shared.breaker).trip(now, limits.cooldown);
                            (shared.log)(&format!(
                                "the store {} did not answer within {} s: the request is cut, new connections are refused for {} s",
                                shared.upstream,
                                limits.stall.as_secs(),
                                limits.cooldown.as_secs()
                            ));
                        }
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    done.store(true, Ordering::SeqCst);
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}

/// A connection to `endpoint` (`host:port`) within `limit`, trying every address the name resolves to.
fn connect(endpoint: &str, limit: Duration) -> Option<TcpStream> {
    let addrs: Vec<SocketAddr> = endpoint.to_socket_addrs().ok()?.collect();
    addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, limit).ok())
}
