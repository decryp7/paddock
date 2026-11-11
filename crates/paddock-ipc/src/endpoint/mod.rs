//! A private local endpoint, never TCP (from Dyno's `dyno-ipc`, with Pitcrew's service pipe access list).
//!
//! - **Windows**: a named pipe `\\.\pipe\paddock-<random hex>` created as the first and only instance (nobody can squat
//!   the name), rejecting remote clients, with an access list ([`Access`]: owner-only by default). Clients open it with
//!   identification-level impersonation only. I/O is overlapped, so one thread reads while another writes.
//! - **Unix**: a socket in a freshly created directory with mode 0700 (in the app's runtime folder or the temporary
//!   folder), removed when the listener is dropped.
//!
//! The OS-specific code lives in two small modules; this one is the portable surface.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::identity::{PeerError, PeerSource, RawPeer};
use crate::trust::Untrusted;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
mod unix_peer;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod windows_peer;
#[cfg(windows)]
use self::windows as sys;

/// Who may open an endpoint: the file mode of a Unix socket, the access list (SDDL) of a Windows pipe. Each OS reads its
/// own half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    /// The socket file's mode.
    pub mode: u32,
    /// The pipe's security descriptor in SDDL.
    pub sddl: String,
}

impl Access {
    /// The owner (the user running the process) and nobody else: every app ↔ child and app ↔ agent link.
    pub fn owner_only() -> Access {
        Access {
            mode: 0o600,
            sddl: "D:P(A;;GA;;;OW)".into(),
        }
    }

    /// A Windows service that serves the logged-on user's app (Pitcrew): SYSTEM, Administrators and the pipe's owner get
    /// full access, interactive users read and write, nobody else (services, network logons, other sessions'
    /// non-interactive tokens). On Unix the service runs as the user, so the socket stays owner-only.
    ///
    /// Interactive users get `0x12019b` (`FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES | FILE_WRITE_EA`), not
    /// `GENERIC_WRITE`: that includes `FILE_APPEND_DATA`, which is `FILE_CREATE_PIPE_INSTANCE` on a pipe, and a user who can
    /// create another instance of the pipe (the owner check does not see it) could serve forged answers.
    pub fn service_for_interactive_users() -> Access {
        Access {
            mode: 0o600,
            sddl: "O:SYD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;0x12019b;;;IU)".into(),
        }
    }

    /// A system endpoint any local account may connect to (Dyno's helper, ADR 0027): a Unix socket with mode 0666 in a
    /// folder only the service can change, or a pipe **owned by SYSTEM** (`O:SY`: a LocalSystem service's objects default
    /// to the Administrators group as owner, which clients refuse) that SYSTEM and Administrators open fully, interactive
    /// users read and write, and network logons never reach. Who may do what is decided per request from the caller's
    /// identity ([`Stream::raw_peer`](crate::identity::PeerSource)), not by this access list.
    pub fn system_for_all_users() -> Access {
        Access {
            mode: 0o666,
            sddl: "O:SYD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;IU)(D;;GA;;;NU)".into(),
        }
    }
}

/// A listening private endpoint. Each [`Listener::accept`] hands over one connection; the listener keeps listening.
pub struct Listener(sys::Listener);

impl Listener {
    /// A fresh endpoint with a random name in the temporary folder (Unix) or the pipe namespace (Windows), owner-only.
    pub fn bind() -> io::Result<Listener> {
        sys::Listener::bind(None).map(Listener)
    }

    /// A fresh endpoint with a random name in a private folder under `dir` (the app's runtime folder, `AppDirs::runtime`);
    /// falls back to the temporary folder when the path would be too long for a socket. Windows ignores `dir`.
    pub fn bind_in(dir: &Path) -> io::Result<Listener> {
        sys::Listener::bind(Some(dir)).map(Listener)
    }

    /// The endpoint `name` (a socket path or a pipe name) with a fixed name and the given access (a service's endpoint).
    /// A stale socket file from a crashed run is replaced; anything else at the path is an error.
    pub fn bind_fixed(name: &str, access: &Access) -> io::Result<Listener> {
        sys::Listener::bind_fixed(name, access).map(Listener)
    }

    /// The name a client connects to (a pipe name or a socket path).
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// Like [`Listener::bind_fixed`] for a system service: on Unix the socket's folder must belong to the account running
    /// this process and have no group or world write bit (a client checks the same before it trusts the socket), so nobody
    /// else can replace the socket. On Windows the pipe is created as the first instance, which makes a squatter's earlier
    /// pipe an error (`ERROR_PIPE_BUSY`, 231) and not an answer.
    pub fn bind_system(name: &str, access: &Access) -> Result<Listener, Untrusted> {
        #[cfg(unix)]
        sys::check_own_folder(name)?;
        sys::Listener::bind_fixed(name, access)
            .map(Listener)
            .map_err(Untrusted::create_failed)
    }

    /// Waits up to `timeout` for a client. Returns the stream and the client's process id when the OS reports it. A
    /// timeout is `ErrorKind::TimedOut`; the listener can then wait again.
    pub fn accept(&mut self, timeout: Duration) -> io::Result<(Stream, Option<u32>)> {
        self.0.accept(timeout).map(|(s, pid)| (Stream(s), pid))
    }

