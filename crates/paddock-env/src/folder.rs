//! Whether a folder may be trusted: the one folder-trust check of the family (rule 42). paddock-ipc's socket-folder check, Dyno's
//! discovery files and the private fallback folder all use it.
//!
//! The folder is opened by handle (`O_DIRECTORY`, and `O_NOFOLLOW` when links are refused) and its owner and mode are read from
//! that handle, so what was checked is what is used. A folder passes when it belongs to the expected account and no one else can
//! write to it. The facts are read through a function that tests replace, so a folder owned by another account is testable
//! without root.

use std::fmt;
use std::path::Path;

/// Whether a link as the last part of the path is followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Links {
    /// The last part must be a real folder. For a shared parent such as the temp folder, where anyone can make a link.
    Refuse,
    /// A link is followed and the folder it leads to is checked. For a user's own data folder, which may be a link (a dotfile
    /// manager, a folder moved to another disk): what matters is who owns the target.
    Follow,
}

/// What the check needs to know about a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderFacts {
    /// The owner's user id (always 0 where the OS has no such id).
    pub uid: u32,
    /// The permission bits, with the sticky bit.
    pub mode: u32,
}

/// Why a folder is not trusted. Each variant is a refusal; [`Refusal::reason`] is a path-free phrase for logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The folder could not be opened or inspected (missing, no access).
    Unreadable {
        /// What was being done.
        what: &'static str,
        /// The OS error text.
        cause: String,
    },
    /// The last part of the path is a link and links are refused.
    Symlink,
    /// The path is not a folder.
    NotAFolder,
    /// Another account owns the folder.
    OtherOwner {
        /// The owner's uid.
        uid: u32,
    },
    /// Another account can change the folder (a group or world write bit).
    OpenToOthers {
        /// The permission bits.
        mode: u32,
    },
}

impl Refusal {
    /// A short phrase for logs and messages; it names no path.
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::Unreadable { .. } => "it cannot be inspected",
            Refusal::Symlink => "it is a symbolic link",
            Refusal::NotAFolder => "it is not a folder",
            Refusal::OtherOwner { .. } => "it belongs to another user",
            Refusal::OpenToOthers { .. } => "other users can change it",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Unreadable { what, cause } => {
                write!(f, "the folder could not be checked ({what}: {cause})")
            }
            Refusal::OtherOwner { uid } => {
                write!(f, "the folder is owned by uid {uid}, not by this account")
            }
            Refusal::OpenToOthers { mode } => write!(
                f,
                "the folder (mode {:04o}) can be changed by other accounts",
                mode & 0o7777
            ),
            other => f.write_str(other.reason()),
        }
    }
}

impl std::error::Error for Refusal {}

/// The decision, pure: the folder must belong to `expected_uid` and have no group or world write bit.
pub fn decide(facts: FolderFacts, expected_uid: u32) -> Result<(), Refusal> {
    if facts.uid != expected_uid {
        return Err(Refusal::OtherOwner { uid: facts.uid });
    }
    if facts.mode & 0o022 != 0 {
        return Err(Refusal::OpenToOthers { mode: facts.mode });
    }
    Ok(())
}

/// Opens `folder` by handle and reads its owner and mode from that handle. Unix; elsewhere the facts are `uid 0, mode 0o700` once
/// the path is known to be a folder (Windows folders under the user's profile are private by their default permissions).
pub fn read_folder(folder: &Path, links: Links) -> Result<FolderFacts, Refusal> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let flags = libc::O_DIRECTORY
            | if links == Links::Refuse {
                libc::O_NOFOLLOW
            } else {
                0
            };
        let unreadable = |what, e: std::io::Error| Refusal::Unreadable {
            what,
            cause: e.to_string(),
        };
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags)
            .open(folder)
            .map_err(|e| match e.raw_os_error() {
                // A refused link says ELOOP on Linux and ENOTDIR on macOS; a link to a file says ENOTDIR when links are followed.
                Some(libc::ELOOP | libc::ENOTDIR) => {
                    if links == Links::Refuse
                        && std::fs::symlink_metadata(folder)
                            .is_ok_and(|m| m.file_type().is_symlink())
                    {
                        Refusal::Symlink
                    } else {
                        Refusal::NotAFolder
                    }
                }
                _ => unreadable("open the folder", e),
            })?;
        let meta = handle
            .metadata()
            .map_err(|e| unreadable("fstat the folder", e))?;
        Ok(FolderFacts {
            uid: meta.uid(),
            mode: meta.mode(),
        })
    }
    #[cfg(not(unix))]
    {
        let meta = if links == Links::Refuse {
            std::fs::symlink_metadata(folder)
        } else {
            std::fs::metadata(folder)
        }
        .map_err(|e| Refusal::Unreadable {
            what: "inspect the folder",
            cause: e.to_string(),
        })?;
        if meta.file_type().is_symlink() {
            return Err(Refusal::Symlink);
        }
        if !meta.is_dir() {
            return Err(Refusal::NotAFolder);
        }
        Ok(FolderFacts {
            uid: 0,
            mode: 0o700,
        })
    }
}

