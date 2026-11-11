//! The loopback handshake, once for the family (rule 55): read the app's discovery file, prove that the process it names is
//! the one that started when the file says (rule 53: the file is only a hint, and a key is never sent to a port nobody
//! proved), connect to 127.0.0.1, send the key line and read the one answer line. [`crate::bridge::run`] and Pitwall's host
//! both start here.

use crate::client::Connection;
use crate::client::ConnectionConfig;
use crate::listener::Discovery;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The longest answer to the key line that is read.
const MAX_ANSWER: u64 = 4096;

/// The next step after the app ended or refused a connection for a reason that is not the key.
pub(crate) const AGAIN: &str =
    "It may be restarting; check that it is open with AI access on, then start this again.";

/// How long each step of the handshake may take. A connect or an answer on 127.0.0.1 is not a running job (ER-19): a
/// listener that does not answer in time is not an MCP listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectLimits {
    /// The TCP connect.
    pub connect: Duration,
    /// The wait for the answer to the key line.
    pub answer: Duration,
}

impl Default for ConnectLimits {
    fn default() -> Self {
        ConnectLimits {
            connect: Duration::from_secs(5),
            answer: Duration::from_secs(10),
        }
    }
}

/// Why the handshake failed. `Display` says what to do; [`ConnectError::KeyRefused`] is the only one that may tell the user to
/// look at the key (anything else sent users to issue a new key for nothing, 2026-11-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// No discovery file, a stale one, or nothing accepted the connection: the app is not open, AI access is off, or no key
    /// exists. `app` is the name from the file, or "The app" when there was none.
    NotOpen {
        /// The app's name.
        app: String,
    },
    /// The key line could not be sent, or the app did not answer it in time.
    NoAnswer {
        /// The app's name.
        app: String,
        /// The OS error.
        cause: String,
    },
    /// The app closed the connection before answering the key.
    ClosedEarly {
        /// The app's name.
        app: String,
    },
    /// The answer was not an MCP listener's.
    NotListener {
        /// The app's name.
        app: String,
    },
    /// The listener said the key is wrong or revoked.
    KeyRefused {
        /// The app's name.
        app: String,
        /// The listener's reason.
        why: String,
    },
    /// The listener turned the connection down for another reason (too many clients, a handshake problem).
    Refused {
        /// The app's name.
        app: String,
        /// The listener's reason.
        why: String,
    },
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::NotOpen { app } => write!(
                f,
                "{app} isn't open, or AI access is off or has no key. Open the app and turn on Settings > AI access with a key, then start this again."
            ),
            ConnectError::NoAnswer { app, cause } => {
                write!(f, "{app} did not answer the key ({cause}). {AGAIN}")
            }
            ConnectError::ClosedEarly { app } => {
                write!(
                    f,
                    "{app} closed the connection before answering the key. {AGAIN}"
                )
            }
            ConnectError::NotListener { app } => {
                write!(f, "{app} did not answer like an MCP listener.")
            }
            ConnectError::KeyRefused { app, why } => write!(f, "{app} refused the key: {why}."),
            ConnectError::Refused { app, why } => {
                write!(f, "{app} turned the connection down: {why}.")
            }
        }
    }
}

impl std::error::Error for ConnectError {}

/// One TCP connection that several owners read, write and end: `&TcpStream` reads and writes, so no duplicate handle (and no
/// failure to make one) is needed.
#[derive(Debug, Clone)]
pub struct SharedStream(Arc<TcpStream>);

impl SharedStream {
    /// Ends one or both directions of the connection.
    pub fn shutdown(&self, how: Shutdown) {
        if let Err(e) = self.0.shutdown(how) {
            // The peer would wait for more input and never end the connection (ER-27: say so).
            log::warn!("paddock_mcp: could not end {how:?} of the connection: {e}");
        }
    }
}

impl Read for SharedStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for SharedStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        (&*self.0).flush()
    }
}

/// A connection past the key handshake: the app has accepted the key and the next line is the protocol's.
#[derive(Debug)]
pub struct LoopbackLink {
    stream: SharedStream,
    reader: BufReader<SharedStream>,
    app: String,
    version: String,
}

