//! Open files and mapped libraries of a process through `libproc`, **without taking a reference on a socket, pipe or
//! kqueue descriptor** (Dyno ADR 0026). `lsof` fetches information for every descriptor it meets, sockets and pipes
//! included (`dproc.c` `process_socket` / `process_pipe`), which can mark another program's socket `SS_DRAINING`.
//! Here the descriptors are only *listed* (`PROC_PIDLISTFDS` reads the table and holds nothing), and information is
//! asked only for vnode (file) descriptors, whose file operations have no drain (`vfs_vnops.c`: `.fo_drain =
//! fo_no_drain`; sockets, pipes and kqueues have `soo_drain`, `pipe_drain`, `kqueue_drain`).
//!
//! Why asking about a descriptor that is not a vnode any more is also harmless (XNU `kern_descrip.c`): the listing and the
//! question are two steps, and the number can be reused by a socket in between. `fp_get_ftype` checks the descriptor's
//! type under `proc_fdlock` *before* it takes the I/O reference (`os_ref_retain_locked` on `fp_iocount`), and returns
//! `ENOTSUP` for another type without taking one. So a query meant for a vnode can briefly delay the target's `close()`
//! (it holds the lock for a moment), but never takes a reference on a socket and so never causes a drain or an error.

use std::ffi::c_void;

use libc::{PROC_PIDLISTFDS, PROX_FDTYPE_VNODE};

const PROC_PIDREGIONPATHINFO: libc::c_int = 8;
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;
/// In `vnode_fdinfowithpath`: a 24-byte `proc_fileinfo`, then `vinfo_stat` whose `vst_dev` (u32) is at 0 and `vst_ino`
/// (u64) at 8.
const VNODE_DEV: usize = 24;
const VNODE_INO: usize = 32;
const PATH_BYTES: usize = 1024;
/// `sizeof(struct proc_regionwithpathinfo)`: 96 bytes of `proc_regioninfo`, then a `vnode_info_path` (152 + 1024).
const REGION_INFO_BYTES: usize = 96;
/// Offsets in `proc_regioninfo`: `pri_address`, `pri_size`.
const REGION_ADDRESS: usize = 80;
const REGION_SIZE: usize = 88;
/// Bounds: descriptors read per process, regions walked per process, pids listed.
const MAX_FDS: usize = 1 << 16;
const MAX_REGIONS: usize = 20_000;
const MAX_PIDS: usize = 1 << 17;

/// Every pid the caller may list.
pub fn pids() -> Vec<u32> {
    let mut buf = vec![0 as libc::pid_t; 4096];
    loop {
        let bytes = i32::try_from(buf.len() * std::mem::size_of::<libc::pid_t>()).unwrap_or(i32::MAX);
        // SAFETY: `buf` is `bytes` writable bytes; the call writes at most that many.
        let n = unsafe { libc::proc_listallpids(buf.as_mut_ptr().cast::<c_void>(), bytes) };
        let n = usize::try_from(n).unwrap_or(0);
        if n >= buf.len() && buf.len() < MAX_PIDS {
            buf.resize(buf.len() * 2, 0);
            continue;
        }
        return buf
            .iter()
            .take(n)
            .filter_map(|p| u32::try_from(*p).ok())
            .filter(|p| *p != 0)
            .collect();
    }
}

/// The descriptors of `pid` as (number, type), read from the table only. Empty when the process is gone or not ours.
pub fn fds(pid: u32) -> Vec<(i32, u32)> {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return Vec::new() };
    // SAFETY: a null buffer asks for the size only.
    let need = unsafe { libc::proc_pidinfo(pid, PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    let Ok(need) = usize::try_from(need) else { return Vec::new() };
    let each = std::mem::size_of::<libc::proc_fdinfo>();
    let room = (need / each + 16).min(MAX_FDS);
    let mut buf: Vec<libc::proc_fdinfo> = Vec::with_capacity(room);
    // SAFETY: `buf` has room for `room` entries; the kernel writes whole entries up to the size given and returns the bytes written.
    let got = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDLISTFDS,
            0,
            buf.as_mut_ptr().cast::<c_void>(),
            i32::try_from(room * each).unwrap_or(i32::MAX),
        )
    };
    let n = (usize::try_from(got).unwrap_or(0) / each).min(room);
    // SAFETY: the first `n` entries were written by the call above.
    unsafe { buf.set_len(n) };
    buf.iter().map(|f| (f.proc_fd, f.proc_fdtype)).collect()
}

/// The short name of a process (`proc_name`).
pub fn name(pid: u32) -> Option<String> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is 256 writable bytes; the call writes at most that many and returns the length.
    let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast::<c_void>(), 256) };
    let n = usize::try_from(n).ok().filter(|n| *n > 0)?;
    Some(String::from_utf8_lossy(&buf[..n.min(256)]).into_owned())
}

/// Whether a descriptor type from [`fds`] is a file (vnode).
pub fn is_vnode(fd_type: u32) -> bool {
    i64::from(fd_type) == i64::from(PROX_FDTYPE_VNODE)
}

/// What a vnode descriptor is open on: its path (as the kernel reports it) and its identity (device and inode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VnodeInfo {
    /// The path.
    pub path: String,
    /// `vst_dev`.
    pub dev: u32,
    /// `vst_ino`.
    pub ino: u64,
}

