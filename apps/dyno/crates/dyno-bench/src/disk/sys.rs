//! The few operating-system calls the disk test needs that `std` does not have: no page cache for a file (macOS `F_NOCACHE`),
//! a full flush to the device (macOS `F_FULLFSYNC`), dropping cached pages (Linux `posix_fadvise`) and the free space of a
//! volume. Each is small and checked; everything else is safe `std`.
#![allow(unsafe_code)]

use std::fs::File;
use std::path::Path;

/// Asks the OS not to cache this file's reads and writes (macOS: `F_NOCACHE`; Linux and Windows get theirs at `open`).
#[cfg(target_os = "macos")]
pub fn no_cache(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor is open for the duration of the call; F_NOCACHE takes an int.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_NOCACHE, 1) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Not needed here: direct I/O is asked for when the file is opened.
#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps)]
pub fn no_cache(_file: &File) -> std::io::Result<()> {
    Ok(())
}

/// Flushes to the device, not only to the drive's cache where the OS can say so (macOS: `F_FULLFSYNC`, falling back to `fsync`
/// where the file system refuses it).
pub fn full_sync(file: &File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor is open for the duration of the call; F_FULLFSYNC takes no argument.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } != -1 {
            return Ok(());
        }
    }
    file.sync_all()
}

/// Drops this file's cached pages after a flush (Linux, when direct I/O was refused); elsewhere nothing to do.
pub fn drop_cache(file: &File) {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor is open; offset 0 and length 0 mean the whole file. A failure only means pages stay cached,
        // which the labels already say.
        let _ = unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    }
    #[cfg(not(target_os = "linux"))]
    let _ = file;
}

/// The alignment a direct-I/O file wants for buffers and offsets, as the kernel reports it (Linux `statx` with `STATX_DIOALIGN`,
/// kernel 6.1 and later). `None` when it does not say (older kernels, other file systems, other OSes): 4 KiB is then assumed, which
/// covers every logical sector size in use.
pub fn dio_align(file: &File) -> Option<(u32, u32)> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: an all-zero statx is a valid value to be overwritten.
        let mut sx: libc::statx = unsafe { std::mem::zeroed() };
        // SAFETY: the descriptor is open; the empty path with AT_EMPTY_PATH means the descriptor itself; `sx` is valid for writing.
        let rc = unsafe { libc::statx(file.as_raw_fd(), c"".as_ptr(), libc::AT_EMPTY_PATH, libc::STATX_DIOALIGN, &mut sx) };
        (rc == 0 && sx.stx_mask & libc::STATX_DIOALIGN != 0 && sx.stx_dio_mem_align != 0)
            .then_some((sx.stx_dio_mem_align, sx.stx_dio_offset_align))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        None
    }
}

/// Whether the reported alignment is one the test's 4 KiB buffers and block sizes satisfy (4 KiB or finer, powers of two).
pub fn align_fits(mem: u32, offset: u32) -> bool {
    mem.is_power_of_two() && offset.is_power_of_two() && mem <= 4096 && offset <= 4096
}

/// Capacity and free bytes of the volume holding `dir`; `None` where the OS will not say (or on Windows, where the runner
/// leaves the check to the agent's pre-flight and to the disk-full error).
pub fn volume_space(dir: &Path) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        // SAFETY: an all-zero statvfs is a valid value to be overwritten.
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` and `s` are valid for the duration of the call.
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
            return None;
        }
        #[allow(clippy::useless_conversion)]
        let unit = u64::from(s.f_frsize);
        #[allow(clippy::useless_conversion)]
        Some((
            u64::from(s.f_blocks).saturating_mul(unit),
            u64::from(s.f_bavail).saturating_mul(unit),
        ))
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_kib_buffers_fit_every_alignment_up_to_four_kib_and_nothing_beyond() {
        assert!(align_fits(512, 512) && align_fits(4, 512) && align_fits(4096, 4096));
        assert!(!align_fits(8192, 4096), "a larger memory alignment than our buffers have");
        assert!(!align_fits(4096, 65_536), "a larger offset alignment than our blocks have");
        assert!(!align_fits(0, 512) && !align_fits(3, 512), "not a power of two");
    }
}
