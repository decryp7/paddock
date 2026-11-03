//! Unix: a socket in a fresh 0700 directory.

use crate::token::random_hex;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
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
    /// A fresh 0700 folder with one socket, under `base` (the app's runtime folder) when the path fits, else under the
    /// temporary folder.
    pub(super) fn bind(base: Option<&Path>) -> io::Result<Listener> {
        let mut last = io::Error::other("no place for a private socket");
        let bases = base
            .map(Path::to_path_buf)
            .into_iter()
            .chain([std::env::temp_dir(), PathBuf::from("/tmp")]);
        for base in bases {
            let dir = base.join(format!("paddock-{}", random_hex(8)?));
            let sock = dir.join("s.sock");
            if sock.as_os_str().len() > MAX_SOCKET_PATH {
                continue;
            }
            if let Err(e) = std::fs::create_dir_all(&base)
                .and_then(|()| std::fs::DirBuilder::new().mode(0o700).create(&dir))
            {
                last = e;
                continue;
            }
            // Defend against a permissive umask or an odd file system.
            let bound = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .and_then(|()| UnixListener::bind(&sock));
            match bound.and_then(|l| l.set_nonblocking(true).map(|()| l)) {
                Ok(listener) => {
                    return Ok(Listener {
                        listener,
                        name: sock.to_string_lossy().into_owned(),
                        cleanup: Cleanup::Dir(dir),
                    });
                }
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    last = e;
                }
            }
        }
        Err(last)
    }

    /// A socket at the fixed `path` with the file mode `access.mode` (a service's endpoint). The
    /// parent folder must exist. A stale socket file from a crashed run is replaced; anything else at the path is an error.
    pub(super) fn bind_fixed(path: &str, access: &super::Access) -> io::Result<Listener> {
        use std::os::unix::fs::FileTypeExt;
        if path.len() > MAX_SOCKET_PATH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the socket path is too long",
            ));
        }
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_socket() => std::fs::remove_file(path)?,
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{path} exists and is not a socket"),
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let listener = UnixListener::bind(path)?;
        let ready = std::fs::set_permissions(path, std::fs::Permissions::from_mode(access.mode))
            .and_then(|_| listener.set_nonblocking(true));
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
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "nobody connected in time",
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
    }
}

/// A system service's socket folder must belong to the account running this process and be closed to others.
pub(super) fn check_own_folder(socket: &str) -> Result<(), crate::trust::Untrusted> {
    // SAFETY: `geteuid` has no arguments and cannot fail.
    #[allow(unsafe_code)]
    let me = unsafe { libc::geteuid() };
    super::unix_peer::check_folder_of(socket, me)
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
        Ok(Stream {
            s,
            read_timeout: None,
        })
    }

    /// The user id of the process on the other end, from the kernel.
    pub(super) fn peer_uid(&self) -> Option<u32> {
        peer_uid(&self.s)
    }

    /// The facts the kernel holds about the other end.
    pub(super) fn raw_peer(&self) -> Result<crate::identity::RawPeer, crate::identity::PeerError> {
        super::unix_peer::raw_peer(&self.s)
    }

    /// Connects to a system endpoint served by root; see [`super::Stream::connect_system`].
    pub(super) fn connect_system(
        name: &str,
        timeout: Duration,
    ) -> Result<Stream, super::SystemConnectError> {
        Stream::connect_expecting(name, timeout, 0)
    }

    /// [`Stream::connect_system`] for a server that runs as `expected_uid`.
    fn connect_expecting(
        name: &str,
        timeout: Duration,
        expected_uid: u32,
    ) -> Result<Stream, super::SystemConnectError> {
        use super::SystemConnectError::{Connect, Untrusted};
        // No folder at all means nothing is listening (the service was never installed): that is not an untrusted server.
        let parent = Path::new(name)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if let Err(e) = std::fs::symlink_metadata(parent)
            && e.kind() == io::ErrorKind::NotFound
        {
            return Err(Connect(e));
        }
        super::unix_peer::check_folder_of(name, expected_uid).map_err(Untrusted)?;
        let stream = Stream::connect(name, timeout).map_err(Connect)?;
        let uid = peer_uid(&stream.s).ok_or_else(|| {
            Untrusted(crate::trust::Untrusted::Unreadable {
                what: "the listener's uid",
                cause: io::Error::last_os_error().to_string(),
            })
        })?;
        crate::trust::check_server_account(uid, expected_uid).map_err(Untrusted)?;
        Ok(stream)
    }

    pub(super) fn connect(name: &str, _timeout: Duration) -> io::Result<Stream> {
        // A Unix listener is never busy: the kernel queues the connection until `accept`.
        UnixStream::connect(name).and_then(Stream::new)
    }

    pub(super) fn try_clone(&self) -> io::Result<Stream> {
        // The copy shares the open file, so it is non-blocking too.
        self.s.try_clone().map(|s| Stream {
            s,
            read_timeout: None,
        })
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
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    let left = match deadline {
                        Some(d) => match d.checked_duration_since(Instant::now()) {
                            Some(left) if !left.is_zero() => Some(left),
                            _ => {
                                return Err(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "nothing arrived in time",
                                ));
                            }
                        },
                        None => None,
                    };
                    wait(&self.s, libc::POLLIN, left)?;
                }
                other => return other,
            }
        }
    }

    pub(super) fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match self.s.write(buf) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    wait(&self.s, libc::POLLOUT, None)?;
                }
                other => return other,
            }
        }
    }
}

