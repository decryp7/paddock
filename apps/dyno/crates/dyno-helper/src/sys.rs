//! The few operating-system calls the helper makes itself: whether this process is elevated, and which groups
//! administer the machine. (Which program a process is has no place here: a caller is its account, never a pid.) Each is a small function with a `SAFETY` comment on every `unsafe` block (AGENTS.md).
#![allow(unsafe_code)]

use paddock_ipc::AdminRule;

/// Whether this process runs as root.
#[cfg(unix)]
pub fn is_elevated() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

/// Windows cannot be asked cheaply here; the install steps fail with "access denied" when it is not elevated.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    true
}

/// The groups whose members administer a Unix machine, resolved by name once at start (macOS `admin`; Linux `sudo`,
/// `wheel` and `admin` where they exist). Windows decides from the caller's token, so the rule is empty there.
#[cfg(unix)]
pub fn admin_rule() -> AdminRule {
    let gids = ["admin", "sudo", "wheel"].into_iter().filter_map(|name| {
        let c = std::ffi::CString::new(name).ok()?;
        // SAFETY: `c` is a valid NUL-terminated string; getgrnam returns null or a pointer to static storage that is read
        // here, on one thread, before any other call.
        let group = unsafe { libc::getgrnam(c.as_ptr()) };
        // SAFETY: a non-null result points at a valid `group`.
        (!group.is_null()).then(|| unsafe { (*group).gr_gid })
    });
    AdminRule::new(gids.collect::<Vec<_>>())
}

/// See the Unix version.
#[cfg(not(unix))]
pub fn admin_rule() -> AdminRule {
    AdminRule::default()
}