/// [`check_folder`] with the read of the facts passed in (tests inject owners and failures).
pub fn check_folder_with(
    folder: &Path,
    expected_uid: u32,
    links: Links,
    read: impl FnOnce(&Path, Links) -> Result<FolderFacts, Refusal>,
) -> Result<(), Refusal> {
    decide(read(folder, links)?, expected_uid)
}

/// Checks that `folder` belongs to `expected_uid` and nobody else can write to it. On Windows only the link and folder checks
/// apply and `expected_uid` is ignored.
pub fn check_folder(folder: &Path, expected_uid: u32, links: Links) -> Result<(), Refusal> {
    #[cfg(not(unix))]
    let expected_uid = {
        let _ = expected_uid;
        0
    };
    check_folder_with(folder, expected_uid, links, read_folder)
}

/// Checks a user's own data folder (a link is followed: the target must be this account's and closed to others) and, when it is
/// refused, logs a warning with the reason (no path) once per change, so a check repeated every few seconds does not fill the log
/// (ER-27). `what` names the file that was refused ("the discovery file"). Where it cannot be trusted, the next step is to
/// own the folder, or point DYNO_DATA_DIR-style overrides at one the account owns.
pub fn check_data_folder(folder: &Path, what: &str) -> Result<(), Refusal> {
    let r = check_own_folder(folder, Links::Follow);
    note_result(what, &r);
    r
}

/// What the last [`check_data_folder`] said, so only a change is logged.
static LAST_REASON: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

fn note_result(what: &str, r: &Result<(), Refusal>) {
    let now = r.as_ref().err().map(Refusal::reason);
    let mut last = LAST_REASON
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *last == now {
        return;
    }
    *last = now;
    match now {
        Some(reason) => {
            log::warn!(Why = reason; "Not trusting {what}: the data folder is not safe ({reason}); the account must own the folder and no one else may change it")
        }
        None => log::info!("The data folder is trusted again"),
    }
}

