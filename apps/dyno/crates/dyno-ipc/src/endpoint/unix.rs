//! Unix: a socket in a fresh 0700 directory.

use crate::token::random_hex;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Unix socket paths are limited to about 104 bytes.
const MAX_SOCKET_PATH: usize = 100;

pub(super) struct Listener {
    listener: UnixListener,
    name: String,
    /// What to remove on drop: the private directory, or just the socket file of a fixed endpoint.
    cleanup: Cleanup,
}

enum Cleanup {
    Dir(PathBuf),
    File(PathBuf),
}

impl Listener {
    pub(super) fn bind() -> io::Result<Listener> {
        let mut last = io::Error::other("no place for a private socket");
        for base in [std::env::temp_dir(), PathBuf::from("/tmp")] {
            let dir = base.join(format!("dyno-{}", random_hex(8)?));
            let sock = dir.join("s.sock");
            if sock.as_os_str().len() > MAX_SOCKET_PATH {
                continue;
            }
            if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(&dir) {
                last = e;
                continue;
            }
            // Defend against a permissive umask or an odd file system.
            let bound = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).and_then(|_| UnixListener::bind(&sock));
            match bound.and_then(|l| l.set_nonblocking(true).map(|_| l)) {
                Ok(listener) => {
                    return Ok(Listener {
                        listener,
                        name: sock.to_string_lossy().into_owned(),
                        cleanup: Cleanup::Dir(dir),
                    })
                }
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    last = e;
                }
            }
        }
        Err(last)
    }

    /// A socket at the fixed `path` with the file mode `access.mode` (the privileged helper's endpoint, ADR 0016). The
    /// parent folder must exist. A stale socket file from a crashed run is replaced; anything else at the path is an error.
    pub(super) fn bind_fixed(path: &str, access: &super::Access) -> io::Result<Listener> {
        use std::os::unix::fs::FileTypeExt;
        if path.len() > MAX_SOCKET_PATH {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the socket path is too long"));
        }
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_socket() => std::fs::remove_file(path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{path} exists and is not a socket"),
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(path)?;
        let ready =
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(access.mode)).and_then(|_| listener.set_nonblocking(true));
        if let Err(e) = ready {
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
        Ok(Listener {
            listener,
            name: path.to_string(),
            cleanup: Cleanup::File(PathBuf::from(path)),
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn accept(&mut self, timeout: Duration) -> io::Result<(Stream, Option<u32>)> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((s, _)) => {
                    let pid = peer_pid(&s);
                    return Stream::new(s).map(|s| (s, pid));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "nobody connected in time"));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        match &self.cleanup {
            Cleanup::Dir(d) => {
                let _ = std::fs::remove_dir_all(d);
            }
            Cleanup::File(f) => {
                let _ = std::fs::remove_file(f);
            }
        }
    }
}

/// One handle of a connection. The socket is non-blocking and every wait is a `poll`: a read or write never sleeps inside
/// the socket itself. On macOS a socket can be left SS_DRAINING for good when a copy of it is closed while another
/// process inspects that copy (a socket lister such as Dyno's own network page, lsof, Activity Monitor; a spawned child
/// closes the copies it holds at exec); from then on every read or write that would WAIT in the socket fails at once with
/// EBADF, while `poll` and non-blocking calls keep working (docs/lessons.md, 2026-10-31).
pub(super) struct Stream {
    s: UnixStream,
    read_timeout: Option<Duration>,
}

impl Stream {
    fn new(s: UnixStream) -> io::Result<Stream> {
        s.set_nonblocking(true)?;
        Ok(Stream { s, read_timeout: None })
    }

    /// The user id of the process on the other end, from the kernel.
    pub(super) fn peer_uid(&self) -> Option<u32> {
        peer_uid(&self.s)
    }

    pub(super) fn connect(name: &str, _timeout: Duration) -> io::Result<Stream> {
        UnixStream::connect(name).and_then(Stream::new)
    }

    pub(super) fn try_clone(&self) -> io::Result<Stream> {
        // The copy shares the open file, so it is non-blocking too.
        self.s.try_clone().map(|s| Stream { s, read_timeout: None })
    }

    pub(super) fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.read_timeout = timeout;
        Ok(())
    }

    pub(super) fn closer(&self) -> io::Result<Closer> {
        self.s.try_clone().map(Closer)
    }

    pub(super) fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let deadline = self.read_timeout.map(|t| Instant::now() + t);
        loop {
            match self.s.read(buf) {
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {
                    let left = match deadline {
                        Some(d) => match d.checked_duration_since(Instant::now()) {
                            Some(left) if !left.is_zero() => Some(left),
                            _ => return Err(io::Error::new(io::ErrorKind::TimedOut, "nothing arrived in time")),
                        },
                        None => None,
                    };
                    wait(&self.s, libc::POLLIN, left);
                }
                other => return other,
            }
        }
    }

    pub(super) fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match self.s.write(buf) {
                Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => wait(&self.s, libc::POLLOUT, None),
                other => return other,
            }
        }
    }
}

