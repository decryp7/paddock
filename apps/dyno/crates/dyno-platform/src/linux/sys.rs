//! The only unsafe code of the Linux adapters: three tiny system calls. Each block says why it is sound.
#![allow(unsafe_code)]

use dyno_core::ports::PlatformError;

/// The I/O scheduling class "idle": the process gets disk time only when nobody else wants it.
const IOPRIO_CLASS_IDLE: i32 = 3;
const IOPRIO_CLASS_SHIFT: i32 = 13;
const IOPRIO_WHO_PROCESS: i32 = 1;

fn failure(what: &str) -> PlatformError {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => PlatformError::NotFound("The process (or one of its threads) has exited.".into()),
        Some(libc::EPERM) | Some(libc::EACCES) => PlatformError::AccessDenied(format!(
            "Not allowed to {what}: the process belongs to another user or is protected. Dyno can't change it without the helper."
        )),
        _ => PlatformError::Failed(format!("Could not {what}: {e}")),
    }
}

/// Sets the I/O priority of the thread/process `tid` to idle, or (`idle == false`) back to "derive from the nice
/// value", which is the kernel's default.
pub fn set_io_idle(tid: u32, idle: bool) -> Result<(), PlatformError> {
    let class = if idle { IOPRIO_CLASS_IDLE } else { 0 };
    let value = class << IOPRIO_CLASS_SHIFT;
    // SAFETY: ioprio_set takes three integers and reads no memory; failure is reported through the return value and errno.
    let r = unsafe { libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, tid as libc::c_int, value) };
    if r == 0 {
        Ok(())
    } else {
        Err(failure("lower the disk priority"))
    }
}

/// Restricts the thread `tid` to the CPUs in `mask` (bit n = CPU n).
pub fn set_thread_affinity(tid: u32, mask: u64) -> Result<(), PlatformError> {
    // SAFETY: cpu_set_t is plain bytes for which all-zero is a valid (empty) set.
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    for cpu in 0..64usize {
        if mask & (1u64 << cpu) != 0 {
            // SAFETY: cpu < 64 is inside the set (CPU_SETSIZE is 1024) and `set` is a valid, exclusively borrowed cpu_set_t.
            unsafe { libc::CPU_SET(cpu, &mut set) };
        }
    }
    // SAFETY: `set` is valid for `size_of::<cpu_set_t>()` bytes and only read by the call.
    let r = unsafe { libc::sched_setaffinity(tid as libc::pid_t, std::mem::size_of::<libc::cpu_set_t>(), &set) };
    if r == 0 {
        Ok(())
    } else {
        Err(failure("limit the cores"))
    }
}

/// The effective user id of this process.
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot fail.
    unsafe { libc::geteuid() }
}