impl LoopbackLink {
    /// The app's name, from its discovery file.
    pub fn app(&self) -> &str {
        &self.app
    }

    /// The app's version, from its discovery file.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The socket and a buffered reader of it (which may already hold bytes past the answer), for a caller that carries lines
    /// itself, as the stdio bridge does.
    pub fn into_parts(self) -> (SharedStream, BufReader<SharedStream>) {
        (self.stream, self.reader)
    }

    /// A [`Connection`] over this link; dropping it ends the connection.
    pub fn into_connection(self, config: ConnectionConfig) -> Connection {
        let stream = self.stream;
        Connection::new(
            Box::new(self.reader),
            Box::new(stream.clone()),
            Some(Box::new(move || stream.shutdown(Shutdown::Both))),
            config,
        )
    }
}

/// How often a wait on the app looks at whether its process still runs. A process that was killed leaves a socket that may
/// still complete a connection but never answers it; the wait ends within this, not at the end of its limit.
const OWNER_LOOK: Duration = Duration::from_millis(100);

/// Why a wait on the app ended without an answer.
enum Waited {
    /// The process the discovery file names is no longer the one that started when it says (rule 53).
    OwnerGone,
    /// The socket failed or the limit passed.
    Failed(std::io::Error),
}

impl From<std::io::Error> for Waited {
    fn from(e: std::io::Error) -> Self {
        Waited::Failed(e)
    }
}

/// Connects to the app's port in short tries, looking at its process between them, for at most `limit` (a live but slow app is
/// still waited for). Windows retries a refused loopback connection for about 2 s, so a dead listener can cost that long per try
/// there; the look between tries is what ends the wait when the owner is gone.
fn connect_watching(hint: &Discovery, limit: Duration) -> Result<TcpStream, Waited> {
    let addr = ([127, 0, 0, 1], hint.port).into();
    let end = Instant::now() + limit;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match TcpStream::connect_timeout(&addr, left.min(OWNER_LOOK).max(Duration::from_millis(1)))
        {
            Ok(s) => return Ok(s),
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                if !hint.writer_alive() {
                    return Err(Waited::OwnerGone);
                }
                if left.is_zero() {
                    return Err(Waited::Failed(e));
                }
            }
            Err(e) => return Err(Waited::Failed(e)),
        }
    }
}

/// Reads the answer line into `into` (at most [`MAX_ANSWER`] bytes) with one blocking read bounded by `limit`, while a watcher
/// thread looks at the app's process every [`OWNER_LOOK`] and, when it is gone, ends the connection so the read returns at once.
/// One read, not slices of a read timeout: Windows documents the socket as indeterminate after a receive timeout. A read that
/// ends in EOF or an error because the owner died (the OS closes its sockets) is the app being gone, not a closed connection.
fn read_answer(
    stream: &SharedStream,
    reader: &mut BufReader<SharedStream>,
    into: &mut Vec<u8>,
    hint: &Discovery,
    limit: Duration,
) -> Result<(), Waited> {
    stream.0.set_read_timeout(Some(limit))?;
    let gone = Arc::new(AtomicBool::new(false));
    let (stop, tick) = std::sync::mpsc::channel::<()>();
    let watcher = {
        let (stream, hint, gone) = (stream.clone(), hint.clone(), gone.clone());
        std::thread::spawn(move || {
            // The sender is dropped when the read is over: that wakes the tick and ends the watcher.
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                tick.recv_timeout(OWNER_LOOK)
            {
                if !hint.writer_alive() {
                    gone.store(true, Ordering::SeqCst);
                    stream.shutdown(Shutdown::Both);
                    break;
                }
            }
        })
    };
    let read = reader.by_ref().take(MAX_ANSWER).read_until(b'\n', into);
    drop(stop);
    let _ = watcher.join();
    let ended = !matches!(read, Ok(n) if n > 0);
    if gone.load(Ordering::SeqCst) || (ended && !hint.writer_alive()) {
        return Err(Waited::OwnerGone);
    }
    read.map(|_| ()).map_err(Waited::Failed)
}

