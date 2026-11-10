//! Windows: folders and files are created with `NtCreateFile` relative to a held folder handle (`OBJECT_ATTRIBUTES.RootDirectory`)
//! with the owner-only security descriptor in the same call, so no object is ever visible with an inherited, wider access list;
//! links and junctions are opened as themselves (`FILE_OPEN_REPARSE_POINT`) and refused; owner and DACL are read from the handle.
//!
//! Folder handles are held without `FILE_SHARE_DELETE`: while one is held, the folder and its ancestors cannot be renamed or
//! deleted, so the held handle cannot be made to name another place (ADR 0092 D10 (b) as a second layer behind D10 (a)).
//! Every `unsafe` block says why it is sound.
#![allow(unsafe_code)]

use super::acl::{Ace, AceKind, Who};
use super::error::Fail;
use crate::folder::Refusal;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DELETE_ON_CLOSE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
    FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT,
    FileRenameInformation, FileRenameInformationEx, NtCreateFile, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    RtlNtStatusToDosError, STATUS_FILE_IS_A_DIRECTORY, STATUS_INVALID_PARAMETER,
    STATUS_NOT_A_DIRECTORY, UNICODE_STRING,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetTokenInformation, INHERIT_ONLY_ACE,
    IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY,
    TOKEN_USER, TokenUser, WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid,
    WinCreatorOwnerSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    FILE_WRITE_ATTRIBUTES, FileBasicInfo, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    OPEN_EXISTING, READ_CONTROL, SYNCHRONIZE, SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// What a held folder handle is opened with: list, traverse (a relative open needs it), read attributes and the descriptor.
const DIR_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE;

/// The SID of `NT SERVICE\\TrustedInstaller`, the owner of system folders.
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

/// `OBJ_CASE_INSENSITIVE` (`ntdef.h`).
const OBJ_CASE_INSENSITIVE: u32 = 0x40;
/// `FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS` (`ntifs.h`): replace in one step even while the target is open.
const RENAME_REPLACE_POSIX: u32 = 0x1 | 0x2;
/// `ACCESS_ALLOWED_ACE_TYPE` (`winnt.h`).
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
/// The other allow types: compound, object, callback, callback object (`winnt.h`).
const OTHER_ALLOW_TYPES: [u8; 4] = [4, 5, 9, 11];

/// The layout of an `ACCESS_ALLOWED_ACE` up to its SID (`winnt.h`): header, mask, then the SID's first word.
#[repr(C)]
struct AllowedAce {
    header: ACE_HEADER,
    mask: u32,
    sid_start: u32,
}

/// Who owns what is trusted: the user's SID in text form.
pub(super) type Account = String;

/// A held folder.
#[derive(Debug)]
pub(super) struct Dir(OwnedHandle);

/// Memory allocated by Windows with `LocalAlloc`, freed on drop.
struct Local(*mut core::ffi::c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by a Windows API that hands out LocalAlloc memory, freed once here.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().collect()
}

/// An NT status as a failure: `STATUS_NOT_A_DIRECTORY` is a refusal, anything else an OS error.
fn nt(status: i32) -> Fail {
    if status == STATUS_NOT_A_DIRECTORY {
        return Fail::Refused(Refusal::NotAFolder);
    }
    if status == STATUS_FILE_IS_A_DIRECTORY {
        return Fail::Refused(Refusal::NotAFile);
    }
    // SAFETY: a pure status-to-error mapping.
    let code = unsafe { RtlNtStatusToDosError(status) };
    Fail::Io(io::Error::from_raw_os_error(code as i32))
}

/// The SDDL text of a SID (`S-1-5-21-...`).
fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text: *mut u16 = null_mut();
    // SAFETY: a valid SID; `text` is freed by `Local`.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(io::Error::last_os_error());
    }
    let _free = Local(text.cast());
    // SAFETY: `text` is NUL-terminated.
    let n = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: `n` u16s were just read.
    Ok(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(text, n)
    }))
}

/// The account running this process: the token user's SID as text.
pub(super) fn current_account() -> io::Result<Account> {
    let mut token: HANDLE = null_mut();
    // SAFETY: the current process pseudo-handle and a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0u32;
    // SAFETY: a size query on a valid token.
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len) };
    let mut buf = vec![0u64; (len as usize).div_ceil(8).max(1)];
    // SAFETY: `buf` holds at least `len` bytes, 8-byte aligned (enough for TOKEN_USER).
    let ok =
        unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) };
    let err = io::Error::last_os_error();
    // SAFETY: the token opened above, closed once.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(err);
    }
    // SAFETY: the buffer starts with a TOKEN_USER filled in above; its SID points into the same buffer.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    sid_text(sid)
}

