//! Unix: every step after the first is relative to a held folder descriptor (`mkdirat`, `openat`, `renameat`), and owner and mode
//! are read from the descriptor (`fstat`), so what was checked is what is used (rule 53, ADR 0033).
#![allow(unsafe_code)]

use super::error::Fail;
use crate::folder::{FolderFacts, Refusal};
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// A held folder.
#[derive(Debug)]
pub(super) struct Dir(OwnedFd);

fn cstring(name: &std::ffi::OsStr) -> io::Result<CString> {
    CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// `-1` is an error, anything else the value.
fn check(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn own(fd: libc::c_int) -> io::Result<OwnedFd> {
    // SAFETY: `fd` was just returned by a successful open and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(check(fd)?) })
}

/// The file type bits of `mode`, in the OS's own `mode_t` (`u16` on macOS, `u32` on Linux), so no conversion is needed.
fn kind(mode: libc::mode_t) -> libc::mode_t {
    mode & libc::S_IFMT
}

/// `mode` as the `u32` of [`FolderFacts`]: a widening on macOS, the same value on Linux. Generic so that one call is right
/// on both without a conversion Clippy calls useless on one of them.
fn widen_mode(mode: impl Into<u32>) -> u32 {
    mode.into()
}

fn fstat(fd: &OwnedFd) -> io::Result<libc::stat> {
    // SAFETY: `stat` is plain data the call fills in; `fd` is open.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    check(unsafe { libc::fstat(fd.as_raw_fd(), &mut st) })?;
    Ok(st)
}

fn facts_of(st: &libc::stat) -> FolderFacts {
    FolderFacts {
        uid: st.st_uid,
        mode: widen_mode(st.st_mode),
    }
}

/// `open` failed with ELOOP (Linux) or ENOTDIR (macOS, or a link to a file): say whether a link was in the way.
fn classify(e: io::Error, is_link: impl FnOnce() -> bool) -> Fail {
    match e.raw_os_error() {
        Some(libc::ELOOP | libc::ENOTDIR) => Fail::Refused(if is_link() {
            Refusal::Symlink
        } else {
            Refusal::NotAFolder
        }),
        _ => Fail::Io(e),
    }
}

impl Dir {
    /// Opens a folder by path. With `follow` a link as the last part is followed; otherwise it is refused. Ancestors are always
    /// resolved by name (the root of the chain cannot be anchored to anything).
    pub(super) fn open_path(path: &Path, follow: bool) -> Result<Dir, Fail> {
        let c = cstring(path.as_os_str())?;
        let flags = libc::O_RDONLY
            | libc::O_DIRECTORY
            | libc::O_CLOEXEC
            | if follow { 0 } else { libc::O_NOFOLLOW };
        // SAFETY: `c` is a NUL-terminated path.
        let fd = unsafe { libc::open(c.as_ptr(), flags) };
        match own(fd) {
            Ok(fd) => Ok(Dir(fd)),
            Err(e) => Err(classify(e, || {
                !follow && std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
            })),
        }
    }

    /// The folder `name` inside this one, a link refused.
    pub(super) fn open_at(&self, name: &std::ffi::OsStr) -> Result<Dir, Fail> {
        let c = cstring(name)?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        // SAFETY: `c` is NUL-terminated; the descriptor is open.
        let fd = unsafe { libc::openat(self.0.as_raw_fd(), c.as_ptr(), flags) };
        match own(fd) {
            Ok(fd) => Ok(Dir(fd)),
            Err(e) => Err(classify(e, || self.is_link(&c))),
        }
    }

    fn is_link(&self, c: &CString) -> bool {
        // SAFETY: plain data filled in by the call; `c` is NUL-terminated.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let r = unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                c.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        r == 0 && kind(st.st_mode) == libc::S_IFLNK
    }

    /// This folder's real parent, found through the held descriptor and not through a name.
    pub(super) fn parent(&self) -> Result<Dir, Fail> {
        // SAFETY: a constant NUL-terminated name; the descriptor is open.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                c"..".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        Ok(Dir(own(fd)?))
    }

    /// Makes the folder `name` with mode 0700 and opens it. The mode is set again on the new handle, so it is exact whatever the
    /// umask was, and the folder is never open to others at any moment.
    pub(super) fn mkdir_at(&self, name: &std::ffi::OsStr) -> Result<Dir, Fail> {
        let c = cstring(name)?;
        // SAFETY: `c` is NUL-terminated; the descriptor is open. The 0700 mode is masked by the umask only downwards.
        check(unsafe { libc::mkdirat(self.0.as_raw_fd(), c.as_ptr(), 0o700) })?;
        let dir = self.open_at(name)?;
        // SAFETY: the descriptor is open.
        check(unsafe { libc::fchmod(dir.0.as_raw_fd(), 0o700) })?;
        Ok(dir)
    }

    /// Creates the file `name` exclusively with mode 0600, no link followed.
    pub(super) fn create_file_at(&self, name: &std::ffi::OsStr) -> Result<File, Fail> {
        let c = cstring(name)?;
        let flags =
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: `c` is NUL-terminated; the descriptor is open.
        let fd = own(unsafe {
            libc::openat(self.0.as_raw_fd(), c.as_ptr(), flags, 0o600 as libc::c_uint)
        })?;
        // SAFETY: the descriptor is open.
        check(unsafe { libc::fchmod(fd.as_raw_fd(), 0o600) })?;
        Ok(File::from(fd))
    }

    /// Opens the file `name` for reading and refuses it unless it is a regular file of `uid` closed to others.
    pub(super) fn open_file_at(&self, name: &std::ffi::OsStr, who: &Account) -> Result<File, Fail> {
        let c = cstring(name)?;
        // O_NONBLOCK: a planted pipe must not make the open wait; the type check below refuses it.
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        // SAFETY: `c` is NUL-terminated; the descriptor is open.
        let fd = own(unsafe { libc::openat(self.0.as_raw_fd(), c.as_ptr(), flags) }).map_err(
            |e| match e.raw_os_error() {
                Some(libc::ELOOP) => Fail::Refused(Refusal::Symlink),
                _ => Fail::Io(e),
            },
        )?;
        let st = fstat(&fd)?;
        if kind(st.st_mode) != libc::S_IFREG {
            return Err(Refusal::NotAFile.into());
        }
        file_decision(facts_of(&st), *who)?;
        acl_check(&fd, AclScope::File, *who)?;
        Ok(File::from(fd))
    }

    /// Renames inside this folder.
    pub(super) fn rename(
        &self,
        _file: &File,
        from: &std::ffi::OsStr,
        to: &std::ffi::OsStr,
    ) -> io::Result<()> {
        let (f, t) = (cstring(from)?, cstring(to)?);
        // SAFETY: both names are NUL-terminated; the descriptor is open.
        check(unsafe {
            libc::renameat(
                self.0.as_raw_fd(),
                f.as_ptr(),
                self.0.as_raw_fd(),
                t.as_ptr(),
            )
        })
        .map(drop)
    }

    /// Removes the file `name` from this folder.
    pub(super) fn unlink(&self, name: &std::ffi::OsStr) -> io::Result<()> {
        let c = cstring(name)?;
        // SAFETY: `c` is NUL-terminated; the descriptor is open.
        check(unsafe { libc::unlinkat(self.0.as_raw_fd(), c.as_ptr(), 0) }).map(drop)
    }

    /// Flushes the folder's entries to disk.
    pub(super) fn sync(&self) -> io::Result<()> {
        // SAFETY: the descriptor is open.
        check(unsafe { libc::fsync(self.0.as_raw_fd()) }).map(drop)
    }

    /// Owner and mode of this folder from its descriptor, plus the macOS ACL check.
    fn facts(&self, me: u32) -> Result<FolderFacts, Fail> {
        acl_check(&self.0, AclScope::Folder, me)?;
        Ok(facts_of(&fstat(&self.0)?))
    }

    /// The folder belongs to `who` and no one else can write to it.
    pub(super) fn check(&self, who: &Account) -> Result<(), Fail> {
        Ok(crate::folder::decide(self.facts(*who)?, *who)?)
    }

    /// The folder's parent cannot be used to rename or replace the folder.
    pub(super) fn check_parent(&self, who: &Account) -> Result<(), Fail> {
        let parent = self.parent()?;
        let wrap = |r| Refusal::Parent(Box::new(r));
        acl_check(&parent.0, AclScope::Parent, *who).map_err(wrap)?;
        Ok(parent_decision(facts_of(&fstat(&parent.0)?), *who).map_err(wrap)?)
    }
}

/// A file passes when it is the account's own and has no group or other bit.
pub(super) fn file_decision(facts: FolderFacts, uid: u32) -> Result<(), Refusal> {
    if facts.uid != uid {
        return Err(Refusal::OtherOwner { uid: facts.uid });
    }
    if facts.mode & 0o077 != 0 {
        return Err(Refusal::OpenToOthers { mode: facts.mode });
    }
    Ok(())
}

/// A parent passes when it belongs to the account or to root and others cannot rename or replace entries in it: no group or
/// world write bit, or the sticky bit (then only an entry's owner may rename it, and the folder is the account's own).
pub(super) fn parent_decision(facts: FolderFacts, uid: u32) -> Result<(), Refusal> {
    if facts.uid != uid && facts.uid != 0 {
        return Err(Refusal::OtherOwner { uid: facts.uid });
    }
    if facts.mode & 0o022 != 0 && facts.mode & 0o1000 == 0 {
        return Err(Refusal::OpenToOthers { mode: facts.mode });
    }
    Ok(())
}

/// Which rule an ACL is judged by: a folder like its mode bits (only write-type rights for others refuse; listing shows no
/// value), a file like its mode (any right for another principal refuses: the content is the secret).
#[derive(Clone, Copy)]
enum AclScope {
    Folder,
    File,
    /// The folder's parent: only the rights that let others delete, rename or take over a child, on entries that apply to the
    /// parent itself (inherit flags widen nothing here; the Windows `PARENT_WRITE` rule).
    Parent,
}

/// macOS extended ACLs: an allow entry for another principal is refused when it carries a right that matters for `scope`
/// (entries that only apply to children, and entries for the account itself, are ignored), naming the principal. The mode
/// bits do not show these. Linux POSIX ACLs show in the group bits through the mask, which the mode check already covers.
#[cfg(target_os = "macos")]
fn acl_check(fd: &OwnedFd, scope: AclScope, me: u32) -> Result<(), Refusal> {
    use std::ffi::c_void;
    type Ptr = *mut c_void;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_uint) -> Ptr;
        fn acl_get_entry(acl: Ptr, id: libc::c_int, entry: *mut Ptr) -> libc::c_int;
        fn acl_get_tag_type(entry: Ptr, tag: *mut libc::c_uint) -> libc::c_int;
        fn acl_get_permset(entry: Ptr, perms: *mut Ptr) -> libc::c_int;
        fn acl_get_perm_np(perms: Ptr, perm: libc::c_uint) -> libc::c_int;
        fn acl_get_flagset_np(entry: Ptr, flags: *mut Ptr) -> libc::c_int;
        fn acl_get_flag_np(flags: Ptr, flag: libc::c_uint) -> libc::c_int;
        fn acl_get_qualifier(entry: Ptr) -> Ptr;
        fn acl_free(obj: Ptr) -> libc::c_int;
        fn mbr_uuid_to_id(uu: *const u8, id: *mut u32, kind: *mut libc::c_int) -> libc::c_int;
    }
    const ACL_TYPE_EXTENDED: libc::c_uint = 0x100;
    const ACL_FIRST_ENTRY: libc::c_int = 0;
    const ACL_NEXT_ENTRY: libc::c_int = -1;
    const ACL_EXTENDED_ALLOW: libc::c_uint = 1;
    const ACL_ENTRY_FILE_INHERIT: libc::c_uint = 1 << 5;
    const ACL_ENTRY_DIRECTORY_INHERIT: libc::c_uint = 1 << 6;
    const ACL_ENTRY_ONLY_INHERIT: libc::c_uint = 1 << 8;
    // WRITE_DATA, DELETE, APPEND_DATA, DELETE_CHILD, WRITE_ATTRIBUTES, WRITE_EXTATTRIBUTES, WRITE_SECURITY, CHANGE_OWNER.
    const WRITE_RIGHTS: [libc::c_uint; 8] = [
        1 << 2,
        1 << 4,
        1 << 5,
        1 << 6,
        1 << 8,
        1 << 10,
        1 << 12,
        1 << 13,
    ];
    // The write rights plus READ_DATA, EXECUTE, READ_ATTRIBUTES, READ_EXTATTRIBUTES, READ_SECURITY: every right.
    const ALL_RIGHTS: [libc::c_uint; 13] = [
        1 << 2,
        1 << 4,
        1 << 5,
        1 << 6,
        1 << 8,
        1 << 10,
        1 << 12,
        1 << 13,
        1 << 1,
        1 << 3,
        1 << 7,
        1 << 9,
        1 << 11,
    ];
    // DELETE_CHILD, WRITE_SECURITY, CHANGE_OWNER: what lets another principal remove, replace or take over a child of the parent.
    const PARENT_RIGHTS: [libc::c_uint; 3] = [1 << 6, 1 << 12, 1 << 13];
    const ID_TYPE_UID: libc::c_int = 0;
    // SAFETY: a valid descriptor; a null result means no extended ACL.
    let acl = unsafe { acl_get_fd_np(fd.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        return Ok(());
    }
    let mut found = None;
    let mut id = ACL_FIRST_ENTRY;
    loop {
        let mut entry: Ptr = std::ptr::null_mut();
        // SAFETY: `acl` is a live ACL and `entry` a valid out pointer; the call returns non-zero after the last entry.
        if unsafe { acl_get_entry(acl, id, &mut entry) } != 0 {
            break;
        }
        id = ACL_NEXT_ENTRY;
        let mut tag: libc::c_uint = 0;
        // SAFETY: `entry` came from the call above.
        if unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != ACL_EXTENDED_ALLOW {
            continue;
        }
        let (mut flags, mut perms): (Ptr, Ptr) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: `entry` is live; the out pointers are valid.
        let has_flags = unsafe { acl_get_flagset_np(entry, &mut flags) } == 0;
        let flag = |f: libc::c_uint| has_flags && unsafe { acl_get_flag_np(flags, f) } == 1;
        let only_inherit = flag(ACL_ENTRY_ONLY_INHERIT);
        // What a new file or folder made here would inherit: it counts with every right, whatever the scope (a read entry
        // that new files inherit makes every file written here readable by that principal).
        let inherited = !matches!(scope, AclScope::Parent)
            && (flag(ACL_ENTRY_FILE_INHERIT) || flag(ACL_ENTRY_DIRECTORY_INHERIT));
        // SAFETY: as above.
        if unsafe { acl_get_permset(entry, &mut perms) } != 0 {
            continue;
        }
        // An entry for children with no inherit flag applies to nothing made here, and to the folder itself not at all.
        if only_inherit && !inherited {
            continue;
        }
        let every_right = matches!(scope, AclScope::File) || inherited;
        let rights: &[libc::c_uint] = match scope {
            AclScope::Parent => &PARENT_RIGHTS,
            _ if every_right => &ALL_RIGHTS,
            _ => &WRITE_RIGHTS,
        };
        // SAFETY: `perms` is the entry's live permission set.
        if !rights
            .iter()
            .any(|&r| unsafe { acl_get_perm_np(perms, r) } == 1)
        {
            continue;
        }
        // SAFETY: `entry` is live; a non-null qualifier is a 16-byte GUID the caller frees.
        let guid = unsafe { acl_get_qualifier(entry) };
        let who = if guid.is_null() {
            "an unnamed principal".to_owned()
        } else {
            let (mut who_id, mut kind) = (0u32, 0 as libc::c_int);
            // SAFETY: `guid` points at 16 bytes; the out pointers are valid.
            let known = unsafe { mbr_uuid_to_id(guid.cast::<u8>(), &mut who_id, &mut kind) } == 0;
            // SAFETY: `guid` came from acl_get_qualifier and is freed once.
            unsafe { acl_free(guid) };
            match (known, kind) {
                (true, ID_TYPE_UID) if who_id == me => continue,
                (true, ID_TYPE_UID) => format!("user {who_id}"),
                (true, _) if who_id == 12 => "group 12 (everyone)".to_owned(),
                (true, _) => format!("group {who_id}"),
                (false, _) => "an unknown principal".to_owned(),
            }
        };
        found = Some(if inherited && matches!(scope, AclScope::Folder) {
            format!("{who} through an entry that new files and folders inherit")
        } else {
            who
        });
        break;
    }
    // SAFETY: `acl` is freed once.
    unsafe { acl_free(acl) };
    match found {
        Some(who) => Err(Refusal::OpenToOthersAcl { who }),
        None => Ok(()),
    }
}

#[cfg(not(target_os = "macos"))]
fn acl_check(_fd: &OwnedFd, _scope: AclScope, _me: u32) -> Result<(), Refusal> {
    Ok(())
}

/// Who owns what is trusted: the user id.
pub(super) type Account = u32;

/// The account running this process.
pub(super) fn current_account() -> io::Result<Account> {
    Ok(crate::folder::current_uid())
}
