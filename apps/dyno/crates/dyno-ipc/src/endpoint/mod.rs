//! A private local endpoint, never TCP.
//!
//! - **Windows**: a named pipe `\\.\pipe\dyno-<random hex>` created as the first and only instance (nobody can squat
//!   the name or add a second instance), rejecting remote clients, with an owner-only access list. Clients open it
//!   with identification-level impersonation only. I/O is overlapped, so one thread can read while another writes.
//! - **Unix**: a socket in a freshly created directory with mode 0700, removed when the listener is dropped.
//!
//! The OS-specific code lives in two small modules; this one is the portable surface.

use std::io::{self, Read, Write};
use std::time::Duration;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as sys;

/// A listening private endpoint for exactly one connection.
pub struct Listener(sys::Listener);

impl Listener {
    /// Creates a fresh endpoint with a random name.
    pub fn bind() -> io::Result<Listener> {
        sys::Listener::bind().map(Listener)
    }

    /// The name a client connects to (a pipe name or a socket path).
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// Waits up to `timeout` for a client. Returns the stream and the client's process id when the OS reports it.
    /// A timeout is `ErrorKind::TimedOut`; the listener can then wait again.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn pair() -> (Stream, Stream, Option<u32>) {
        let mut l = Listener::bind().unwrap();
        let name = l.name().to_string();
        let client = std::thread::spawn(move || Stream::connect(&name, Duration::from_secs(5)).unwrap());
        let (server, pid) = l.accept(Duration::from_secs(5)).unwrap();
        (server, client.join().unwrap(), pid)
    }

    #[test]
    fn names_are_random_and_private() {
        let a = Listener::bind().unwrap();
        let b = Listener::bind().unwrap();
        assert_ne!(a.name(), b.name());
        assert!(a.name().contains("dyno-"));
    }

    #[test]
    fn bytes_flow_both_ways_and_the_peer_pid_is_ours() {
        let (mut s, mut c, pid) = pair();
        if cfg!(any(windows, target_os = "linux", target_os = "macos")) {
            assert_eq!(pid, Some(std::process::id()));
        }
        c.write_all(b"ping").unwrap();
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"ping");
        s.write_all(b"pong").unwrap();
        c.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"pong");
        assert_eq!(s.read(&mut []).unwrap(), 0);
    }

    #[test]
    fn reading_and_writing_at_once_on_two_threads_does_not_block_each_other() {
        let (s, mut c, _) = pair();
        let mut reader = s.try_clone().unwrap();
        let mut writer = s;
        let r = std::thread::spawn(move || {
            let mut buf = [0u8; 5];
            reader.read_exact(&mut buf).map(|_| buf)
        });
        std::thread::sleep(Duration::from_millis(50));
        // The server's reader is blocked; its writer still gets through.
        writer.write_all(b"hello").unwrap();
        let mut buf = [0u8; 5];
        c.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello");
        c.write_all(b"world").unwrap();
        assert_eq!(&r.join().unwrap().unwrap(), b"world");
    }

    #[test]
    fn accept_and_reads_time_out() {
        let mut l = Listener::bind().unwrap();
        let t = Instant::now();
        assert_eq!(
            l.accept(Duration::from_millis(100)).err().map(|e| e.kind()),
            Some(io::ErrorKind::TimedOut)
        );
        assert!(t.elapsed() >= Duration::from_millis(90));
        // The listener still works after a timeout.
        let name = l.name().to_string();
        let client = std::thread::spawn(move || Stream::connect(&name, Duration::from_secs(5)).unwrap());
        let (mut s, _) = l.accept(Duration::from_secs(5)).unwrap();
        let _c = client.join().unwrap();
        s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut buf = [0u8; 1];
        let err = s.read(&mut buf).unwrap_err();
        assert!(matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock), "{err:?}");
        s.set_read_timeout(None).unwrap();
    }

    #[test]
    fn connecting_to_nothing_fails() {
        assert!(Stream::connect(&format!("{}-gone", Listener::bind().unwrap().name()), Duration::from_millis(50)).is_err());
    }

    #[test]
    fn a_closer_wakes_a_blocked_reader_and_the_other_side_sees_the_end() {
        let (s, mut c, _) = pair();
        let closer = s.try_clone().unwrap().closer().unwrap();
        let mut reader = s;
        let r = std::thread::spawn(move || {
            let mut buf = [0u8; 1];
            reader.read(&mut buf)
        });
        std::thread::sleep(Duration::from_millis(50));
        closer.close();
        closer.close();
        assert_eq!(r.join().unwrap().unwrap(), 0);
        drop(closer);
        let mut buf = [0u8; 1];
        assert_eq!(c.read(&mut buf).unwrap_or(0), 0);
        assert!(c.write_all(b"x").and_then(|_| c.write_all(&[0u8; 1 << 20])).is_err());
    }
}
