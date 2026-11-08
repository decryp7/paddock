//! Private scratch folders: the one way Debrief makes a temporary folder.
//!
//! A fixed or guessable name in the shared temp folder can be planted by another local user (a folder or a link made first).
//! [`private_tempdir`] makes a folder with a random name, atomically (the name must not exist), with mode 0700 on Unix, and
//! removes it when the guard is dropped. `tempfile` alone leaves the mode to the umask, so use this, not `tempfile::tempdir`,
//! for production scratch space.

use std::io;
use tempfile::TempDir;

/// A fresh scratch folder named `<prefix><random>`, owner-only, removed when the returned guard is dropped.
///
/// # Errors
/// The I/O error when the folder cannot be made (no space, no rights); the caller names the next step.
pub fn private_tempdir(prefix: &str) -> io::Result<TempDir> {
    let mut b = tempfile::Builder::new();
    b.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        b.permissions(std::fs::Permissions::from_mode(0o700));
    }
    b.tempdir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_folder_is_owner_only_whatever_the_umask_and_goes_with_its_guard() {
        use std::os::unix::fs::PermissionsExt;
        let a = private_tempdir("debrief-scratch-test-").unwrap();
        let b = private_tempdir("debrief-scratch-test-").unwrap();
        assert_ne!(a.path(), b.path(), "no predictable name");
        assert_eq!(std::fs::metadata(a.path()).unwrap().permissions().mode() & 0o777, 0o700);
        let gone = a.path().to_path_buf();
        drop(a);
        assert!(!gone.exists());
    }
}