/// The owner-only descriptor in SDDL: owned by `me`, protected (nothing inherited), full control for `me` and SYSTEM. A folder's
/// entries are inherited by what is made in it.
fn sddl(me: &str, folder: bool) -> String {
    let flags = if folder { "OICI" } else { "" };
    format!("O:{me}D:P(A;{flags};FA;;;{me})(A;{flags};FA;;;SY)")
}

/// A security descriptor converted from SDDL, freed on drop.
struct Descriptor(Local);

impl Descriptor {
    fn owner_only(me: &str, folder: bool) -> io::Result<Descriptor> {
        let text: Vec<u16> = sddl(me, folder)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `text` is NUL-terminated and outlives the call; `sd` is freed by `Local`.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Descriptor(Local(sd)))
    }
}

/// What `NtCreateFile` is asked for.
struct Request<'a> {
    name: &'a OsStr,
    access: u32,
    attributes: u32,
    share: u32,
    disposition: u32,
    options: u32,
    descriptor: Option<&'a Descriptor>,
}

impl Dir {
    /// `NtCreateFile` relative to this folder (or to nothing for a full NT path).
    fn nt_create(&self, r: &Request<'_>) -> Result<OwnedHandle, Fail> {
        let name = wide(r.name);
        let bytes = u16::try_from(name.len() * 2)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let unicode = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: name.as_ptr().cast_mut(),
        };
        let attrs = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: self.0.as_raw_handle(),
            ObjectName: &unicode,
            Attributes: OBJ_CASE_INSENSITIVE,
            SecurityDescriptor: r.descriptor.map_or(null(), |d| d.0.0.cast_const().cast()),
            SecurityQualityOfService: null(),
        };
        let mut handle: HANDLE = null_mut();
        // SAFETY: a zeroed status block is filled in by the call.
        let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        // SAFETY: every pointer is valid for the call (`unicode` and `name` live to the end of this function, the descriptor lives as
        // long as the request); the root handle is open.
        let rc = unsafe {
            NtCreateFile(
                &mut handle,
                r.access,
                &attrs,
                &mut status,
                null(),
                r.attributes,
                r.share,
                r.disposition,
                r.options | FILE_SYNCHRONOUS_IO_NONALERT,
                null(),
                0,
            )
        };
        if rc < 0 {
            return Err(nt(rc));
        }
        // SAFETY: a fresh, valid handle nobody else owns.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    /// [`Dir::ensure_not_indexed`], where a failure is logged and not fatal: the access list is already owner-only, so a search
    /// indexer that does read the content runs as the same account; refusing to create the object would help nobody (ER-27: the
    /// line names no path).
    fn not_indexed_or_warn(handle: &OwnedHandle) {
        if let Err(e) = Self::ensure_not_indexed(handle) {
            log::warn!(
                "Could not mark a new private folder or file as not content-indexed ({}); it stays owner-only",
                match e {
                    Fail::Io(e) => e.to_string(),
                    Fail::Refused(r) => r.reason().to_owned(),
                }
            );
        }
    }

    /// Marks the new object as not to be content-indexed (a created folder's flag is inherited by what is made in it). The
    /// descriptor already holds the access list; this only keeps the search indexer from reading the content.
    fn ensure_not_indexed(handle: &OwnedHandle) -> Result<(), Fail> {
        let attrs = Self::attributes(handle)?;
        if attrs & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED != 0 {
            return Ok(());
        }
        // A zero time means "leave unchanged".
        let info = FILE_BASIC_INFO {
            CreationTime: 0,
            LastAccessTime: 0,
            LastWriteTime: 0,
            ChangeTime: 0,
            FileAttributes: attrs | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
        };
        // SAFETY: a valid handle opened with FILE_WRITE_ATTRIBUTES and a complete FILE_BASIC_INFO.
        let ok = unsafe {
            SetFileInformationByHandle(
                handle.as_raw_handle(),
                FileBasicInfo,
                std::ptr::addr_of!(info).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }

    /// The handle's attributes: refuses a reparse point (link, junction, mount point) when `follow` is not set.
    fn attributes(handle: &OwnedHandle) -> Result<u32, Fail> {
        // SAFETY: plain data filled in by the call.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: a valid handle and out pointer.
        if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(info.dwFileAttributes)
    }

    /// Opens a folder by path. With `follow` a link as the last part is followed; otherwise it is refused. Ancestors are always
    /// resolved by name. The handle shares read and write but not delete.
    pub(super) fn open_path(path: &Path, follow: bool) -> Result<Dir, Fail> {
        let mut name = wide(path.as_os_str());
        name.push(0);
        let flags = FILE_FLAG_BACKUP_SEMANTICS
            | if follow {
                0
            } else {
                FILE_FLAG_OPEN_REPARSE_POINT
            };
        // SAFETY: a NUL-terminated path; no security attributes.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                DIR_ACCESS,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                flags,
                null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: a fresh, valid handle nobody else owns.
        let handle = unsafe { OwnedHandle::from_raw_handle(h) };
        let attrs = Self::attributes(&handle)?;
        if !follow && attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Refusal::ReparsePoint.into());
        }
        if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(Refusal::NotAFolder.into());
        }
        Ok(Dir(handle))
    }

    /// The folder `name` inside this one, a link or junction refused.
    pub(super) fn open_at(&self, name: &OsStr) -> Result<Dir, Fail> {
        let handle = self.nt_create(&Request {
            name,
            access: DIR_ACCESS,
            attributes: 0,
            share: FILE_SHARE_READ | FILE_SHARE_WRITE,
            disposition: FILE_OPEN,
            options: FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            descriptor: None,
        })?;
        if Self::attributes(&handle)? & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Refusal::ReparsePoint.into());
        }
        Ok(Dir(handle))
    }

    /// This folder's parent, found from the held handle's final path (the handle cannot be renamed while held, so the path
    /// names where it is) and opened as itself.
    pub(super) fn parent(&self) -> Result<Dir, Fail> {
        let mut buf = vec![0u16; 1024];
        let len = loop {
            // SAFETY: a valid handle and a buffer of the stated length.
            let n = unsafe {
                GetFinalPathNameByHandleW(
                    self.0.as_raw_handle(),
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    0,
                )
            };
            if n == 0 {
                return Err(io::Error::last_os_error().into());
            }
            if (n as usize) < buf.len() {
                break n as usize;
            }
            buf.resize(n as usize + 1, 0);
        };
        let path = PathBuf::from(std::ffi::OsString::from_wide(&buf[..len]));
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Dir::open_path(parent, false)
    }

    /// Makes the folder `name` with the owner-only descriptor in the creating call and opens it.
    pub(super) fn mkdir_at(&self, name: &OsStr) -> Result<Dir, Fail> {
        let me = current_account()?;
        let descriptor = Descriptor::owner_only(&me, true)?;
        let handle = self.nt_create(&Request {
            name,
            access: DIR_ACCESS | FILE_WRITE_ATTRIBUTES,
            attributes: FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
            share: FILE_SHARE_READ | FILE_SHARE_WRITE,
            disposition: FILE_CREATE,
            options: FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            descriptor: Some(&descriptor),
        })?;
        Self::not_indexed_or_warn(&handle);
        Ok(Dir(handle))
    }

    /// Creates the file `name` exclusively with the owner-only descriptor, no sharing, not content-indexed. The handle can be
    /// renamed (`DELETE`).
    pub(super) fn create_file_at(&self, name: &OsStr) -> Result<File, Fail> {
        let me = current_account()?;
        let descriptor = Descriptor::owner_only(&me, false)?;
        let handle = self.nt_create(&Request {
            name,
            access: GENERIC_WRITE
                | DELETE
                | READ_CONTROL
                | SYNCHRONIZE
                | FILE_READ_ATTRIBUTES
                | FILE_WRITE_ATTRIBUTES,
            attributes: FILE_ATTRIBUTE_NORMAL | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED,
            share: 0,
            disposition: FILE_CREATE,
            options: FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            descriptor: Some(&descriptor),
        })?;
        Self::not_indexed_or_warn(&handle);
        Ok(File::from(handle))
    }

    /// Opens the file `name` for reading and refuses it unless it is a regular file of `who` with no right for anyone else.
    pub(super) fn open_file_at(&self, name: &OsStr, who: &Account) -> Result<File, Fail> {
        let handle = self.nt_create(&Request {
            name,
            access: GENERIC_READ | READ_CONTROL | SYNCHRONIZE,
            attributes: 0,
            share: FILE_SHARE_READ | FILE_SHARE_DELETE,
            disposition: FILE_OPEN,
            options: FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT,
            descriptor: None,
        })?;
        if Self::attributes(&handle)? & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Refusal::ReparsePoint.into());
        }
        let (owner, aces) = security(handle.as_raw_handle(), who)?;
        super::acl::file_decision(&owner, &aces)?;
        Ok(File::from(handle))
    }

    /// Renames the open `file` (`from` is its name) to `to` inside this folder, replacing a file already there, relative to this
    /// folder's handle.
    pub(super) fn rename(&self, file: &File, _from: &OsStr, to: &OsStr) -> io::Result<()> {
        // `FileRenameInformationEx` (POSIX semantics) needs Windows 10 1809 or later; an older system answers
        // STATUS_INVALID_PARAMETER and the replace is retried with the plain class and `ReplaceIfExists`.
        let mut rc = self.rename_class(file, to, true);
        if rc == STATUS_INVALID_PARAMETER {
            rc = self.rename_class(file, to, false);
        }
        if rc < 0 {
            return Err(match nt(rc) {
                Fail::Io(e) => e,
                Fail::Refused(_) => io::Error::from(io::ErrorKind::InvalidInput),
            });
        }
        Ok(())
    }

    /// One rename call, `extended` for `FileRenameInformationEx`; the NT status.
    fn rename_class(&self, file: &File, to: &OsStr, extended: bool) -> i32 {
        let name = wide(to);
        let header = std::mem::size_of::<FILE_RENAME_INFORMATION>();
        let total = header + name.len() * 2;
        let mut buf = vec![0u64; total.div_ceil(8)];
        let info = buf.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: `buf` is 8-byte aligned and at least `header + name bytes` long; the name is copied after the fixed part.
        unsafe {
            if extended {
                (*info).Anonymous.Flags = RENAME_REPLACE_POSIX;
            } else {
                (*info).Anonymous.ReplaceIfExists = true;
            }
            (*info).RootDirectory = self.0.as_raw_handle();
            (*info).FileNameLength = (name.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                name.as_ptr(),
                std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
                name.len(),
            );
        }
        // SAFETY: a zeroed status block is filled in by the call.
        let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
        let class = if extended {
            FileRenameInformationEx
        } else {
            FileRenameInformation
        };
        // SAFETY: the file handle was opened with DELETE; `buf` holds a complete FILE_RENAME_INFORMATION.
        unsafe {
            NtSetInformationFile(
                file.as_raw_handle(),
                &mut status,
                buf.as_ptr().cast(),
                total as u32,
                class,
            )
        }
    }

    /// The plain-class rename, for the test of the fallback.
    #[cfg(test)]
    pub(super) fn rename_plain(&self, file: &File, to: &OsStr) -> io::Result<()> {
        match self.rename_class(file, to, false) {
            rc if rc < 0 => Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(rc) } as i32,
            )),
            _ => Ok(()),
        }
    }

    /// Removes the file `name` from this folder: opened with `DELETE` as itself, deleted when the handle closes.
    pub(super) fn unlink(&self, name: &OsStr) -> io::Result<()> {
        let handle = self.nt_create(&Request {
            name,
            access: DELETE | SYNCHRONIZE,
            attributes: 0,
            share: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            disposition: FILE_OPEN,
            options: FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_DELETE_ON_CLOSE,
            descriptor: None,
        });
        match handle {
            Ok(h) => {
                drop(h);
                Ok(())
            }
            Err(Fail::Io(e)) => Err(e),
            Err(Fail::Refused(_)) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
        }
    }

    /// Windows has no portable flush of a folder's entries (a flush needs a handle with write access and is not supported on
    /// every volume); NTFS journals the rename, so the replace is as durable as the file system makes a rename. Nothing to do.
    pub(super) fn sync(&self) -> io::Result<()> {
        Ok(())
    }

    /// The folder belongs to `who` and no one else can write to it.
    pub(super) fn check(&self, who: &Account) -> Result<(), Fail> {
        let (owner, aces) = security(self.0.as_raw_handle(), who)?;
        Ok(super::acl::folder_decision(&owner, &aces)?)
    }

    /// The folder's parent cannot be used to rename or replace the folder.
    pub(super) fn check_parent(&self, who: &Account) -> Result<(), Fail> {
        let parent = self.parent()?;
        let (owner, aces) = security(parent.0.as_raw_handle(), who)?;
        Ok(super::acl::parent_decision(&owner, &aces)?)
    }

    /// The owner and entries, for tests.
    #[cfg(test)]
    pub(super) fn security(&self, who: &Account) -> io::Result<(Who, Vec<Ace>)> {
        security(self.0.as_raw_handle(), who)
    }
}