/// Waits until `s` is ready for `events` (or has hung up or failed: the next call reports it), up to `timeout`.
#[allow(unsafe_code)]
fn wait(s: &UnixStream, events: libc::c_short, timeout: Option<Duration>) {
    let ms = timeout.map_or(-1, |t| libc::c_int::try_from(t.as_millis().max(1)).unwrap_or(libc::c_int::MAX));
    let mut p = libc::pollfd {
        fd: s.as_raw_fd(),
        events,
        revents: 0,
    };
    // SAFETY: one valid pollfd for the duration of the call.
    let _ = unsafe { libc::poll(&mut p, 1, ms) };
    // Ready, timed out, interrupted or failed: the caller's next call or deadline check decides (a failed descriptor
    // reports its own error from the read or write).
}

pub(super) struct Closer(UnixStream);

impl Closer {
    pub(super) fn close(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

/// The connected peer's process id, from the kernel.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn peer_pid(s: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for writes of the sizes passed; the fd is open for the call.
    let rc = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0).then(|| u32::try_from(cred.pid).ok()).flatten()
}

/// The connected peer's process id, from the kernel.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn peer_pid(s: &UnixStream) -> Option<u32> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: `pid` and `len` are valid for writes of the sizes passed; the fd is open for the call.
    let rc = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    (rc == 0).then(|| u32::try_from(pid).ok()).flatten()
}

/// Other Unixes: unknown (the token still authenticates the peer).
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_pid(_: &UnixStream) -> Option<u32> {
    None
}

/// The connected peer's user id, from the kernel.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn peer_uid(s: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for writes of the sizes passed; the fd is open for the call.
    let rc = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (rc == 0).then_some(cred.uid)
}

/// The connected peer's user id, from the kernel (`getpeereid`).
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn peer_uid(s: &UnixStream) -> Option<u32> {
    let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
    // SAFETY: `uid` and `gid` are valid for writes; the fd is open for the call.
    let rc = unsafe { libc::getpeereid(s.as_raw_fd(), &mut uid, &mut gid) };
    (rc == 0).then_some(uid)
}

/// Other Unixes: unknown (a caller that needs the user refuses the connection).
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_uid(_: &UnixStream) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// macOS: whether the socket behind `fd` is marked SS_DRAINING (0x4000 in `so_state`, read through
    /// `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`; `soi_state` sits at byte 192 of the 792-byte `socket_fdinfo`).
    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn draining(fd: std::os::fd::RawFd) -> bool {
        let mut info = [0u8; 792];
        // SAFETY: the buffer is valid for writes of the size passed; PROC_PIDFDSOCKETINFO is 3.
        let n = unsafe { libc::proc_pidfdinfo(libc::getpid(), fd, 3, info.as_mut_ptr().cast(), 792) };
        n > 0 && i16::from_ne_bytes([info[192], info[193]]) & 0x4000 != 0
    }

    /// Puts the socket behind `fd` into the state an inspected close leaves it in (2026-10-31, docs/lessons.md): a copy
    /// of the descriptor is closed while a thread is inside a read on that copy, so XNU drains it and marks the SOCKET
    /// (shared by every descriptor of it) SS_DRAINING for good. Waits on the condition, bounded.
    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn poison(fd: std::os::fd::RawFd) {
        let poisoned = (0..2000).any(|_| {
            // SAFETY: a copy of an open descriptor, closed exactly once below.
            let copy = unsafe { libc::dup(fd) };
            assert!(copy >= 0);
            let reader = std::thread::spawn(move || {
                let mut b = [0u8; 1];
                // SAFETY: reads into a valid one-byte buffer; the descriptor may be closed under it (that is the point).
                unsafe { libc::read(copy, b.as_mut_ptr().cast(), 1) }
            });
            std::thread::yield_now();
            // SAFETY: our own copy.
            unsafe { libc::close(copy) };
            let _ = reader.join();
            draining(fd)
        });
        assert!(poisoned, "could not put the socket into the draining state");
    }

    /// Regression (Dyno agent tests, 1 run in 20 under load): a socket another process inspected while one of its copies
    /// was closed (Dyno's own socket lister, lsof, Activity Monitor; a spawned child closes the copies it got at exec) is
    /// left SS_DRAINING, and from then on every read that has to WAIT fails at once with EBADF although the descriptor is
    /// open and healthy. A stream must keep working on such a socket: a read waits for data, a read timeout still times
    /// out, and a write into a full buffer waits for room.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_draining_socket_still_waits_for_data_times_out_and_writes() {
        let (a, b) = UnixStream::pair().unwrap();
        let (mut ours, mut theirs) = (Stream::new(a).unwrap(), Stream::new(b).unwrap());
        poison(ours.s.as_raw_fd());
        // Nothing comes: the read waits until its timeout (a draining socket fails at once with EBADF instead).
        ours.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let mut buf = [0u8; 1];
        let err = ours.read(&mut buf).unwrap_err();
        assert!(matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock), "{err:?}");
        ours.set_read_timeout(None).unwrap();
        // Data that is there is read.
        theirs.write(b"x").unwrap();
        assert_eq!(ours.read(&mut buf).unwrap(), 1);
        // A write far larger than the socket buffer waits for room while the peer drains it.
        let drain = std::thread::spawn(move || {
            let mut sink = vec![0u8; 1 << 20];
            let mut got = 0;
            while got < 1 << 20 {
                got += theirs.read(&mut sink[got..]).unwrap();
            }
            got
        });
        let big = vec![7u8; 1 << 20];
        let mut sent = 0;
        while sent < big.len() {
            sent += ours.write(&big[sent..]).expect("a write on a draining socket waits for room");
        }
        assert_eq!(drain.join().unwrap(), 1 << 20);
    }
}
