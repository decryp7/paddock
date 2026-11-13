//! Private folders and files (ADR 0092): [`PrivateDir`] and [`PrivateFile`], owner-only from creation and checked from handles, and
//! the last-resort folder when no home folder is known.
//!
//! A shared, fixed path such as `/tmp/Dyno` is never a fallback: another local user can create it first, and the app would then
//! write into it or trust a discovery file planted there (rule 53). [`private_fallback_dir`] makes the one fallback that can pass
//! the folder-trust check ([`crate::folder`]).

mod acl;
mod dir;
mod error;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

#[cfg(any(test, feature = "test-support"))]
pub use dir::AccountId;
pub use dir::{PrivateDir, PrivateFile};
pub use error::{PrivateError, PrivateResult};

/// The one check of a folder the user named (a data folder, a config folder), used by every app and tool (ADR 0092, revised 2026-11-12):
/// the deepest folder of `root` that exists is opened through [`PrivateDir`] (owner, rights for others, its parent, links), and nothing
/// is created. A missing `root` passes when the folder it would be made in passes.
///
/// # Errors
/// [`PrivateError::Refused`] with the cause and the next step, or [`PrivateError::Io`].
pub fn check_named_folder(root: &Path) -> PrivateResult<()> {
    let mut existing = root;
    while std::fs::symlink_metadata(existing).is_err() {
        match existing.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    PrivateDir::open(existing, crate::folder::Links::Follow).map(|_| ())
}

use crate::error::{EnvError, Result};
use crate::folder::{FolderFacts, Refusal, current_uid, decide};
use std::path::{Path, PathBuf};

/// The shared temp folder may hold the fallback only if its contents cannot be renamed or replaced by others: it belongs to this
/// account, or to root with the sticky bit set (like `/tmp`).
pub fn check_temp_parent(facts: FolderFacts, uid: u32) -> std::result::Result<(), Refusal> {
    let root_sticky = facts.uid == 0 && facts.mode & 0o1000 != 0;
    if root_sticky {
        return Ok(());
    }
    decide(facts, uid)
}

/// The last-resort folder for `name` when no per-user folder can be found: `<temp>/<name>-<uid>`, made with mode 0700 and used
/// only if the folder passes the folder-trust check as a real folder (never a link) owned by this account and closed to everyone
/// else, and the temp folder passes [`check_temp_parent`]. Unix only; Windows always has a profile folder, so there it is an error
/// and the caller stops with the message.
///
/// # Errors
/// [`EnvError::NoHome`] on Windows; [`EnvError::Io`] when the folder cannot be made or is not safe to use (a planted folder is
/// refused, never used).
pub fn private_fallback_dir(temp: &Path, name: &str) -> Result<PathBuf> {
    private_fallback_dir_as(temp, name, current_uid())
}

/// [`private_fallback_dir`] for the account `uid` (tests name another account to meet a folder it does not own).
pub fn private_fallback_dir_as(temp: &Path, name: &str, uid: u32) -> Result<PathBuf> {
    #[cfg(not(unix))]
    {
        let _ = (temp, name, uid);
        Err(EnvError::NoHome(
            "no per-user folder is known and no shared folder is used instead",
        ))
    }
    #[cfg(unix)]
    {
        use crate::folder::{Links, check_folder_with, read_folder};
        use std::os::unix::fs::DirBuilderExt;
        let io = |op, path: &Path, source| EnvError::Io {
            op,
            path: path.to_path_buf(),
            source,
        };
        let refuse = |path: &Path, why: Refusal| {
            io(
                "use folder",
                path,
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, why.reason()),
            )
        };
        let facts = read_folder(temp, Links::Follow).map_err(|e| refuse(temp, e))?;
        check_temp_parent(facts, uid).map_err(|e| refuse(temp, e))?;
        let dir = temp.join(format!("{name}-{uid}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io("create folder", &dir, e)),
        }
        // One handle: the owner and mode come from the folder opened here, and a link in its place is refused.
        check_folder_with(&dir, uid, Links::Refuse, |p, l| {
            let f = read_folder(p, l)?;
            if f.mode & 0o077 != 0 {
                return Err(Refusal::OpenToOthers { mode: f.mode });
            }
            Ok(f)
        })
        .map_err(|e| refuse(&dir, e))?;
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temp_folder_must_be_the_users_or_roots_with_the_sticky_bit() {
        let f = |uid, mode| FolderFacts { uid, mode };
        assert_eq!(check_temp_parent(f(0, 0o41777), 501), Ok(()), "like /tmp");
        assert_eq!(check_temp_parent(f(501, 0o40700), 501), Ok(()));
        assert_eq!(
            check_temp_parent(f(0, 0o40777), 501),
            Err(Refusal::OtherOwner { uid: 0 }),
            "world-writable without sticky"
        );
        assert_eq!(
            check_temp_parent(f(502, 0o41777), 501),
            Err(Refusal::OtherOwner { uid: 502 })
        );
        assert_eq!(
            check_temp_parent(f(501, 0o40777), 501),
            Err(Refusal::OpenToOthers { mode: 0o40777 })
        );
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        /// A temp folder that passes the parent check (tempfile makes it 0700 and owned by this user).
        fn temp() -> tempfile::TempDir {
            tempfile::tempdir().unwrap()
        }

        #[test]
        fn the_fallback_is_per_user_private_and_reused() {
            let t = temp();
            let a = private_fallback_dir(t.path(), "Dyno").unwrap();
            assert_ne!(a, t.path().join("Dyno"), "never the fixed shared name");
            assert_eq!(
                std::fs::metadata(&a).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(private_fallback_dir(t.path(), "Dyno").unwrap(), a);
        }

        #[test]
        fn a_planted_link_in_place_of_the_fallback_is_refused() {
            let t = temp();
            let elsewhere = t.path().join("attacker");
            std::fs::create_dir(&elsewhere).unwrap();
            let planted = private_fallback_dir(t.path(), "Dyno").unwrap();
            std::fs::remove_dir(&planted).unwrap();
            std::os::unix::fs::symlink(&elsewhere, &planted).unwrap();
            let err = private_fallback_dir(t.path(), "Dyno").unwrap_err();
            assert!(err.to_string().contains("symbolic link"), "{err}");
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
        }

        #[test]
        fn a_fallback_folder_owned_by_someone_else_is_refused_and_nothing_is_written() {
            let t = temp();
            // The folder for account 4242 exists but this user owns it: to account 4242 it belongs to someone else.
            let planted = t.path().join("Dyno-4242");
            std::fs::create_dir(&planted).unwrap();
            let err = private_fallback_dir_as(t.path(), "Dyno", 4242);
            // Check_temp_parent also fails first when the temp folder is this user's and uid differs; either way it is refused.
            assert!(err.is_err(), "{err:?}");
            assert_eq!(
                std::fs::read_dir(&planted).unwrap().count(),
                0,
                "nothing written there"
            );
        }

        #[test]
        fn a_fallback_open_to_others_is_refused() {
            let t = temp();
            let d = private_fallback_dir(t.path(), "Dyno").unwrap();
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o750)).unwrap();
            let err = private_fallback_dir(t.path(), "Dyno").unwrap_err();
            assert!(err.to_string().contains("other users"), "{err}");
        }

        #[test]
        fn a_temp_folder_that_is_missing_or_a_file_is_refused() {
            let t = temp();
            assert!(private_fallback_dir(&t.path().join("none"), "Dyno").is_err());
            let f = t.path().join("f");
            std::fs::write(&f, b"x").unwrap();
            assert!(private_fallback_dir(&f, "Dyno").is_err());
        }

        #[test]
        fn a_folder_that_cannot_be_made_is_an_error() {
            let t = temp();
            std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
            let r = private_fallback_dir(t.path(), "Dyno");
            std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
            if current_uid() != 0 {
                assert!(r.unwrap_err().to_string().contains("create folder"));
            }
        }
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

#[cfg(all(test, unix))]
mod named_folder_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A refused folder, or the place a missing one would be made in, is refused and nothing is created.
    #[test]
    fn the_check_creates_nothing_and_refuses_a_folder_others_can_write_and_what_would_be_made_in_it()
     {
        let tmp = tempfile::tempdir().unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(matches!(
            check_named_folder(&open),
            Err(PrivateError::Refused { .. })
        ));
        let below = open.join("a").join("b");
        assert!(matches!(
            check_named_folder(&below),
            Err(PrivateError::Refused { .. })
        ));
        assert_eq!(
            std::fs::read_dir(&open).unwrap().count(),
            0,
            "nothing was created"
        );
        let own = tmp.path().join("own");
        std::fs::create_dir(&own).unwrap();
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o700)).unwrap();
        if let Err(e) = check_named_folder(&own.join("not").join("yet")) {
            panic!("a missing folder in a private one passes: {e}");
        }
        assert!(!own.join("not").exists(), "and is not made by the check");
    }
}

