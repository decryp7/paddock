//! A private local endpoint, never TCP (from Dyno's `dyno-ipc`, with Pitcrew's service pipe access list).
//!
//! - **Windows**: a named pipe `\\.\pipe\paddock-<random hex>` created as the first and only instance (nobody can squat
//!   the name), rejecting remote clients, with an access list ([`Access`]: owner-only by default). Clients open it with
//!   identification-level impersonation only. I/O is overlapped, so one thread reads while another writes.
//! - **Unix**: a socket in a freshly created directory with mode 0700 (in the app's runtime folder or the temporary
//!   folder), removed when the listener is dropped.
//!
//! The OS-specific code lives in two small modules; this one is the portable surface.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
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
    pub fn service_for_interactive_users() -> Access {
        Access {
            mode: 0o600,
            sddl: "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;GRGW;;;IU)".into(),
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

    /// Waits up to `timeout` for a client. Returns the stream and the client's process id when the OS reports it. A
    /// timeout is `ErrorKind::TimedOut`; the listener can then wait again.
    pub fn accept(&mut self, timeout: Duration) -> io::Result<(Stream, Option<u32>)> {
        self.0.accept(timeout).map(|(s, pid)| (Stream(s), pid))
    }
}

/// One side of a connection. Clone it with [`Stream::try_clone`] to read on one thread and write on another.
pub struct Stream(sys::Stream);

impl Stream {
    /// Connects to the endpoint `name`, retrying for up to `timeout` while it is busy.
    pub fn connect(name: &str, timeout: Duration) -> io::Result<Stream> {
        sys::Stream::connect(name, timeout).map(Stream)
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
