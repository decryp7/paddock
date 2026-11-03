//! Unix: the facts the kernel attaches to a connection (never a pid-based lookup) and the client's check of the server.
//!
//! - macOS: `LOCAL_PEERTOKEN`, the peer's audit token (uid, gid, pid and pid version together, so a recycled pid cannot be
//!   mistaken for it); groups come from `getgrouplist` for that uid; the program for the audit line from
//!   `proc_pidpath_audittoken`.
//! - Linux: `SO_PEERCRED` (uid and gid fixed at `connect`) and `SO_PEERGROUPS`; the program for the audit line is read
//!   through `SO_PEERPIDFD` (Linux 6.5 and later) and the pidfd's `/proc` entry, and is missing on older kernels.
#![allow(unsafe_code)]

use crate::identity::{AccountId, AdminEvidence, AuditHint, PeerError, RawPeer};
use crate::trust::{Untrusted, check_socket_folder};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;

fn unreadable(what: &'static str) -> PeerError {
    PeerError::Unreadable {
        what,
        cause: io::Error::last_os_error().to_string(),
    }
}

/// What the check needs to know about a folder.
pub(super) struct FolderFacts {
    uid: u32,
    mode: u32,
}

/// An `io::Error` as the reason a server could not be checked.
fn untrusted_io(what: &'static str) -> impl FnOnce(io::Error) -> Untrusted {
    move |e| Untrusted::Unreadable {
        what,
        cause: e.to_string(),
    }
}

/// Opens `folder` by handle (no symlink on the last component) and reads its owner and mode from that handle.
fn read_folder(folder: &Path) -> Result<FolderFacts, Untrusted> {
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(folder)
        .map_err(untrusted_io("open the socket's folder"))?;
    let meta = handle
        .metadata()
        .map_err(untrusted_io("fstat the socket's folder"))?;
    Ok(FolderFacts {
        uid: meta.uid(),
        mode: meta.mode(),
    })
}

/// Checks the folder that holds the socket `socket`: it must belong to `expected_uid` and have no group or world write bit.
pub(super) fn check_folder_of(socket: &str, expected_uid: u32) -> Result<(), Untrusted> {
    check_folder_with(socket, expected_uid, read_folder)
}

/// [`check_folder_of`] with the read of the folder passed in (tests inject folders and failures).
fn check_folder_with(
    socket: &str,
    expected_uid: u32,
    read: impl FnOnce(&Path) -> Result<FolderFacts, Untrusted>,
) -> Result<(), Untrusted> {
    let parent = Path::new(socket)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let facts = read(parent)?;
    check_socket_folder(facts.uid, facts.mode, expected_uid)
}

/// How one call into the OS that fills a buffer ended.
enum Fill {
    /// This many entries were written.
    Done(usize),
    /// The buffer was too small; this many entries are needed.
    Need(usize),
    /// The call failed for another reason.
    Failed,
}

/// Calls `call` with a buffer of `initial` entries, and once more with the size it asked for. `None` when it fails or
/// still does not fit (the groups changed between the calls).
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
fn fill<T: Copy + Default>(
    initial: usize,
    mut call: impl FnMut(&mut [T]) -> Fill,
) -> Option<Vec<T>> {
    let mut buf = vec![T::default(); initial];
    for _ in 0..2 {
        match call(&mut buf) {
            Fill::Done(n) => {
                buf.truncate(n);
                return Some(buf);
            }
            Fill::Need(n) => buf = vec![T::default(); n.clamp(1, 4096)],
            Fill::Failed => return None,
        }
    }
    None
}

#[cfg(target_os = "linux")]
mod os {
    use super::*;

    /// `SO_PEERGROUPS` and `SO_PEERPIDFD` (Linux 4.13 and 6.5) as numbers: older `libc` versions lack the names.
    const SO_PEERGROUPS: libc::c_int = 59;
    const SO_PEERPIDFD: libc::c_int = 77;

