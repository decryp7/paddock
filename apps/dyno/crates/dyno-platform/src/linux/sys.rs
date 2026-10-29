//! The only unsafe code of the Linux adapters: three tiny system calls. Each block says why it is sound.
#![allow(unsafe_code)]

use dyno_core::ports::PlatformError;

fn failure(what: &str) -> PlatformError {
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => PlatformError::NotFound("The process (or one of its threads) has exited.".into()),
        Some(libc::EPERM) | Some(libc::EACCES) => PlatformError::AccessDenied(format!(
            "Not allowed to {what}: the process belongs to another user or is protected."
        )),
        _ => PlatformError::Failed(format!("Could not {what}: {e}")),
    }
}

/// The effective user id of this process.
pub fn effective_uid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

/// How long one wait for the kernel's `sock_diag` answer may take.
const NETLINK_WAIT_SECS: libc::time_t = 5;
/// `NETLINK_SOCK_DIAG`.
const NETLINK_SOCK_DIAG: libc::c_int = 4;

/// Sends one `sock_diag` request and hands every reply datagram to `on_chunk` until it returns `true` (the dump is
/// complete) or the kernel stops answering. Any user may do this; nothing is bound or changed.
pub fn sock_diag_dump(request: &[u8], mut on_chunk: impl FnMut(&[u8]) -> bool) -> Result<(), PlatformError> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // SAFETY: socket takes three integers and returns a new descriptor or -1.
    let raw = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, NETLINK_SOCK_DIAG) };
    if raw < 0 {
        return Err(failure("open the kernel's socket-statistics channel"));
    }
    // SAFETY: `raw` is a descriptor we just created and nothing else owns; OwnedFd closes it exactly once.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let timeout = libc::timeval {
        tv_sec: NETLINK_WAIT_SECS,
        tv_usec: 0,
    };
    // SAFETY: `timeout` is a valid timeval for the stated length and only read by the call.
    let r = unsafe {
        libc::setsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            std::ptr::addr_of!(timeout).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if r != 0 {
        return Err(failure("set a time limit on the socket-statistics channel"));
    }
    // SAFETY: sockaddr_nl is plain integers; all-zero (the kernel as peer) is valid.
    let mut kernel: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    // SAFETY: `request` and `kernel` are valid for the stated lengths and only read by the call.
    let sent = unsafe {
        libc::sendto(
            fd.as_raw_fd(),
            request.as_ptr().cast(),
            request.len(),
            0,
            std::ptr::addr_of!(kernel).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if sent < 0 {
        return Err(failure("ask the kernel for socket statistics"));
    }
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        // SAFETY: `buf` is valid for writes of its length; the call writes at most that many bytes.
        let n = unsafe { libc::recv(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n < 0 {
            return Err(failure("read the kernel's socket statistics"));
        }
        if n == 0 || on_chunk(&buf[..n as usize]) {
            return Ok(());
        }
    }
}