/// The user id this process runs as (0 where the OS has none).
pub fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no arguments and cannot fail.
        unsafe { libc::geteuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// [`check_folder`] for the account running this process.
pub fn check_own_folder(folder: &Path, links: Links) -> Result<(), Refusal> {
    check_folder(folder, current_uid(), links)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(uid: u32, mode: u32) -> impl FnOnce(&Path, Links) -> Result<FolderFacts, Refusal> {
        move |_, _| Ok(FolderFacts { uid, mode })
    }

    #[test]
    fn a_refusal_is_noted_once_per_change() {
        // The note keeps the last reason: the same refusal twice changes nothing, a different state does.
        note_result("x", &Err(Refusal::Symlink));
        assert_eq!(*LAST_REASON.lock().unwrap(), Some("it is a symbolic link"));
        note_result("x", &Err(Refusal::Symlink));
        note_result("x", &Err(Refusal::NotAFolder));
        assert_eq!(*LAST_REASON.lock().unwrap(), Some("it is not a folder"));
        note_result("x", &Ok(()));
        assert_eq!(*LAST_REASON.lock().unwrap(), None);
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_data_folder(t.path(), "x"), Ok(()));
        assert!(check_data_folder(&t.path().join("none"), "x").is_err());
    }

    #[test]
    fn the_owner_with_no_other_write_bit_passes() {
        for mode in [0o700, 0o755, 0o750, 0o40755] {
            assert_eq!(
                check_folder_with(Path::new("/x"), 7, Links::Follow, facts(7, mode)),
                Ok(())
            );
        }
    }

    #[test]
    fn another_owner_is_refused_whatever_the_mode() {
        let e = check_folder_with(Path::new("/x"), 7, Links::Follow, facts(8, 0o700)).unwrap_err();
        assert_eq!(e, Refusal::OtherOwner { uid: 8 });
        assert!(e.to_string().contains("uid 8"));
        assert_eq!(e.reason(), "it belongs to another user");
    }

    #[test]
    fn a_group_or_world_writable_folder_is_refused() {
        for mode in [0o770, 0o707, 0o777, 0o720] {
            let e =
                check_folder_with(Path::new("/x"), 7, Links::Follow, facts(7, mode)).unwrap_err();
            assert_eq!(e, Refusal::OpenToOthers { mode });
            assert!(e.to_string().contains("can be changed"));
        }
    }

    #[test]
    fn a_failed_read_is_a_refusal_with_its_cause() {
        let broken = |_: &Path, _| {
            Err(Refusal::Unreadable {
                what: "open the folder",
                cause: "denied".into(),
            })
        };
        let e = check_folder_with(Path::new("/x"), 0, Links::Follow, broken).unwrap_err();
        assert!(e.to_string().contains("denied") && e.to_string().contains("open the folder"));
        assert_eq!(e.reason(), "it cannot be inspected");
    }

    #[test]
    fn every_refusal_has_a_path_free_reason() {
        assert_eq!(Refusal::Symlink.reason(), "it is a symbolic link");
        assert_eq!(Refusal::NotAFolder.reason(), "it is not a folder");
        assert_eq!(Refusal::Symlink.to_string(), "it is a symbolic link");
        assert_eq!(
            Refusal::OpenToOthers { mode: 0o777 }.reason(),
            "other users can change it"
        );
    }

    #[test]
    fn the_folder_a_user_made_passes_for_that_user_and_not_for_another() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_own_folder(t.path(), Links::Refuse), Ok(()));
        let other = current_uid().wrapping_add(1);
        #[cfg(unix)]
        assert!(matches!(
            check_folder(t.path(), other, Links::Refuse),
            Err(Refusal::OtherOwner { .. })
        ));
        #[cfg(not(unix))]
        let _ = other;
    }

    #[test]
    fn a_missing_path_and_a_file_are_refused() {
        let t = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_folder(&t.path().join("none"), Links::Follow),
            Err(Refusal::Unreadable { .. })
        ));
        let f = t.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(read_folder(&f, Links::Follow), Err(Refusal::NotAFolder));
        assert_eq!(read_folder(&f, Links::Refuse), Err(Refusal::NotAFolder));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused_or_followed_as_asked() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = t.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(read_folder(&link, Links::Refuse), Err(Refusal::Symlink));
        assert_eq!(
            check_own_folder(&link, Links::Refuse),
            Err(Refusal::Symlink)
        );
        assert_eq!(
            check_own_folder(&link, Links::Follow),
            Ok(()),
            "the target is the user's own folder"
        );
        // A link to a file is not a folder; a link to a folder owned by someone else is refused after it is followed.
        let file = t.path().join("file");
        std::fs::write(&file, b"x").unwrap();
        let to_file = t.path().join("to-file");
        std::os::unix::fs::symlink(&file, &to_file).unwrap();
        assert_eq!(read_folder(&to_file, Links::Refuse), Err(Refusal::Symlink));
        assert_eq!(
            read_folder(&to_file, Links::Follow),
            Err(Refusal::NotAFolder)
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_root_folder_is_not_the_users_unless_the_user_is_root() {
        let r = check_own_folder(Path::new("/"), Links::Follow);
        assert!(
            current_uid() == 0 || r == Err(Refusal::OtherOwner { uid: 0 }),
            "{r:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_with_a_group_write_bit_is_refused_for_real() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(matches!(
            check_own_folder(t.path(), Links::Follow),
            Err(Refusal::OpenToOthers { .. })
        ));
    }
}