/// Connects with the default limits; see [`connect_loopback_with`].
///
/// # Errors
/// A [`ConnectError`] whose text says what to do.
pub fn connect_loopback(discovery: &Path, key: &str) -> Result<LoopbackLink, ConnectError> {
    connect_loopback_with(discovery, key, ConnectLimits::default())
}

/// Reads the discovery file at `discovery`, checks that its writer is the running process, connects to the port on 127.0.0.1,
/// sends `key` and reads the answer. The key is never logged.
///
/// # Errors
/// A [`ConnectError`] whose text says what to do.
pub fn connect_loopback_with(
    discovery: &Path,
    key: &str,
    limits: ConnectLimits,
) -> Result<LoopbackLink, ConnectError> {
    let started = Instant::now();
    let hint = Discovery::read(discovery)
        .filter(Discovery::writer_alive)
        .ok_or_else(|| {
            log::info!(
                "paddock_mcp client: no live discovery file at {}",
                discovery.display()
            );
            ConnectError::NotOpen {
                app: "The app".into(),
            }
        })?;
    let app = hint.app.clone();
    let not_open = |why: &str| {
        log::info!(
            "paddock_mcp client: {app} is gone ({why}), after {} ms",
            started.elapsed().as_millis()
        );
        ConnectError::NotOpen { app: app.clone() }
    };
    let tcp = connect_watching(&hint, limits.connect).map_err(|e| match e {
        Waited::OwnerGone => not_open("its process ended while connecting"),
        Waited::Failed(e) => {
            log::warn!(
                "paddock_mcp client: connect to {app} on port {} failed: {e}",
                hint.port
            );
            ConnectError::NotOpen { app: app.clone() }
        }
    })?;
    let _ = tcp.set_nodelay(true);
    let stream = SharedStream(Arc::new(tcp));
    let mut reader = BufReader::new(stream.clone());
    // The key line and its answer are bounded in time and size; the protocol that follows has its own limits.
    let mut first = Vec::new();
    let read = (|| {
        writeln!(stream.clone(), "{}", serde_json::json!({"key": key}))?;
        read_answer(&stream, &mut reader, &mut first, &hint, limits.answer)?;
        stream.0.set_read_timeout(None)?;
        Ok::<usize, Waited>(first.len())
    })()
    .map_err(|e| match e {
        Waited::OwnerGone => not_open("its process ended before it answered the key"),
        // A write to a socket the dying owner closed fails like any other: look at the owner before blaming the app's answer.
        Waited::Failed(_) if !hint.writer_alive() => {
            not_open("its process ended while the key was sent")
        }
        Waited::Failed(e) => {
            log::warn!("paddock_mcp client: {app} gave no answer to the key: {e}");
            ConnectError::NoAnswer {
                app: app.clone(),
                cause: e.to_string(),
            }
        }
    })?;
    let first = String::from_utf8_lossy(&first).into_owned();
    if read == 0 {
        log::warn!("paddock_mcp client: {app} closed the connection before answering the key");
        return Err(ConnectError::ClosedEarly { app });
    }
    let answer: serde_json::Value = serde_json::from_str(first.trim())
        .map_err(|_| ConnectError::NotListener { app: app.clone() })?;
    if answer["ok"] != true {
        let why = answer["error"]
            .as_str()
            .unwrap_or("no reason given")
            .to_string();
        let code = answer["code"].as_str().unwrap_or("").to_string();
        log::warn!("paddock_mcp client: {app} turned the handshake down (code {code:?})");
        return Err(if code == "key" {
            ConnectError::KeyRefused { app, why }
        } else {
            ConnectError::Refused { app, why }
        });
    }
    log::info!(
        "paddock_mcp client: connected to {app} {} on port {} in {} ms",
        hint.version,
        hint.port,
        started.elapsed().as_millis()
    );
    Ok(LoopbackLink {
        stream,
        reader,
        app,
        version: hint.version,
    })
}