    pub(in super::super) fn raw_peer(s: &UnixStream) -> Result<RawPeer, PeerError> {
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
        if rc != 0 {
            return Err(unreadable("SO_PEERCRED"));
        }
        let groups = groups(s)?;
        Ok(RawPeer {
            account: AccountId::Uid(cred.uid),
            admin: AdminEvidence::Groups(groups),
            audit: AuditHint::new(u32::try_from(cred.pid).ok(), program_through_pidfd(s)),
        })
    }

    /// `SO_PEERGROUPS`: the supplementary groups the peer had at `connect`. A buffer that is too small answers `ERANGE`
    /// with the size needed.
    fn groups(s: &UnixStream) -> Result<Vec<u32>, PeerError> {
        fill(32, |buf: &mut [u32]| {
            let mut len = (buf.len() * 4) as libc::socklen_t;
            // SAFETY: `buf` holds `len` bytes; the fd is open for the call.
            let rc = unsafe {
                libc::getsockopt(
                    s.as_raw_fd(),
                    libc::SOL_SOCKET,
                    SO_PEERGROUPS,
                    buf.as_mut_ptr().cast(),
                    &mut len,
                )
            };
            if rc == 0 {
                Fill::Done(len as usize / 4)
            } else if io::Error::last_os_error().raw_os_error() == Some(libc::ERANGE) {
                Fill::Need(len as usize / 4)
            } else {
                Fill::Failed
            }
        })
        .ok_or_else(|| unreadable("SO_PEERGROUPS"))
    }

    /// The caller's program, for the audit line only. The kernel pinned the process at `connect` (`SO_PEERPIDFD`); the pid is
    /// read from the descriptor's fdinfo and `/proc/<pid>/exe` is read **while the descriptor is open**, then the process is
    /// confirmed to be still there by signalling it through the descriptor (signal 0). The pid names that process for as long as
    /// the descriptor exists, but only while it has not exited: once it is gone the number can be handed on, so an entry read
    /// from a gone process is dropped (`None`) rather than risk another program's path in the log. `None` on kernels before 6.5
    /// (no `SO_PEERPIDFD`) or when `/proc` is hidden.
    fn program_through_pidfd(s: &UnixStream) -> Option<String> {
        let mut fd: libc::c_int = -1;
        let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: `fd` and `len` are valid for writes; the returned descriptor is closed below.
        let rc = unsafe {
            libc::getsockopt(
                s.as_raw_fd(),
                libc::SOL_SOCKET,
                SO_PEERPIDFD,
                (&mut fd as *mut libc::c_int).cast(),
                &mut len,
            )
        };
        if rc != 0 || fd < 0 {
            return None;
        }
        let program = (|| {
            let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).ok()?;
            let pid = info
                .lines()
                .find_map(|l| l.strip_prefix("Pid:")?.trim().parse::<u32>().ok())?;
            let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
            // SAFETY: `pidfd_send_signal(fd, 0, NULL, 0)` only asks whether the process behind the descriptor exists.
            let alive = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    fd,
                    0,
                    std::ptr::null::<libc::siginfo_t>(),
                    0u32,
                )
            } == 0;
            alive.then(|| exe.to_string_lossy().into_owned())
        })();
        // SAFETY: `fd` came from the kernel above and is closed exactly once.
        unsafe {
            libc::close(fd);
        }
        program
    }
}

#[cfg(target_os = "macos")]
mod os {
    use super::*;

    /// `LOCAL_PEERTOKEN` from `<sys/un.h>`.
    const LOCAL_PEERTOKEN: libc::c_int = 0x006;

    unsafe extern "C" {
        /// `proc_pidpath_audittoken` from `<libproc.h>`: the path of the process the audit token names (token-checked,
        /// not pid-based).
        fn proc_pidpath_audittoken(
            token: *const [u32; 8],
            buffer: *mut libc::c_void,
            size: u32,
        ) -> libc::c_int;
    }

