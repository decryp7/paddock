//! macOS' share of process control: efficiency mode is the background QoS class (`setpriority(PRIO_DARWIN_PROCESS,
//! PRIO_DARWIN_BG)`, what `taskpolicy -b` does), and there is no CPU affinity.

use crate::unix_control::OsControl;
use dyno_core::ports::PlatformError;
use dyno_core::process::Pid;

const PRIO_DARWIN_PROCESS: libc::c_int = 4;
const PRIO_DARWIN_BG: libc::c_int = 0x1000;

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

/// macOS specifics of process control.
pub struct MacOsControl;

impl OsControl for MacOsControl {
    fn set_efficiency(&mut self, pid: Pid, on: bool) -> Result<(), PlatformError> {
        let value = if on { PRIO_DARWIN_BG } else { 0 };
        // SAFETY: plain integers; failure is reported through errno.
        let r = unsafe { libc::setpriority(PRIO_DARWIN_PROCESS, pid as libc::id_t, value) };
        if r == 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::ESRCH) => Err(PlatformError::NotFound(format!("Process {pid} has exited."))),
            Some(libc::EPERM) | Some(libc::EACCES) => Err(PlatformError::AccessDenied(
                "This process belongs to another user or the system; changing it needs the helper (administrator rights).".into(),
            )),
            _ => Err(PlatformError::Failed(format!("Could not change efficiency mode: {e}"))),
        }
    }

    fn set_affinity(&mut self, _pid: Pid, _mask: u64) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "macOS has no CPU affinity: the system decides which cores run a process.".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn efficiency_mode_toggles_background_priority_on_a_dummy() {
        let mut child = crate::process::command("sleep").arg("30").stdout(Stdio::null()).spawn().unwrap();
        let mut c = MacOsControl;
        c.set_efficiency(child.id(), true).unwrap();
        assert_eq!(is_background(child.id()), Some(true));
        c.set_efficiency(child.id(), false).unwrap();
        assert_eq!(is_background(child.id()), Some(false));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn affinity_is_unsupported_with_a_reason() {
        let e = MacOsControl.set_affinity(1, 1).unwrap_err();
        assert!(matches!(e, PlatformError::Unsupported(m) if m.contains("no CPU affinity")));
    }

    #[test]
    fn a_missing_process_is_not_found() {
        let mut child = crate::process::command("true").spawn().unwrap();
        child.wait().unwrap();
        assert!(matches!(
            MacOsControl.set_efficiency(child.id(), true),
            Err(PlatformError::NotFound(_))
        ));
        assert_eq!(is_background(child.id()), None);
    }
}
