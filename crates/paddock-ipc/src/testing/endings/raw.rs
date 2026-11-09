//! The raw-stream client: it reads below every library's folding of resets into "closed", so a test sees the exact
//! `io::ErrorKind` with which a connection ended and the bytes that came before (ADR 0072 §5).

use std::io::{self, Read, Write};
use std::time::Duration;

/// The longest a raw read waits for the next bytes. It is a hang guard for something that must already have happened
/// (an answer, the end of the stream), never a pause to let something finish.
pub const READ_GUARD: Duration = Duration::from_secs(20);

/// The most bytes a raw client keeps (the last ones); the read goes on to the end of the stream.
const KEEP: usize = 4 << 20;

/// A stream the harness can read from and write to, with a read timeout and a half-close where the transport has one.
pub trait RawStream: Read + Write + Send {
    /// Makes a read fail with `TimedOut` after `timeout` (`None`: wait forever).
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
    /// Closes the write half. `Unsupported` where the transport has none (a named pipe).
    fn shutdown_write(&mut self) -> io::Result<()>;
}

impl RawStream for std::net::TcpStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        std::net::TcpStream::set_read_timeout(self, timeout)
    }
    fn shutdown_write(&mut self) -> io::Result<()> {
        self.shutdown(std::net::Shutdown::Write)
    }
}

#[cfg(unix)]
impl RawStream for std::os::unix::net::UnixStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, timeout)
    }
    fn shutdown_write(&mut self) -> io::Result<()> {
        self.shutdown(std::net::Shutdown::Write)
    }
}

impl RawStream for crate::endpoint::Stream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        crate::endpoint::Stream::set_read_timeout(self, timeout)
    }
    fn shutdown_write(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a paddock-ipc stream has no half-close",
        ))
    }
}

/// How a raw read ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEndKind {
    /// The peer's orderly end of the stream.
    Eof,
    /// The read failed with this kind (a reset is `ConnectionReset` or `ConnectionAborted`).
    Error(io::ErrorKind),
    /// Nothing came within [`READ_GUARD`]: the peer neither answered nor ended the stream.
    TimedOut,
}

impl RawEndKind {
    /// Whether the end was a reset, which discards what the peer had not yet read.
    pub fn is_reset(self) -> bool {
        matches!(
            self,
            RawEndKind::Error(
                io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
            )
        )
    }
}

/// What a raw client received: every byte up to the end, and how the end came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEnd {
    /// The bytes read, in order.
    pub bytes: Vec<u8>,
    /// How the stream ended.
    pub kind: RawEndKind,
}

/// Reads `stream` to its end, waiting at most `guard` for each read.
pub fn read_to_end(stream: &mut dyn RawStream, guard: Duration) -> RawEnd {
    let mut bytes = Vec::new();
    // macOS refuses the option on a socket the peer has already reset (EINVAL); the read below then reports how it ended, so
    // a refused option is not an end.
    stream.set_read_timeout(Some(guard)).ok();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                return RawEnd {
                    bytes,
                    kind: RawEndKind::Eof,
                };
            }
            Ok(n) => {
                // Reads on to the real end of the stream and keeps the last KEEP bytes: the end is what the scenario judges
                // (a reason is the last thing written), so a long stream must not hide it.
                bytes.extend_from_slice(&buf[..n]);
                if bytes.len() > KEEP {
                    let excess = bytes.len() - KEEP;
                    bytes.drain(..excess);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return RawEnd {
                    bytes,
                    kind: RawEndKind::TimedOut,
                };
            }
            Err(e) => {
                return RawEnd {
                    bytes,
                    kind: RawEndKind::Error(e.kind()),
                };
            }
        }
    }
}

/// Writes `junk` to `stream` in chunks until `total` bytes went out or a write fails (the peer ended the connection, which
/// is the point of the scenario, not an error). Returns the bytes written.
pub fn write_junk(stream: &mut dyn RawStream, total: usize) -> usize {
    let chunk = [0x5au8; 4096];
    let mut written = 0;
    while written < total {
        match stream.write(&chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => written += n,
        }
    }
    written
}
