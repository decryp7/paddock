//! Whether a macOS process runs in the background (efficiency) class. Read-only.

use dyno_core::process::Pid;

/// bit 0x10000 (xnu `PROC_FLAG_EXT_DARWINBG`) of `proc_bsdinfo.pbi_flags`: the process runs in the background class.
const PROC_FLAG_DARWINBG: u32 = 0x0001_0000;

/// Whether `pid` runs in the background (efficiency) class; `None` when the kernel won't say.
/// (`getpriority(PRIO_DARWIN_PROCESS)` does not report it on current macOS; the BSD info flags do.)
pub fn is_background(pid: Pid) -> Option<bool> {
    // SAFETY: `proc_bsdinfo` is plain data, zero is a valid bit pattern; the buffer size passed is its exact size.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: `info` is valid for `size` bytes for the call's duration.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (got == size).then_some(info.pbi_flags & PROC_FLAG_DARWINBG != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_not_in_the_background_class() {
        assert_eq!(is_background(std::process::id()), Some(false));
    }
}
