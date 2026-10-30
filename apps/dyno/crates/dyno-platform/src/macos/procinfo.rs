//! What macOS still tells about processes of other users when the full BSD info is refused: `PROC_PIDT_SHORTBSDINFO` gives
//! the owner and the parent, and `sysctl kern.proc.pid` the start time. Read-only.

use dyno_core::process::Pid;

/// The owner and the parent of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortInfo {
    /// The user id the process runs as.
    pub uid: u32,
    /// The parent's pid.
    pub parent: Pid,
}

/// The short BSD info of `pid`; `None` when the kernel won't say or the process is gone.
// ADR 0033 WP-9: the parent pid is shown in the process tree without a start-time check (display only).
pub fn short_info(pid: Pid) -> Option<ShortInfo> {
    // SAFETY: `proc_bsdshortinfo` is plain data for which zero is a valid bit pattern.
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int;
    // SAFETY: `info` is valid for `size` bytes for the duration of the call.
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdshortinfo).cast(),
            size,
        )
    };
    (got == size && info.pbsi_pid == pid).then_some(ShortInfo {
        uid: info.pbsi_uid,
        parent: info.pbsi_ppid,
    })
}

/// Size of the kernel's `kinfo_proc` on 64-bit macOS.
const KINFO_PROC_SIZE: usize = 648;
/// Offset of `p_pid` inside it (after the 16-byte start time, two pointers, the flags and the state).
const OFF_PID: usize = 40;

/// The start time (seconds since the Unix epoch) from the bytes of a `kinfo_proc` answer for `pid`. The first field is the
/// start time's seconds; the pid in the record must be the one asked for.
pub fn start_secs_from_kinfo(bytes: &[u8], pid: Pid) -> Option<u64> {
    let secs = i64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
    let got = i32::from_le_bytes(bytes.get(OFF_PID..OFF_PID + 4)?.try_into().ok()?);
    (got == i32::try_from(pid).ok()? && secs > 0).then_some(secs as u64)
}

/// The start time of `pid` from `sysctl kern.proc.pid`; `None` when the kernel won't say.
pub fn start_secs(pid: Pid) -> Option<u64> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_PID, pid as libc::c_int];
    let mut buf = vec![0u8; KINFO_PROC_SIZE];
    let mut len = buf.len();
    // SAFETY: `mib` has 4 entries as passed; `buf` is valid for `len` bytes and no new value is set.
    let r = unsafe { libc::sysctl(mib.as_mut_ptr(), 4, buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) };
    (r == 0 && len >= OFF_PID + 4)
        .then(|| start_secs_from_kinfo(&buf[..len], pid))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_read_back_by_its_short_info_and_start_time() {
        let me = std::process::id();
        let info = short_info(me).expect("own short info");
        assert_eq!(info.uid, unsafe { libc::getuid() });
        assert_eq!(info.parent, std::os::unix::process::parent_id());
        let started = start_secs(me).expect("own start time");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(started > 1_000_000_000 && started <= now);
    }

    #[test]
    fn a_pid_that_does_not_exist_gives_nothing() {
        assert_eq!(short_info(u32::MAX >> 1), None);
        assert_eq!(start_secs(u32::MAX >> 1), None);
    }

    #[test]
    fn the_start_time_is_taken_from_the_record_only_when_it_is_the_asked_process() {
        let mut rec = vec![0u8; KINFO_PROC_SIZE];
        rec[..8].copy_from_slice(&1_700_000_000i64.to_le_bytes());
        rec[OFF_PID..OFF_PID + 4].copy_from_slice(&321i32.to_le_bytes());
        assert_eq!(start_secs_from_kinfo(&rec, 321), Some(1_700_000_000));
        assert_eq!(start_secs_from_kinfo(&rec, 322), None);
        assert_eq!(start_secs_from_kinfo(&rec[..20], 321), None);
        rec[..8].copy_from_slice(&0i64.to_le_bytes());
        assert_eq!(start_secs_from_kinfo(&rec, 321), None);
        assert_eq!(start_secs_from_kinfo(&rec, u32::MAX), None);
    }
}
