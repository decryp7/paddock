//! A folder only the current user controls: the check that a data folder may be trusted, and the last-resort folder when no home
//! folder is known.
//!
//! A shared, fixed path such as `/tmp/Dyno` is never a fallback: another local user can create it first, and the app would then
//! write into it or trust a discovery file planted there (rule 53: such a file is only a hint, and a hint from a folder somebody
//! else controls is not even that). [`check_private_dir`] is the test every folder must pass before it is trusted;
//! [`private_fallback_dir`] makes the one fallback that can pass it.

use crate::error::{EnvError, Result};
use std::path::{Path, PathBuf};

/// Why a folder is not trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Untrusted {
    /// The path cannot be inspected (missing or no access).
    Unreadable,
    /// The path is a symbolic link, so it may lead anywhere.
    Symlink,
    /// The path is not a folder.
    NotAFolder,
    /// Another user owns it.
    OtherOwner,
    /// Other users can read or change it (a folder this code made itself).
    OpenToOthers,
}

impl Untrusted {
    /// A short phrase for logs and messages; it names no path.
    pub fn reason(self) -> &'static str {
        match self {
            Untrusted::Unreadable => "it cannot be inspected",
            Untrusted::Symlink => "it is a symbolic link",
            Untrusted::NotAFolder => "it is not a folder",
            Untrusted::OtherOwner => "it belongs to another user",
            Untrusted::OpenToOthers => "other users can access it",
        }
    }
}

/// Checks that `path` is a real folder (not a link) owned by the user running this process. Windows folders under the user's
/// profile are private by their default permissions, so only the link and folder checks apply there.
///
/// # Errors
/// The first reason the folder cannot be trusted.
pub fn check_private_dir(path: &Path) -> std::result::Result<(), Untrusted> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| Untrusted::Unreadable)?;
    if meta.file_type().is_symlink() {
        return Err(Untrusted::Symlink);
    }
    if !meta.is_dir() {
        return Err(Untrusted::NotAFolder);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no arguments and cannot fail.
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(Untrusted::OtherOwner);
        }
    }
    Ok(())
}

/// The last-resort folder for `name` when no per-user folder can be found: `<temp>/<name>-<uid>`, made with mode 0700 and used
/// only if it passes [`check_private_dir`] and no other user has any access to it. Unix only; Windows always has a profile
/// folder, so there it is an error and the caller stops with the message.
///
/// # Errors
/// [`EnvError::NoHome`] on Windows; [`EnvError::Io`] when the folder cannot be made or belongs to someone else (a planted folder
/// is refused, never used).
pub fn private_fallback_dir(temp: &Path, name: &str) -> Result<PathBuf> {
    #[cfg(not(unix))]
    {
        let _ = (temp, name);
        Err(EnvError::NoHome(
            "no per-user folder is known and no shared folder is used instead",
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let io = |op, path: &Path, source| EnvError::Io {
            op,
            path: path.to_path_buf(),
            source,
        };
        // SAFETY: geteuid has no arguments and cannot fail.
        let uid = unsafe { libc::geteuid() };
        let dir = temp.join(format!("{name}-{uid}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io("create folder", &dir, e)),
        }
        let refuse = |why: Untrusted| {
            io(
                "use folder",
                &dir,
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, why.reason()),
            )
        };
        check_private_dir(&dir).map_err(refuse)?;
        let mode = std::fs::symlink_metadata(&dir)
            .map_err(|e| io("inspect folder", &dir, e))?
            .mode();
        if mode & 0o077 != 0 {
            return Err(refuse(Untrusted::OpenToOthers));
        }
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_the_user_made_is_trusted() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_private_dir(t.path()), Ok(()));
    }

    #[test]
    fn a_missing_path_and_a_file_are_not_trusted() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(
            check_private_dir(&t.path().join("none")),
            Err(Untrusted::Unreadable)
        );
        let f = t.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(check_private_dir(&f), Err(Untrusted::NotAFolder));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_not_trusted_even_to_a_good_folder() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = t.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(check_private_dir(&link), Err(Untrusted::Symlink));
    }

    #[cfg(unix)]
    #[test]
    fn the_fallback_is_per_user_private_and_reused() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let a = private_fallback_dir(t.path(), "Dyno").unwrap();
        assert_ne!(a, t.path().join("Dyno"), "never the fixed shared name");
        assert_eq!(
            std::fs::metadata(&a).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(private_fallback_dir(t.path(), "Dyno").unwrap(), a);
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_symlink_in_place_of_the_fallback_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let elsewhere = t.path().join("attacker");
        std::fs::create_dir(&elsewhere).unwrap();
        let planted = private_fallback_dir(t.path(), "Dyno").unwrap();
        std::fs::remove_dir(&planted).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &planted).unwrap();
        let err = private_fallback_dir(t.path(), "Dyno").unwrap_err();
        assert!(err.to_string().contains("symbolic link"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_fallback_open_to_others_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let d = private_fallback_dir(t.path(), "Dyno").unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = private_fallback_dir(t.path(), "Dyno").unwrap_err();
        assert!(err.to_string().contains("other users"), "{err}");
    }

    #[cfg(not(unix))]
    #[test]
    fn without_a_profile_folder_windows_stops() {
        let t = tempfile::tempdir().unwrap();
        assert!(matches!(
            private_fallback_dir(t.path(), "Dyno"),
            Err(EnvError::NoHome(_))
        ));
    }
}
