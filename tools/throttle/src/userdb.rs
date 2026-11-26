//! The current user's home folder from the user database (`getpwuid_r`), for a process that has no `HOME` (a launchd daemon with
//! `UserName` set, a systemd unit): [`crate::places`] asks it only when the environment gives no absolute home.
#![allow(unsafe_code)]

use std::path::PathBuf;

/// The home folder of the effective user, if it has an entry in the user database and the entry names an absolute folder.
#[cfg(unix)]
pub fn home_of_current_user() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: `geteuid` has no arguments and cannot fail.
    let uid = unsafe { libc::geteuid() };
    // SAFETY: an all-zero `passwd` is a valid out value for `getpwuid_r`.
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: every pointer is valid for the size passed; `result` points into `pw` or is null.
    let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
    if rc != 0 || result.is_null() || pw.pw_dir.is_null() {
        return None;
    }
    // SAFETY: `pw_dir` is a NUL-terminated string inside `buf`, which is alive here.
    let dir = unsafe { std::ffi::CStr::from_ptr(pw.pw_dir) };
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
    path.is_absolute().then_some(path)
}

/// No user database on Windows: the profile folder comes from `USERPROFILE`.
#[cfg(not(unix))]
pub fn home_of_current_user() -> Option<PathBuf> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn the_user_database_names_an_absolute_home_for_the_running_user() {
        let home = super::home_of_current_user().expect("the running user has a passwd entry");
        assert!(home.is_absolute(), "{home:?}");
    }
}
