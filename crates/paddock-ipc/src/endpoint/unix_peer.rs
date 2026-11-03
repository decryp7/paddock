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

/// Checks the folder that holds the socket `socket` by handle (no symlink on the last component): it must belong to
/// `expected_uid` and have no group or world write bit.
pub(super) fn check_folder_of(socket: &str, expected_uid: u32) -> Result<(), Untrusted> {
    let parent = Path::new(socket)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let folder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(parent)
        .map_err(|e| Untrusted::Unreadable {
            what: "open the socket's folder",
            cause: e.to_string(),
        })?;
    let meta = folder.metadata().map_err(|e| Untrusted::Unreadable {
        what: "fstat the socket's folder",
        cause: e.to_string(),
    })?;
    check_socket_folder(meta.uid(), meta.mode(), expected_uid)
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
        let mut buf = vec![0u32; 32];
        for _ in 0..2 {
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
                buf.truncate(len as usize / 4);
                return Ok(buf);
            }
            if io::Error::last_os_error().raw_os_error() != Some(libc::ERANGE) {
                break;
            }
            buf = vec![0u32; (len as usize / 4).clamp(1, 4096)];
        }
        Err(unreadable("SO_PEERGROUPS"))
    }

    /// The caller's program, for the audit line only: through the pidfd the kernel pinned at `connect`, so the entry cannot
    /// have moved to another process. `None` on kernels before 6.5 (no `SO_PEERPIDFD`) or when `/proc` is hidden.
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
        let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}"));
        // SAFETY: `fd` came from the kernel above and is closed exactly once.
        unsafe {
            libc::close(fd);
        }
        let pid = info
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("Pid:")?.trim().parse::<u32>().ok())?;
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
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

    /// The groups of the account `uid`, by the account (never by pid). An account without a passwd entry has only its
    /// primary group: fewer groups can only mean less privilege.
    fn groups_of(uid: u32, gid: u32) -> Vec<u32> {
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0 as libc::c_char; 4096];
        // SAFETY: all pointers are valid for the sizes passed; `result` points into `pw` or is null.
        let rc =
            unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
        if rc != 0 || result.is_null() {
            return vec![gid];
        }
        let mut groups = vec![0 as libc::c_int; 64];
        for _ in 0..2 {
            let mut n = groups.len() as libc::c_int;
            // SAFETY: `pw.pw_name` is a NUL-terminated string in `buf`; `groups` holds `n` entries.
            let rc = unsafe {
                libc::getgrouplist(pw.pw_name, gid as libc::c_int, groups.as_mut_ptr(), &mut n)
            };
            if rc >= 0 {
                groups.truncate(usize::try_from(n).unwrap_or(0));
                return groups.into_iter().map(|g| g as u32).collect();
            }
            groups = vec![0; usize::try_from(n).unwrap_or(0).clamp(1, 4096)];
        }
        vec![gid]
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