/// The owner and entries of an open file, for tests (the file must have been opened with `READ_CONTROL`).
#[cfg(test)]
pub(super) fn file_security(file: &File, who: &Account) -> io::Result<(Who, Vec<Ace>)> {
    security(file.as_raw_handle(), who)
}

/// Who `sid` is, for the judgement.
fn who_is(sid: PSID, me: &str) -> io::Result<Who> {
    // SAFETY: `sid` is valid for the duration of the calls.
    let known = unsafe {
        if IsWellKnownSid(sid, WinLocalSystemSid) != 0 {
            Some(Who::System)
        } else if IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0 {
            Some(Who::Admins)
        } else if IsWellKnownSid(sid, WinCreatorOwnerRightsSid) != 0 {
            Some(Who::OwnerRights)
        } else if IsWellKnownSid(sid, WinCreatorOwnerSid) != 0 {
            Some(Who::CreatorOwner)
        } else {
            None
        }
    };
    match known {
        Some(w) => Ok(w),
        None => {
            let text = sid_text(sid)?;
            Ok(if text == me {
                Who::Me
            } else if text == TRUSTED_INSTALLER {
                Who::TrustedInstaller
            } else {
                Who::Other(text)
            })
        }
    }
}

/// The owner and DACL entries of an open handle (opened with `READ_CONTROL`). A NULL DACL is one entry that lets everyone do
/// everything.
fn security(handle: HANDLE, me: &str) -> io::Result<(Who, Vec<Ace>)> {
    let (mut owner, mut dacl, mut sd): (PSID, *mut ACL, PSECURITY_DESCRIPTOR) =
        (null_mut(), null_mut(), null_mut());
    // SAFETY: a valid handle and valid out pointers; `sd` (which holds `owner` and `dacl`) is freed by `Local`.
    let rc = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut sd,
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let _free = Local(sd);
    let owner = if owner.is_null() {
        Who::Other("(no owner)".to_owned())
    } else {
        who_is(owner, me)?
    };
    if dacl.is_null() {
        let everyone = Ace {
            kind: AceKind::Allow,
            who: Who::Other("S-1-1-0".to_owned()),
            mask: u32::MAX,
            inherit_only: false,
        };
        return Ok((owner, vec![everyone]));
    }
    // SAFETY: `dacl` points into `sd`, which lives until the end of this function.
    let count = unsafe { (*dacl).AceCount };
    let mut aces = Vec::with_capacity(usize::from(count));
    for i in 0..u32::from(count) {
        let mut ace: *mut core::ffi::c_void = null_mut();
        // SAFETY: `i` is below the ACE count of a valid ACL.
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 || ace.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: every ACE starts with an ACE_HEADER.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        let inherit_only = u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0;
        let entry = if header.AceType == ACCESS_ALLOWED_ACE_TYPE {
            // SAFETY: an ACCESS_ALLOWED_ACE: the mask follows the header and the SID starts at `sid_start`, inside the ACE.
            let (mask, sid) = unsafe {
                let a = ace.cast::<AllowedAce>();
                (
                    (*a).mask,
                    core::ptr::addr_of!((*a).sid_start)
                        .cast_mut()
                        .cast::<core::ffi::c_void>(),
                )
            };
            Ace {
                kind: AceKind::Allow,
                who: who_is(sid, me)?,
                mask,
                inherit_only,
            }
        } else if OTHER_ALLOW_TYPES.contains(&header.AceType) {
            Ace {
                kind: AceKind::OtherAllow,
                who: Who::Other("an object or callback entry".to_owned()),
                mask: u32::MAX,
                inherit_only,
            }
        } else {
            Ace {
                kind: AceKind::Deny,
                who: Who::Other(String::new()),
                mask: 0,
                inherit_only,
            }
        };
        aces.push(entry);
    }
    Ok((owner, aces))
}
