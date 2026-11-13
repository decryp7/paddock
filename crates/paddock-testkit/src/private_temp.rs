//! [`PrivateTemp`]: a temporary folder that passes the private-folder check on every OS (ADR 0092, ADR 0078).
//!
//! `tempfile::tempdir()` is private on Unix (mode 0700) but on Windows it takes the access list of the temp folder it is made in.
//! A user's own `%TEMP%` (under the profile) is closed to other accounts, so a developer's machine hides the difference; a CI
//! runner's temp folder under `C:\...` inherits `Authenticated Users: Modify` from the drive root, and an app that refuses a data
//! folder other accounts can write (`paddock_env::private::check_named_folder`) then refuses every test folder (Dyno CI run 8035:
//! "its access list grants access to S-1-5-11"). A [`PrivateTemp`] is made owner-only from creation (no inheritance), so a test
//! that hands its folder to such an app behaves as it does on a user's machine, whatever the temp folder's access list is.
//!
//! Known limit, for tests only: on Windows the folder is made inside an outer `tempfile` folder that has the temp folder's access
//! list, so on a shared host another account that can write the temp folder can still rename or delete that outer folder (the
//! private folder inside it cannot be read or changed by them). The apps' own check of a parent allows that case (it refuses only a
//! parent others can delete from or change the permissions of), and a test folder is short-lived; nothing here is a security
//! boundary for production data.

use std::io;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A temporary folder, deleted on drop, that only the account running the test can use (on Windows: owner-only, nothing inherited, but see the
/// known limit in the module docs about its outer folder;
/// on Unix: mode 0700, as `tempfile` makes it).
#[derive(Debug)]
pub struct PrivateTemp {
    outer: TempDir,
    /// On Windows the owner-only folder inside `outer` (which has the temp folder's access list); on Unix `outer` itself.
    path: PathBuf,
}

impl PrivateTemp {
    /// A new folder in the system temp folder.
    ///
    /// # Errors
    /// The folder cannot be made or made private.
    pub fn new() -> io::Result<PrivateTemp> {
        PrivateTemp::from(
            tempfile::Builder::new()
                .prefix("paddock-private-")
                .tempdir()?,
        )
    }

    /// A new folder in `parent`.
    ///
    /// # Errors
    /// As [`PrivateTemp::new`].
    pub fn new_in(parent: &Path) -> io::Result<PrivateTemp> {
        PrivateTemp::from(
            tempfile::Builder::new()
                .prefix("paddock-private-")
                .tempdir_in(parent)?,
        )
    }

    /// A new folder in `parent`, named with `prefix`.
    ///
    /// # Errors
    /// As [`PrivateTemp::new`].
    pub fn with_prefix(prefix: &str, parent: Option<&Path>) -> io::Result<PrivateTemp> {
        let mut b = tempfile::Builder::new();
        b.prefix(prefix);
        PrivateTemp::from(match parent {
            Some(p) => b.tempdir_in(p)?,
            None => b.tempdir()?,
        })
    }

    #[cfg(windows)]
    fn from(outer: TempDir) -> io::Result<PrivateTemp> {
        let inner = outer.path().join("private");
        paddock_env::private::PrivateDir::create_all(&inner)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(PrivateTemp { outer, path: inner })
    }

    #[cfg(not(windows))]
    fn from(outer: TempDir) -> io::Result<PrivateTemp> {
        let path = outer.path().to_path_buf();
        Ok(PrivateTemp { outer, path })
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Keeps the folder on disk (for a person debugging) and returns its path.
    pub fn keep(self) -> PathBuf {
        let path = self.path;
        let _ = self.outer.keep();
        path
    }
}

impl AsRef<Path> for PrivateTemp {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// A new [`PrivateTemp`]: the drop-in for `tempfile::tempdir()` (same shape, so a test only changes the call) in a test whose
/// folder is given to code that checks it.
///
/// # Errors
/// As [`PrivateTemp::new`].
pub fn private_tempdir() -> io::Result<PrivateTemp> {
    PrivateTemp::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_exists_is_removed_on_drop_and_can_be_kept() {
        let t = private_tempdir().unwrap();
        let p = t.path().to_path_buf();
        assert!(p.is_dir());
        std::fs::write(p.join("f"), b"x").unwrap();
        drop(t);
        assert!(!p.exists());
        let kept = private_tempdir().unwrap().keep();
        assert!(kept.is_dir());
        std::fs::remove_dir_all(&kept).unwrap();
        let _ = std::fs::remove_dir(kept.parent().unwrap());
    }

    /// The CI runner's temp folder: a Windows folder under `C:\` grants `Authenticated Users` (S-1-5-11) Modify to what is made in it.
    /// A plain `tempfile` folder there is refused as a named data folder; a private one passes, and nothing is made by the check.
    #[cfg(windows)]
    #[test]
    fn windows_a_private_folder_passes_the_named_folder_check_under_a_parent_other_accounts_can_write()
     {
        let parent = tempfile::tempdir().unwrap();
        let grant = std::process::Command::new("icacls")
            .arg(parent.path())
            .args(["/grant", "*S-1-5-11:(OI)(CI)M"])
            .output()
            .unwrap();
        assert!(
            grant.status.success(),
            "{}",
            String::from_utf8_lossy(&grant.stdout)
        );

        let plain = tempfile::tempdir_in(parent.path()).unwrap();
        assert!(
            paddock_env::private::check_named_folder(&plain.path().join("data")).is_err(),
            "the probe sees the difference: a plain folder there is refused"
        );

        let private = PrivateTemp::new_in(parent.path()).unwrap();
        let data = private.path().join("data");
        paddock_env::private::check_named_folder(&data).unwrap();
        assert!(!data.exists(), "the check creates nothing");
    }
}