    pub(in super::super) fn raw_peer(s: &UnixStream) -> Result<RawPeer, PeerError> {
        let mut token = [0u32; 8];
        let mut len = std::mem::size_of_val(&token) as libc::socklen_t;
        // SAFETY: `token` and `len` are valid for writes of the sizes passed; the fd is open for the call.
        let rc = unsafe {
            libc::getsockopt(
                s.as_raw_fd(),
                libc::SOL_LOCAL,
                LOCAL_PEERTOKEN,
                token.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if rc != 0 || len as usize != std::mem::size_of_val(&token) {
            return Err(unreadable("LOCAL_PEERTOKEN"));
        }
        // audit_token_t: auid, euid, egid, ruid, rgid, pid, asid, pidversion.
        let (euid, egid, pid) = (token[1], token[2], token[5]);
        Ok(RawPeer {
            account: AccountId::Uid(euid),
            admin: AdminEvidence::Groups(groups_of(euid, egid)),
            audit: AuditHint::new(Some(pid), program_of(&token)),
        })
    }

    /// The groups of the account `uid`, by the account (never by pid). An account without a passwd entry, or whose groups
    /// cannot be listed, has only its primary group: fewer groups can only mean less privilege.
    pub(super) fn groups_of(uid: u32, gid: u32) -> Vec<u32> {
        let Some(name) = user_name(uid) else {
            return vec![gid];
        };
        fill(64, |buf: &mut [libc::c_int]| {
            let mut n = buf.len() as libc::c_int;
            // SAFETY: `name` is NUL-terminated; `buf` holds `n` entries.
            let rc = unsafe {
                libc::getgrouplist(name.as_ptr(), gid as libc::c_int, buf.as_mut_ptr(), &mut n)
            };
            grouplist_result(rc, n, buf.len())
        })
        .map_or_else(|| vec![gid], |g| g.into_iter().map(|g| g as u32).collect())
    }

    /// What `getgrouplist` answered: `rc >= 0` means `n` groups were written; `-1` with a larger `n` means the buffer
    /// (`capacity` entries) was too small; anything else is a failure.
    pub(super) fn grouplist_result(rc: libc::c_int, n: libc::c_int, capacity: usize) -> Fill {
        let n = usize::try_from(n).unwrap_or(0);
        if rc >= 0 {
            Fill::Done(n)
        } else if n > capacity {
            Fill::Need(n)
        } else {
            Fill::Failed
        }
    }

    /// The account name for `uid` (`getpwuid_r`), if it has a passwd entry.
    fn user_name(uid: u32) -> Option<std::ffi::CString> {
        // SAFETY: an all-zero `passwd` is a valid out value for `getpwuid_r`.
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0 as libc::c_char; 4096];
        // SAFETY: all pointers are valid for the sizes passed; `result` points into `pw` or is null.
        let rc =
            unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc != 0 || result.is_null() {
            return None;
        }
        // SAFETY: `pw_name` is a NUL-terminated string inside `buf`.
        Some(unsafe { std::ffi::CStr::from_ptr(pw.pw_name) }.to_owned())
    }

    fn program_of(token: &[u32; 8]) -> Option<String> {
        let mut buf = vec![0u8; 4096];
        // SAFETY: `token` is a valid audit token and `buf` holds the size passed.
        let n =
            unsafe { proc_pidpath_audittoken(token, buf.as_mut_ptr().cast(), buf.len() as u32) };
        let n = usize::try_from(n).ok().filter(|n| *n > 0)?;
        buf.truncate(n);
        String::from_utf8(buf).ok()
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod os {
    use super::*;

    pub(in super::super) fn raw_peer(_: &UnixStream) -> Result<RawPeer, PeerError> {
        Err(PeerError::Unreadable {
            what: "peer credentials",
            cause: "this Unix reports none to Paddock".into(),
        })
    }
}

pub(super) use os::raw_peer;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn facts(uid: u32, mode: u32) -> impl FnOnce(&Path) -> Result<FolderFacts, Untrusted> {
        move |_| Ok(FolderFacts { uid, mode })
    }

    #[test]
    fn the_folder_decision_uses_the_folder_read_from_the_handle() {
        assert!(check_folder_with("/var/run/x/h.sock", 0, facts(0, 0o755)).is_ok());
        assert_eq!(
            check_folder_with("/var/run/x/h.sock", 0, facts(7, 0o755)),
            Err(Untrusted::FolderOwner { uid: 7 })
        );
        assert_eq!(
            check_folder_with("/var/run/x/h.sock", 0, facts(0, 0o777)),
            Err(Untrusted::FolderWritable { mode: 0o777 })
        );
        // The folder is the socket's parent, or the current folder for a bare name.
        let seen = Cell::new(String::new());
        let record = |p: &Path| {
            seen.set(p.to_string_lossy().into_owned());
            Ok(FolderFacts {
                uid: 0,
                mode: 0o755,
            })
        };
        check_folder_with("/var/run/x/h.sock", 0, record).unwrap();
        assert_eq!(seen.take(), "/var/run/x");
        check_folder_with("h.sock", 0, record).unwrap();
        assert_eq!(seen.take(), ".");
        // A folder that cannot be read is a refusal that says what failed.
        let broken = |_: &Path| {
            Err(untrusted_io("fstat the socket's folder")(io::Error::other(
                "EIO",
            )))
        };
        let e = check_folder_with("/a/b", 0, broken).unwrap_err();
        assert!(e.to_string().contains("fstat") && e.to_string().contains("EIO"));
    }

    #[test]
    fn a_folder_is_read_by_handle_and_a_file_or_a_missing_folder_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let f = read_folder(dir.path()).unwrap();
        assert_eq!(f.uid, std::fs::metadata(dir.path()).unwrap().uid());
        assert!(matches!(
            read_folder(&dir.path().join("nope")),
            Err(Untrusted::Unreadable { .. })
        ));
        let file = dir.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(
            read_folder(&file),
            Err(Untrusted::Unreadable { .. })
        ));
    }