/// The file a vnode descriptor is open on. Only for descriptors [`fds`] reported as vnodes; `None` when it closed meanwhile.
pub fn vnode_info(pid: u32, fd: i32) -> Option<VnodeInfo> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    let mut buf = vec![0u8; 2048];
    // SAFETY: `buf` is 2048 writable bytes, larger than `struct vnode_fdinfowithpath` (1200); the call returns the bytes written.
    let got = unsafe { libc::proc_pidfdinfo(pid, fd, PROC_PIDFDVNODEPATHINFO, buf.as_mut_ptr().cast::<c_void>(), 2048) };
    let got = usize::try_from(got).ok().filter(|g| *g > PATH_BYTES)?;
    let path = path_at_end(&buf[..got])?;
    let dev = u32::from_le_bytes(buf.get(VNODE_DEV..VNODE_DEV + 4)?.try_into().ok()?);
    let ino = u64::from_le_bytes(buf.get(VNODE_INO..VNODE_INO + 8)?.try_into().ok()?);
    Some(VnodeInfo { path, dev, ino })
}

/// The path of a vnode descriptor (see [`vnode_info`]).
pub fn vnode_path(pid: u32, fd: i32) -> Option<String> {
    vnode_info(pid, fd).map(|v| v.path)
}

/// The path is the last field of both `vnode_fdinfowithpath` and `proc_regionwithpathinfo`: its last 1024 bytes.
fn path_at_end(info: &[u8]) -> Option<String> {
    let field = info.get(info.len().checked_sub(PATH_BYTES)?..)?;
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    let s = String::from_utf8_lossy(&field[..end]).into_owned();
    (!s.is_empty()).then_some(s)
}

/// The files `pid` has mapped (its program and libraries), by walking its memory regions. Bounded; empty when the
/// process is gone or not ours.
pub fn mapped_files(pid: u32) -> Vec<String> {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    let mut address: u64 = 0;
    let mut buf = vec![0u8; 2048];
    for _ in 0..MAX_REGIONS {
        // SAFETY: `buf` is 2048 writable bytes, larger than `struct proc_regionwithpathinfo` (1272).
        let got = unsafe { libc::proc_pidinfo(pid, PROC_PIDREGIONPATHINFO, address, buf.as_mut_ptr().cast::<c_void>(), 2048) };
        let Some(got) = usize::try_from(got).ok().filter(|g| *g >= REGION_INFO_BYTES + PATH_BYTES) else {
            break;
        };
        let rd = |at: usize| u64::from_le_bytes(buf[at..at + 8].try_into().unwrap_or([0; 8]));
        let (start, size) = (rd(REGION_ADDRESS), rd(REGION_SIZE));
        if let Some(p) = path_at_end(&buf[..got]) {
            if out.last() != Some(&p) && !out.contains(&p) {
                out.push(p);
            }
        }
        let next = start.saturating_add(size.max(1));
        if next <= address {
            break;
        }
        address = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn this_process_lists_its_own_open_file_by_path_and_its_program_as_mapped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("held.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"x").unwrap();
        let me = std::process::id();
        let canonical = path.canonicalize().unwrap().to_string_lossy().into_owned();
        let found = fds(me)
            .into_iter()
            .filter(|(_, t)| is_vnode(*t))
            .filter_map(|(fd, _)| vnode_path(me, fd))
            .any(|p| p == canonical);
        assert!(found, "the open file is listed by path");
        // Its identity is the one the file system reports, also through a hard link.
        let link = dir.path().join("link.txt");
        std::fs::hard_link(&path, &link).unwrap();
        let meta = std::fs::metadata(&link).unwrap();
        // (With two names for one file the kernel may report either as the path: why lockers match on identity.)
        use std::os::unix::fs::MetadataExt;
        let want = (meta.dev() as u32, meta.ino());
        let held = fds(me)
            .into_iter()
            .filter(|(_, t)| is_vnode(*t))
            .filter_map(|(fd, _)| vnode_info(me, fd))
            .find(|v| (v.dev, v.ino) == want);
        assert!(held.is_some(), "the descriptor is found by device and inode");
        assert!(pids().contains(&me));
        let exe = std::env::current_exe()
            .unwrap()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(mapped_files(me).contains(&exe), "the program file is among the mapped files");
    }

    #[test]
    fn a_path_field_is_cut_at_its_first_zero_and_an_empty_one_is_none() {
        let mut info = vec![7u8; 200];
        let mut field = vec![0u8; PATH_BYTES];
        field[..4].copy_from_slice(b"/a/b");
        info.extend_from_slice(&field);
        assert_eq!(path_at_end(&info).as_deref(), Some("/a/b"));
        assert_eq!(path_at_end(&vec![0u8; PATH_BYTES + 8]), None);
        assert_eq!(path_at_end(&[1, 2, 3]), None);
    }

    #[test]
    fn a_pid_that_does_not_exist_gives_nothing() {
        assert!(fds(0x7fff_fff0).is_empty());
        assert!(mapped_files(0x7fff_fff0).is_empty());
        assert_eq!(vnode_path(0x7fff_fff0, 3), None);
    }
}