#[cfg(all(test, windows))]
mod named_folder_windows_tests {
    use super::*;
    use std::process::Command;

    fn everyone_can_write(folder: &Path) {
        let out = Command::new("icacls")
            .arg(folder)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    /// Windows twin of the Unix test: `PrivateDir::open` refuses a folder Everyone may write, the walk finds it from a missing folder
    /// below it, and nothing is created. Owed to the Windows runner or VM.
    #[test]
    fn windows_the_check_walks_up_creates_nothing_and_refuses_a_folder_everyone_can_write() {
        let tmp = tempfile::tempdir().unwrap();
        let open = tmp.path().join("open");
        std::fs::create_dir(&open).unwrap();
        everyone_can_write(&open);
        assert!(matches!(
            PrivateDir::open(&open, crate::folder::Links::Follow),
            Err(PrivateError::Refused { .. })
        ));
        assert!(matches!(
            check_named_folder(&open),
            Err(PrivateError::Refused { .. })
        ));
        let below = open.join("a").join("b");
        assert!(matches!(
            check_named_folder(&below),
            Err(PrivateError::Refused { .. })
        ));
        assert_eq!(
            std::fs::read_dir(&open).unwrap().count(),
            0,
            "nothing was created"
        );
        // Made owner-only, not with `create_dir`: a plain folder takes the access list of `tmp`, and a CI runner's temp folder (under
        // `C:\`) grants `Authenticated Users` Modify to what is made in it (Dyno CI run 8035), which the check rightly refuses.
        let own = tmp.path().join("own");
        PrivateDir::create_all(&own).unwrap();
        if let Err(e) = check_named_folder(&own.join("not").join("yet")) {
            panic!("a missing folder in a private one passes: {e}");
        }
        assert!(!own.join("not").exists(), "and is not made by the check");
    }
}