/// Waits until `s` is ready for `events` (or has hung up or failed), up to `timeout`.
///
/// `Ok` means "look again": ready, timed out or interrupted; the caller's next read or write and its deadline check decide.
/// A descriptor that is not open (POLLNVAL) or has failed or hung up (POLLERR, POLLHUP) is not an error here: the
/// caller's next read or write reports its own error. A failing `poll` itself (EINVAL, ENOMEM, ...) is returned, else a
/// caller whose reads and writes keep giving EAGAIN would spin on it.
fn wait(s: &UnixStream, events: libc::c_short, timeout: Option<Duration>) -> io::Result<()> {
    wait_with(s, events, timeout, sys_poll)
}

/// The real `poll(2)` on one descriptor: the number of ready descriptors, or the OS error.
#[allow(unsafe_code)]
fn sys_poll(p: &mut libc::pollfd, ms: libc::c_int) -> io::Result<libc::c_int> {
    // SAFETY: one valid pollfd for the duration of the call.
    let rc = unsafe { libc::poll(p, 1, ms) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(rc)
    }
}

/// [`wait`] with the poll call passed in (a test injects a failing one).
fn wait_with(
    s: &UnixStream,
    events: libc::c_short,
    timeout: Option<Duration>,
    poll: impl FnOnce(&mut libc::pollfd, libc::c_int) -> io::Result<libc::c_int>,
) -> io::Result<()> {
    let ms = timeout.map_or(-1, |t| {
        libc::c_int::try_from(t.as_millis().max(1)).unwrap_or(libc::c_int::MAX)
    });
    let mut p = libc::pollfd {
        fd: s.as_raw_fd(),
        events,
        revents: 0,
    };
    match poll(&mut p, ms) {
        Err(e) if e.kind() != io::ErrorKind::Interrupted => Err(e),
        _ => Ok(()),
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
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
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
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
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
    #[allow(unused_imports)]
    use super::*;

    /// The client's checks of a system server, with this account standing in for root (a test cannot be root).
    #[test]
    fn a_server_run_by_the_expected_account_in_its_closed_folder_is_trusted() {
        use std::os::unix::fs::PermissionsExt;
        #[allow(unsafe_code)]
        let me = unsafe { libc::geteuid() };
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let name = dir.path().join("s.sock").to_string_lossy().into_owned();
        let mut listener =
            Listener::bind_fixed(&name, &crate::endpoint::Access::system_for_all_users()).unwrap();
        let ok = Stream::connect_expecting(&name, Duration::from_secs(5), me).unwrap();
        drop(ok);
        assert!(listener.accept(Duration::from_secs(5)).is_ok());
        // The same server, expected to be another account: refused after the folder check, naming the owner.
        let e = Stream::connect_expecting(&name, Duration::from_secs(5), me.wrapping_add(1))
            .err()
            .unwrap();
        assert!(
            matches!(e, crate::endpoint::SystemConnectError::Untrusted(crate::trust::Untrusted::FolderOwner { uid }) if uid == me)
        );
        // Nobody listening: a connect error, not a trust error.
        drop(listener);
        let e = Stream::connect_expecting(&name, Duration::from_secs(1), me)
            .err()
            .unwrap();
        assert!(matches!(e, crate::endpoint::SystemConnectError::Connect(_)));
    }

    /// macOS: whether the socket behind `fd` is marked SS_DRAINING (0x4000 in `so_state`, read through
    /// `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)`; `soi_state` sits at byte 192 of the 792-byte `socket_fdinfo`).
    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn draining(fd: std::os::fd::RawFd) -> bool {
        let mut info = [0u8; 792];
        // SAFETY: the buffer is valid for writes of the size passed; PROC_PIDFDSOCKETINFO is 3.
        let n =
            unsafe { libc::proc_pidfdinfo(libc::getpid(), fd, 3, info.as_mut_ptr().cast(), 792) };
        n > 0 && i16::from_ne_bytes([info[192], info[193]]) & 0x4000 != 0
    }

    /// Puts the socket behind `s` into the state an inspected close leaves it in (2026-10-31, docs/lessons.md): a copy
    /// of the descriptor is replaced while a thread is inside a read on that copy, so XNU drains it and marks the SOCKET
    /// (shared by every descriptor of it) SS_DRAINING for good. Call it while `s` is still blocking (the reader must
    /// wait inside the socket). The copy's slot is replaced by /dev/null with `dup2`, never closed, so a late reader
    /// gets end of file and can never read a number another thread has reused. Waits on the condition, bounded.
    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn poison(s: &UnixStream) {
        let fd = s.as_raw_fd();
        let poisoned = (0..2000).any(|_| {
            // SAFETY: a copy of an open descriptor and a fresh /dev/null, each closed exactly once below.
            let (copy, null) = unsafe {
                (
                    libc::dup(fd),
                    libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY),
                )
            };
            assert!(copy >= 0 && null >= 0);
            let reader = std::thread::spawn(move || {
                let mut b = [0u8; 1];
                // SAFETY: reads into a valid one-byte buffer; the slot is replaced under it (that is the point).
                unsafe { libc::read(copy, b.as_mut_ptr().cast(), 1) }
            });
            std::thread::yield_now();
            // SAFETY: replaces our own copy's slot atomically, then closes the spare and (once the reader is done) the copy.
            unsafe {
                libc::dup2(null, copy);
                libc::close(null);
            }
            let _ = reader.join();
            // SAFETY: our own copy, no thread uses it any more.
            unsafe { libc::close(copy) };
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
        poison(&a);
        let (mut ours, mut theirs) = (Stream::new(a).unwrap(), Stream::new(b).unwrap());
        // Nothing comes: the read waits until its timeout (a draining socket fails at once with EBADF instead).
        ours.set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let mut buf = [0u8; 1];
        let err = ours.read(&mut buf).unwrap_err();
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{err:?}"
        );
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
            sent += ours
                .write(&big[sent..])
                .expect("a write on a draining socket waits for room");
        }
        assert_eq!(drain.join().unwrap(), 1 << 20);
    }

    /// On every Unix: a timed read times out, a read with no timeout wakes when data arrives, a clone shares the
    /// non-blocking socket, and a closer wakes a waiting read with end of stream.
    #[test]
    fn reads_wait_on_poll_time_out_and_wake_on_data_or_close() {
        let (a, b) = UnixStream::pair().unwrap();
        let (mut ours, mut theirs) = (Stream::new(a).unwrap(), Stream::new(b).unwrap());
        ours.set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(
            ours.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        ours.set_read_timeout(None).unwrap();
        let mut clone = ours.try_clone().unwrap();
        let t = std::thread::spawn(move || {
            let mut b = [0u8; 4];
            let n = clone.read(&mut b).unwrap();
            (n, b)
        });
        theirs.write(b"hi").unwrap();
        let (n, got) = t.join().unwrap();
        assert_eq!((n, &got[..n]), (2, &b"hi"[..]));
        let closer = ours.closer().unwrap();
        let t = std::thread::spawn(move || {
            let mut b = [0u8; 4];
            ours.read(&mut b)
        });
        closer.close();
        assert_eq!(t.join().unwrap().unwrap(), 0);
    }

    /// A failing `poll` (EINVAL, ENOMEM) is an error for the caller, not something to spin on; an interrupted one is not.
    #[test]
    fn a_failing_poll_is_returned_but_an_interrupted_one_is_not() {
        let (a, _b) = UnixStream::pair().unwrap();
        let fail =
            |_: &mut libc::pollfd, _: libc::c_int| Err(io::Error::from_raw_os_error(libc::EINVAL));
        let err = wait_with(&a, libc::POLLIN, None, fail).unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::EINVAL));
        let eintr =
            |_: &mut libc::pollfd, _: libc::c_int| Err(io::Error::from_raw_os_error(libc::EINTR));
        wait_with(&a, libc::POLLIN, None, eintr).unwrap();
        // A poll that reports a bad descriptor through revents (POLLNVAL) is Ok: the next read or write reports the error.
        let nval = |p: &mut libc::pollfd, _: libc::c_int| {
            p.revents = libc::POLLNVAL;
            Ok(1)
        };
        wait_with(&a, libc::POLLIN, None, nval).unwrap();
    }

    /// A closer's shutdown wakes a write that is blocked on a full buffer (the peer never reads) with an error.
    #[test]
    fn a_closer_wakes_a_write_blocked_on_a_full_buffer() {
        let (a, _never_reads) = UnixStream::pair().unwrap();
        let mut ours = Stream::new(a).unwrap();
        let closer = ours.closer().unwrap();
        let (full_tx, full_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let chunk = [1u8; 4096];
            let mut signalled = false;
            loop {
                // Once a non-blocking write would block, the buffer is full: tell the test, then keep writing (it waits).
                if !signalled {
                    match ours.s.write(&chunk) {
                        Ok(_) => continue,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            signalled = true;
                            let _ = full_tx.send(());
                        }
                        Err(e) => return e,
                    }
                }
                if let Err(e) = ours.write(&chunk) {
                    return e;
                }
            }
        });
        full_rx.recv().unwrap();
        closer.close();
        let err = writer.join().unwrap();
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::NotConnected
            ),
            "{err:?}"
        );
    }
}
