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

/// What the last [`check_data_folder`] said for each `what`, so only a change is logged (two folders checked in one process do not
/// hide each other's changes).
static LAST_REASON: std::sync::Mutex<Vec<(String, &'static str)>> =
    std::sync::Mutex::new(Vec::new());

fn note_result(what: &str, r: &Result<(), Refusal>) {
    let now = r.as_ref().err().map(Refusal::reason);
    let mut seen = LAST_REASON
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = seen.iter().position(|(w, _)| w == what);
    let before = at.map(|i| seen[i].1);
    if before == now {
        return;
    }
    match (now, at) {
        (Some(reason), Some(i)) => seen[i].1 = reason,
        (Some(reason), None) => seen.push((what.to_owned(), reason)),
        (None, _) => seen.retain(|(w, _)| w != what),
    }
    match now {
        Some(reason) => {
            log::warn!(Why = reason; "Not trusting {what}: the data folder is not safe ({reason}); it must belong to this account and no one else may change it (chmod go-w)")
        }
        None => log::info!("The data folder is trusted again for {what}"),
    }
}

/// Makes `path` and its parents with mode 0700 where the OS has modes, whatever the umask: a data folder is private from the
/// moment it exists.
///
/// # Errors
/// The folder could not be made.
pub fn create_private_dir_all(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// What [`tighten_group_write`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tightened {
    /// The folder already passed; nothing changed.
    NotNeeded,
    /// Group write was removed from the user's own folder (a folder made under umask 002 in a private group).
    GroupWriteRemoved,
}

/// The pure part of [`tighten_group_write`]: whether group write may be removed from a folder with these facts. Only the
/// account's own folder, with group write alone (no world write), whose group is the account's own primary group with no other
/// member, qualifies: nobody else could have put anything in it. A world-writable or foreign folder never does.
pub fn may_remove_group_write(
    facts: FolderFacts,
    gid: u32,
    uid: u32,
    egid: u32,
    group_has_others: bool,
) -> bool {
    facts.uid == uid
        && facts.mode & 0o002 == 0
        && facts.mode & 0o020 != 0
        && gid == egid
        && !group_has_others
}

/// For the agent at start: a data folder that the account owns and that only its private group can write (Dyno 1.1.0 and older
/// made it with the umask, 0775 under umask 002) gets its group write bit removed, through the handle it opened, once. Anything
/// else that fails the check is refused as it is, with the reason; the next step is `chmod go-w <data folder>`.
///
/// `group_has_others(gid)` says whether the group has any member besides the account (inject it in tests).
///
/// # Errors
/// The folder is not the account's, is open to the world, or its group is shared.
#[cfg(unix)]
pub fn tighten_group_write(
    folder: &Path,
    group_has_others: impl FnOnce(u32) -> bool,
) -> Result<Tightened, Refusal> {
    tighten_group_write_with(folder, group_has_others, fchmod)
}

/// `fchmod` on an open handle: the change goes to the folder the handle names, not to a path looked up again.
#[cfg(unix)]
fn fchmod(handle: &std::fs::File, mode: u32) -> std::io::Result<()> {
    // SAFETY: the descriptor is open for the call.
    let rc = unsafe {
        libc::fchmod(
            std::os::fd::AsRawFd::as_raw_fd(handle),
            mode as libc::mode_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// [`tighten_group_write`] with the group lookup and the mode change passed in (tests make the change fail).
///
/// # Errors
/// See [`tighten_group_write`]; a mode change that fails is [`Refusal::Unreadable`] ("remove group write").
#[cfg(unix)]
pub fn tighten_group_write_with(
    folder: &Path,
    group_has_others: impl FnOnce(u32) -> bool,
    chmod: impl FnOnce(&std::fs::File, u32) -> std::io::Result<()>,
) -> Result<Tightened, Refusal> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let unreadable = |what, e: std::io::Error| Refusal::Unreadable {
        what,
        cause: e.to_string(),
    };
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .open(folder)
        .map_err(|e| unreadable("open the folder", e))?;
    let meta = handle
        .metadata()
        .map_err(|e| unreadable("fstat the folder", e))?;
    let facts = FolderFacts {
        uid: meta.uid(),
        mode: meta.mode(),
    };
    let me = current_uid();
    let refusal = match decide(facts, me) {
        Ok(()) => return Ok(Tightened::NotNeeded),
        Err(r) => r,
    };
    // SAFETY: getegid has no arguments and cannot fail.
    let egid = unsafe { libc::getegid() };
    let gid = meta.gid();
    if !may_remove_group_write(facts, gid, me, egid, group_has_others(gid)) {
        return Err(refusal);
    }
    let mode = meta.mode() & 0o7777 & !0o020;
    chmod(&handle, mode).map_err(|e| unreadable("remove group write", e))?;
    log::info!(
        "Removed group write from the data folder (it belongs to this account's private group)"
    );
    Ok(Tightened::GroupWriteRemoved)
}

/// The pure scan: whether `members` (group member names) holds anyone other than `own` (the account's user name).
pub fn others_among(own: Option<&[u8]>, members: &[Vec<u8>]) -> bool {
    members.iter().any(|m| Some(m.as_slice()) != own)
}

/// The account's user name (`getpwuid_r`).
#[cfg(unix)]
fn user_name(uid: u32) -> Option<Vec<u8>> {
    // u64 elements: the lookups store pointers in the buffer, which must be aligned.
    let mut buf = vec![0u64; 2 * 1024];
    // SAFETY: a zeroed passwd is a plain-old-data out parameter the call fills; the buffer outlives the read of the name.
    unsafe {
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        let rc = libc::getpwuid_r(
            uid,
            &mut pw,
            buf.as_mut_ptr().cast(),
            buf.len() * 8,
            &mut out,
        );
        (rc == 0 && !out.is_null())
            .then(|| std::ffi::CStr::from_ptr(pw.pw_name).to_bytes().to_vec())
    }
}

/// The names of the members of group `gid` (`getgrgid_r`); `None` when the group cannot be read.
#[cfg(unix)]
fn group_members(gid: u32) -> Option<Vec<Vec<u8>>> {
    let mut buf = vec![0u64; 2 * 1024];
    // SAFETY: a zeroed group is a plain-old-data out parameter the call fills; the buffer outlives the reads below.
    unsafe {
        let mut gr: libc::group = std::mem::zeroed();
        let mut out: *mut libc::group = std::ptr::null_mut();
        let rc = libc::getgrgid_r(
            gid,
            &mut gr,
            buf.as_mut_ptr().cast(),
            buf.len() * 8,
            &mut out,
        );
        if rc != 0 || out.is_null() {
            return None;
        }
        // The member list may sit unaligned in the buffer on some systems: read each pointer unaligned.
        let mut names = Vec::new();
        let mut at = gr.gr_mem;
        while !at.is_null() {
            let member = std::ptr::read_unaligned(at);
            if member.is_null() {
                break;
            }
            names.push(std::ffi::CStr::from_ptr(member).to_bytes().to_vec());
            at = at.add(1);
        }
        Some(names)
    }
}

/// Whether the group `gid` has a member other than the account `uid`; an unreadable group counts as shared, so nothing is
/// tightened on a doubt.
#[cfg(unix)]
pub fn group_has_other_members(gid: u32, uid: u32) -> bool {
    group_members(gid).is_none_or(|members| others_among(user_name(uid).as_deref(), &members))
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

    fn noted(what: &str) -> Option<&'static str> {
        LAST_REASON
            .lock()
            .unwrap()
            .iter()
            .find(|(w, _)| w == what)
            .map(|(_, r)| *r)
    }

    #[test]
    fn a_refusal_is_noted_once_per_change_and_per_caller() {
        note_result("note-a", &Err(Refusal::Symlink));
        note_result("note-b", &Err(Refusal::NotAFolder));
        assert_eq!(noted("note-a"), Some("it is a symbolic link"));
        assert_eq!(
            noted("note-b"),
            Some("it is not a folder"),
            "another caller's state is separate"
        );
        note_result("note-a", &Err(Refusal::Symlink));
        note_result("note-a", &Err(Refusal::NotAFolder));
        assert_eq!(noted("note-a"), Some("it is not a folder"));
        note_result("note-a", &Ok(()));
        assert_eq!(noted("note-a"), None);
        note_result("note-a", &Ok(()));
        let t = tempfile::tempdir().unwrap();
        assert_eq!(check_data_folder(t.path(), "note-c"), Ok(()));
        assert!(check_data_folder(&t.path().join("none"), "note-c").is_err());
        assert!(noted("note-c").is_some());
    }

    #[test]
    fn group_write_may_be_removed_only_from_the_accounts_own_private_group_folder() {
        let f = |uid, mode| FolderFacts { uid, mode };
        assert!(may_remove_group_write(f(501, 0o40775), 20, 501, 20, false));
        assert!(
            !may_remove_group_write(f(501, 0o40775), 20, 501, 20, true),
            "the group has other members"
        );
        assert!(
            !may_remove_group_write(f(501, 0o40775), 99, 501, 20, false),
            "not the account's own group"
        );
        assert!(
            !may_remove_group_write(f(501, 0o40777), 20, 501, 20, false),
            "world-writable is never tightened"
        );
        assert!(
            !may_remove_group_write(f(501, 0o40757), 20, 501, 20, false),
            "world write alone is not group write"
        );
        assert!(
            !may_remove_group_write(f(0, 0o40775), 20, 501, 20, false),
            "foreign owner"
        );
        assert!(
            !may_remove_group_write(f(501, 0o40755), 20, 501, 20, false),
            "nothing to remove"
        );
    }

    #[cfg(unix)]
    mod tighten {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn mode(p: &Path) -> u32 {
            std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
        }

        #[test]
        fn a_new_data_folder_is_private_whatever_the_umask() {
            let t = tempfile::tempdir().unwrap();
            let deep = t.path().join("a").join("b");
            create_private_dir_all(&deep).unwrap();
            assert_eq!(mode(&deep), 0o700);
            assert_eq!(mode(&t.path().join("a")), 0o700);
            create_private_dir_all(&deep).unwrap();
        }

        #[test]
        fn an_own_group_writable_folder_loses_group_write_once() {
            let t = tempfile::tempdir().unwrap();
            std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
            assert_eq!(
                tighten_group_write(t.path(), |_| false),
                Ok(Tightened::GroupWriteRemoved)
            );
            assert_eq!(mode(t.path()), 0o755);
            assert_eq!(
                tighten_group_write(t.path(), |_| false),
                Ok(Tightened::NotNeeded)
            );
        }

        #[test]
        fn a_shared_group_a_world_writable_folder_and_a_foreign_folder_are_refused_untouched() {
            let t = tempfile::tempdir().unwrap();
            std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
            assert_eq!(
                tighten_group_write(t.path(), |_| true),
                Err(Refusal::OpenToOthers { mode: 0o40775 })
            );
            assert_eq!(mode(t.path()), 0o775, "not changed");
            std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
            assert!(matches!(
                tighten_group_write(t.path(), |_| false),
                Err(Refusal::OpenToOthers { .. })
            ));
            assert_eq!(
                mode(t.path()),
                0o777,
                "a world-writable folder is never tightened"
            );
            let root = tighten_group_write(Path::new("/"), |_| false);
            assert!(
                current_uid() == 0 || root == Err(Refusal::OtherOwner { uid: 0 }),
                "{root:?}"
            );
            assert!(matches!(
                tighten_group_write(&t.path().join("none"), |_| false),
                Err(Refusal::Unreadable { .. })
            ));
        }

        #[test]
        fn a_mode_change_the_os_refuses_is_an_error_and_an_unknown_group_counts_as_shared() {
            // /dev/null belongs to root: fchmod on its handle is refused unless this is root.
            let null = std::fs::File::open("/dev/null").unwrap();
            assert!(current_uid() == 0 || fchmod(&null, 0o600).is_err());
            let unknown = 3_999_999_999;
            assert!(
                group_members(unknown).is_some() || group_has_other_members(unknown, current_uid())
            );
        }

        #[test]
        fn the_member_scan_finds_anyone_but_the_account() {
            let m = |names: &[&str]| {
                names
                    .iter()
                    .map(|n| n.as_bytes().to_vec())
                    .collect::<Vec<_>>()
            };
            assert!(!others_among(Some(b"me"), &m(&[])), "no members");
            assert!(!others_among(Some(b"me"), &m(&["me"])), "only the account");
            assert!(
                others_among(Some(b"me"), &m(&["me", "you"])),
                "another member"
            );
            assert!(others_among(Some(b"me"), &m(&["you"])));
            assert!(
                others_among(None, &m(&["me"])),
                "an unknown account name counts every member as another"
            );
            assert!(!others_among(None, &m(&[])));
        }

        #[test]
        fn a_failed_mode_change_is_refused_and_changes_nothing_else() {
            let t = tempfile::tempdir().unwrap();
            std::fs::set_permissions(t.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
            let failing = |_: &std::fs::File, _| Err(std::io::Error::other("EPERM"));
            let r = tighten_group_write_with(t.path(), |_| false, failing);
            assert!(
                matches!(&r, Err(Refusal::Unreadable { what: "remove group write", cause }) if cause.contains("EPERM")),
                "{r:?}"
            );
            assert_eq!(mode(t.path()), 0o775, "nothing changed");
            assert_eq!(
                tighten_group_write(t.path(), |_| false),
                Ok(Tightened::GroupWriteRemoved),
                "the real change works"
            );
        }

        #[test]
        fn the_real_group_lookup_answers_for_this_account() {
            // Whatever the machine's groups are, the lookup answers without failing.
            let _ = group_has_other_members(0, current_uid());
            // SAFETY: getegid has no arguments and cannot fail.
            let _ = group_has_other_members(unsafe { libc::getegid() }, current_uid());
        }
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
