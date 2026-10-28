//! Who is running this process, as the session checks need it.

use dyno_core::session::user_key;

/// The effective Unix user id of this process; `None` on Windows.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn this_uid() -> Option<u32> {
    // SAFETY: geteuid has no arguments and cannot fail.
    Some(unsafe { libc::geteuid() })
}

/// The effective Unix user id of this process; `None` on Windows.
#[cfg(not(unix))]
pub fn this_uid() -> Option<u32> {
    None
}

/// The effective Unix user and group ids of this process; `None` on Windows.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn this_ids() -> Option<(u32, u32)> {
    // SAFETY: getegid has no arguments and cannot fail.
    this_uid().map(|u| (u, unsafe { libc::getegid() }))
}

/// The effective Unix user and group ids of this process; `None` on Windows.
#[cfg(not(unix))]
pub fn this_ids() -> Option<(u32, u32)> {
    None
}

/// The key naming the logged-in user (see [`dyno_core::session::user_key`]).
pub fn this_user_key() -> String {
    user_key(this_uid(), std::env::var("USERNAME").ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_key_names_this_user_the_same_way_each_time() {
        assert_eq!(this_user_key(), this_user_key());
        assert_ne!(this_user_key(), "");
        if cfg!(unix) {
            assert!(this_user_key().starts_with("uid:"));
        }
    }
}