    /// After [`Listener::accept`] failed because the process has no file descriptor left ([`out_of_descriptors`]), accepts
    /// the waiting client on a descriptor the listener keeps in reserve, so the caller can tell it why it is turned away and
    /// close it; otherwise it would wait in the queue until its own limit (Linux). `None` when no client waits, on macOS
    /// (whose failed accept has already dropped the client) and on Windows (no descriptor table). A stopgap of ADR 0072 for
    /// I7, to be absorbed by M3/M4.
    pub(crate) fn accept_on_spare(&mut self) -> Option<Stream> {
        self.0.accept_on_spare().map(Stream)
    }
}

/// Whether `e` says the process or the system has no file descriptor left (EMFILE, ENFILE); always `false` on Windows.
pub(crate) fn out_of_descriptors(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        sys::out_of_descriptors(e)
    }
    #[cfg(not(unix))]
    {
        let _: &io::Error = e;
        false
    }
}

/// One side of a connection. Clone it with [`Stream::try_clone`] to read on one thread and write on another.
pub struct Stream(sys::Stream);

impl Stream {
    /// Connects to the endpoint `name`, retrying for up to `timeout` while it is busy.
    pub fn connect(name: &str, timeout: Duration) -> io::Result<Stream> {
        sys::Stream::connect(name, timeout).map(Stream)
    }

    /// Connects to a system endpoint and checks the server before anything is sent (ADR 0027 §2): on Windows the pipe is
    /// opened at identification level and its owner must be SYSTEM; on Unix the socket's folder must be root's and closed
    /// to others, and the process listening must run as root. A server that fails is [`SystemConnectError::Untrusted`],
    /// which callers treat as "the service is missing".
    pub fn connect_system(name: &str, timeout: Duration) -> Result<Stream, SystemConnectError> {
        sys::Stream::connect_system(name, timeout).map(Stream)
    }

    /// The user id of the process on the other end, where the OS tells (Unix); `None` on Windows (the access list
    /// decides there).
    pub fn peer_uid(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.0.peer_uid()
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Another handle to the same connection.
    pub fn try_clone(&self) -> io::Result<Stream> {
        self.0.try_clone().map(Stream)
    }

    /// Makes reads on this handle fail with `ErrorKind::TimedOut` after `timeout` (`None`: wait forever).
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }

    /// Something that ends the connection from any thread: blocked reads return end of stream, writes fail, and the
    /// other side sees the end of the stream once every handle is gone.
    pub fn closer(&self) -> io::Result<Closer> {
        self.0.closer().map(Closer)
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl PeerSource for Stream {
    /// The caller's facts from the OS (audit token, `SO_PEERCRED`, the caller's own token); never from a pid. On Windows
    /// the OS may answer [`PeerError::NotReady`] before the client's first bytes arrive (Windows 11 build 26300 did not).
    fn raw_peer(&self) -> Result<RawPeer, PeerError> {
        self.0.raw_peer()
    }
}

/// Why [`Stream::connect_system`] gave no connection.
#[derive(Debug)]
pub enum SystemConnectError {
    /// The endpoint could not be opened (nobody listens, the name is not there, a timeout).
    Connect(io::Error),
    /// Something answered, but it is not the service: nothing was sent to it.
    Untrusted(Untrusted),
}

impl fmt::Display for SystemConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SystemConnectError::Connect(e) => write!(f, "the service could not be reached: {e}"),
            SystemConnectError::Untrusted(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for SystemConnectError {}

/// The caller's facts on a Windows named pipe the caller does not own as a [`Stream`] (a service that serves its own pipe, such
/// as Pitcrew's): the same read as [`PeerSource::raw_peer`] (the client's own token through `ImpersonateNamedPipeClient`, never
/// a pid), for the server end `pipe` of a connected instance. Decide with [`crate::identity::identify`].
///
/// # Errors
/// [`PeerError::NotReady`] before the client's first bytes arrived (read once, then ask again), or what the OS refused.
#[cfg(windows)]
pub fn raw_peer_of_pipe(pipe: std::os::windows::io::RawHandle) -> Result<RawPeer, PeerError> {
    windows_peer::raw_peer(pipe)
}

/// Ends a connection; see [`Stream::closer`].
pub struct Closer(sys::Closer);

impl Closer {
    /// Ends the connection. Idempotent.
    pub fn close(&self) {
        self.0.close()
    }
}

/// Two connected streams on a fresh private endpoint (in-process links and tests).
pub fn pair() -> io::Result<(Stream, Stream)> {
    let mut l = Listener::bind()?;
    let name = l.name().to_string();
    let client = std::thread::spawn(move || Stream::connect(&name, Duration::from_secs(30)));
    let accepted = l.accept(Duration::from_secs(30));
    let client = client
        .join()
        .map_err(|_| io::Error::other("the connecting thread panicked"))??;
    Ok((accepted?.0, client))
}
