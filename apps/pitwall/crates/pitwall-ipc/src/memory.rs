//! The memory cap of a plugin on Unix (on Windows the job object enforces it): the host samples the resident memory
//! of the plugin process and stops it once it is over its cap. The decision is the pure [`over_cap`]; reading the
//! memory of a process is [`resident_bytes`] (Linux: `/proc/<pid>/statm`; macOS: `proc_pidinfo`).

/// How often the host looks at the memory of a capped plugin.
pub const SAMPLE_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

/// Whether a process using `used` bytes is over its cap `limit`.
pub fn over_cap(used: u64, limit: u64) -> bool {
    used > limit
}

/// "512 MB": a memory size for messages.
pub fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes.div_ceil(1 << 20))
}

/// The resident memory of process `pid` in bytes, `None` when it cannot be read (it exited, or the platform has no
/// way to ask).
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub fn resident_bytes(pid: u32) -> Option<u64> {
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // SAFETY: sysconf takes a plain integer and has no memory effects.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    Some(pages.saturating_mul(u64::try_from(page).ok().filter(|p| *p > 0).unwrap_or(4096)))
}

/// The resident memory of process `pid` in bytes, `None` when it cannot be read.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn resident_bytes(pid: u32) -> Option<u64> {
    let mut info: libc::proc_taskinfo = unsafe {
        // SAFETY: `proc_taskinfo` is plain old data; all zeroes is a valid value.
        std::mem::zeroed()
    };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a valid, writable buffer of `size` bytes for the PROC_PIDTASKINFO flavour.
    let n = unsafe {
        libc::proc_pidinfo(
            libc::c_int::try_from(pid).ok()?,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    (n == size).then_some(info.pti_resident_size)
}

/// The resident memory of process `pid` in bytes (not read on this platform: the job object caps it on Windows).
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn resident_bytes(_pid: u32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cap_and_its_message() {
        assert!(!over_cap(512 << 20, 512 << 20), "at the cap is fine");
        assert!(over_cap((512 << 20) + 1, 512 << 20));
        assert!(over_cap(1, 0));
        assert_eq!(megabytes(512 << 20), "512 MB");
        assert_eq!(megabytes(1), "1 MB");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn this_process_has_resident_memory() {
        assert!(resident_bytes(std::process::id()).is_some_and(|b| b > 0));
        assert_eq!(resident_bytes(u32::MAX), None);
    }
}
