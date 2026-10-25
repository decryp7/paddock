//! Unix: a socket in a fresh 0700 directory.

use crate::token::random_hex;
use std::io::{self, Read, Write};
use std::net::Shutdown;
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
                    // Accepted sockets inherit non-blocking mode on some systems.
                    s.set_nonblocking(false)?;
                    let pid = peer_pid(&s);
                    return Ok((Stream(s), pid));
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

pub(super) struct Stream(UnixStream);

impl Stream {
    /// The user id of the process on the other end, from the kernel.
    pub(super) fn peer_uid(&self) -> Option<u32> {
        peer_uid(&self.0)
    }

    pub(super) fn connect(name: &str, _timeout: Duration) -> io::Result<Stream> {
        UnixStream::connect(name).map(Stream)
    }

    pub(super) fn try_clone(&self) -> io::Result<Stream> {
        self.0.try_clone().map(Stream)
    }

    pub(super) fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        // std refuses a zero duration.
        self.0.set_read_timeout(timeout.map(|t| t.max(Duration::from_millis(1))))
    }

    pub(super) fn closer(&self) -> io::Result<Closer> {
        self.0.try_clone().map(Closer)
    }

    pub(super) fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    pub(super) fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
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
    use std::os::fd::AsRawFd;
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
    use std::os::fd::AsRawFd;
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
    use std::os::fd::AsRawFd;
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
    use std::os::fd::AsRawFd;
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