    #[test]
    fn a_buffer_call_may_ask_once_for_more_room() {
        let calls = Cell::new(0);
        let grow = |buf: &mut [u32]| {
            calls.set(calls.get() + 1);
            if buf.len() < 100 {
                Fill::Need(100)
            } else {
                buf[0] = 7;
                Fill::Done(1)
            }
        };
        assert_eq!(fill(8, grow), Some(vec![7]));
        assert_eq!(calls.get(), 2);
        assert_eq!(fill(8, |_: &mut [u32]| Fill::Failed), None);
        // Still too small after the second call: give up, never loop.
        assert_eq!(fill(8, |_: &mut [u32]| Fill::Need(0)), None);
    }

    #[test]
    fn a_descriptor_that_is_not_a_socket_gives_no_facts() {
        let not_a_socket = UnixStream::from(std::os::fd::OwnedFd::from(
            std::fs::File::open("/dev/null").unwrap(),
        ));
        let e = raw_peer(&not_a_socket).unwrap_err();
        assert!(e.to_string().contains("refused"), "{e}");
        assert!(matches!(
            unreadable("x"),
            PeerError::Unreadable { what: "x", .. }
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn groups_come_from_the_account_and_an_unknown_account_has_its_primary_group() {
        // SAFETY: plain getters.
        #[allow(unsafe_code)]
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        assert!(os::groups_of(uid, gid).contains(&gid));
        assert_eq!(os::groups_of(u32::MAX - 7, 4242), vec![4242]);
        assert!(matches!(os::grouplist_result(3, 3, 64), Fill::Done(3)));
        assert!(matches!(os::grouplist_result(-1, 90, 64), Fill::Need(90)));
        assert!(matches!(os::grouplist_result(-1, 64, 64), Fill::Failed));
        assert!(matches!(os::grouplist_result(-1, -5, 64), Fill::Failed));
    }
}
