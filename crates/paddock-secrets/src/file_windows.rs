//! Windows: a secret file created owner-only and unshared, and the owner and DACL of an open file or folder as the pure
//! judgement in `acl.rs` reads them. Every `unsafe` block says why it is sound.
#![allow(unsafe_code)]

use crate::acl::{Ace, AceKind, Who};
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, GetTokenInformation, INHERIT_ONLY_ACE,
    IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TOKEN_USER, TokenUser, WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid,
    WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

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

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

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

/// The token user's SID of this process, as SDDL text.
fn current_user_sid() -> io::Result<String> {
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

/// Who `sid` is, for the judgement.
fn who(sid: PSID, me: &str) -> io::Result<Who> {
    // SAFETY: `sid` is valid for the duration of the calls.
    let known = unsafe {
        if IsWellKnownSid(sid, WinLocalSystemSid) != 0 {
            Some(Who::System)
        } else if IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0 {
            Some(Who::Admins)
        } else if IsWellKnownSid(sid, WinCreatorOwnerRightsSid) != 0 {
            Some(Who::OwnerRights)
        } else {
            None
        }
    };
    match known {
        Some(w) => Ok(w),
        None => Ok(if sid_text(sid)? == me {
            Who::Me
        } else {
            Who::Other
        }),
    }
}

/// Creates `tmp` new, with a protected DACL that grants only the user running this process, and with no sharing: no
/// other principal can open it (the DACL), and no other open of it succeeds while this handle lives (the share mode).
pub(crate) fn create_owner_only(tmp: &Path) -> io::Result<fs::File> {
    let sddl: Vec<u16> = format!("D:P(A;;FA;;;{})", current_user_sid()?)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `sddl` is NUL-terminated and outlives the call; `sd` is freed by `Local`.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let _free = Local(sd);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    let name = wide(tmp);
    // SAFETY: a NUL-terminated path and attributes valid for the duration of the call.
    let h = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE | READ_CONTROL,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a fresh, valid handle nobody else owns.
    Ok(fs::File::from(unsafe { OwnedHandle::from_raw_handle(h) }))
}

/// The owner and DACL entries of an open handle (opened with `READ_CONTROL`). A NULL DACL is one entry that lets
/// everyone do everything.
fn security(handle: HANDLE) -> io::Result<(Who, Vec<Ace>)> {
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
    let me = current_user_sid()?;
    let owner = if owner.is_null() {
        Who::Other
    } else {
        who(owner, &me)?
    };
    if dacl.is_null() {
        let everyone = Ace {
            kind: AceKind::Allow,
            who: Who::Other,
            mask: crate::acl::FULL,
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
                who: who(sid, &me)?,
                mask,
                inherit_only,
            }
        } else if OTHER_ALLOW_TYPES.contains(&header.AceType) {
            Ace {
                kind: AceKind::OtherAllow,
                who: Who::Other,
                mask: crate::acl::FULL,
                inherit_only,
            }
        } else {
            Ace {
                kind: AceKind::Deny,
                who: Who::Other,
                mask: 0,
                inherit_only,
            }
        };
        aces.push(entry);
    }
    Ok((owner, aces))
}

/// The owner and DACL of an open secret file.
pub(crate) fn file_security(file: &fs::File) -> io::Result<(Who, Vec<Ace>)> {
    security(file.as_raw_handle())
}

/// The owner and DACL of a folder, read from a handle to it (a folder needs `FILE_FLAG_BACKUP_SEMANTICS` to be opened).
pub(crate) fn folder_security(dir: &Path) -> io::Result<(Who, Vec<Ace>)> {
    let folder = fs::OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?;
    security(folder.as_raw_handle())
}

/// Opens `path` for reading without following a symbolic link or junction: a reparse point is opened as itself and
/// reported as `Ok(None)`, so the caller refuses it.
pub(crate) fn open_no_reparse(path: &Path) -> io::Result<Option<fs::File>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    Ok((meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0).then_some(file))
}

/// These run only on a real Windows machine: the `windows-latest` leg of the crates CI job (`cargo nextest` over the
/// crates workspace) runs them; on another OS this module is not compiled.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{Refusal, file_refusal, folder_refusal};
    use windows_sys::Win32::Security::Authorization::SetNamedSecurityInfoW;
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    fn set_sddl(path: &Path, sddl: &str) {
        let w: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: test-only; a NUL-terminated SDDL and path, the descriptor freed at the end.
        unsafe {
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    w.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    null_mut()
                ),
                0
            );
            let (mut present, mut defaulted, mut dacl) = (0, 0, null_mut::<ACL>());
            assert_ne!(
                GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted),
                0
            );
            let name = wide(path);
            let rc = SetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null_mut(),
            );
            assert_eq!(rc, 0);
            LocalFree(sd);
        }
    }

    /// Review P1-1: the temporary file is owner-only from its creation and cannot be opened while it is written.
    #[test]
    fn the_temporary_file_is_owner_only_and_unshared_from_its_creation() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("admin.key");
        paddock_env::write_atomic_created(&target, b"pw_x", &mut |tmp| {
            let f = create_owner_only(tmp)?;
            assert!(
                fs::File::open(tmp).is_err(),
                "nobody can open it for reading while it is written"
            );
            let (owner, aces) = file_security(&f)?;
            assert_eq!(file_refusal(owner, &aces), None);
            assert!(
                aces.iter().all(|a| a.who == Who::Me),
                "only the user: {aces:?}"
            );
            Ok(f)
        })
        .unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "pw_x");
    }

    #[test]
    fn a_written_secret_is_owner_only_and_a_shared_one_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let f = crate::SecretFile::new(d.path().join("admin.key"));
        f.write(&crate::Secret::new("pw_x").unwrap()).unwrap();
        let (owner, aces) = file_security(&fs::File::open(f.path()).unwrap()).unwrap();
        assert_eq!(file_refusal(owner, &aces), None);
        assert_eq!(
            f.read().unwrap().unwrap(),
            crate::Secret::new("pw_x").unwrap()
        );
        set_sddl(f.path(), "D:P(A;;FA;;;WD)");
        assert!(matches!(
            f.read(),
            Err(crate::SecretsError::InsecureFile(_))
        ));
        // The usual per-user folder entries (the user, SYSTEM, Administrators): trusted, as 1.0's files have them.
        set_sddl(
            f.path(),
            &format!(
                "D:P(A;;FA;;;{})(A;;FA;;;SY)(A;;FA;;;BA)",
                current_user_sid().unwrap()
            ),
        );
        assert!(f.read().unwrap().is_some());
        f.write(&crate::Secret::new("pw_y").unwrap()).unwrap();
        assert_eq!(
            f.read().unwrap().unwrap(),
            crate::Secret::new("pw_y").unwrap()
        );
    }

    /// Review P1-1: a folder where Authenticated Users may create files (a second NTFS drive's default) is refused.
    #[test]
    fn a_folder_others_can_write_in_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("data");
        fs::create_dir(&dir).unwrap();
        let me = current_user_sid().unwrap();
        set_sddl(
            &dir,
            &format!("D:P(A;OICI;FA;;;{me})(A;OICI;0x1301bf;;;AU)"),
        );
        let (owner, aces) = folder_security(&dir).unwrap();
        assert_eq!(folder_refusal(owner, &aces), Some(Refusal::OthersWrite));
        let e = crate::SecretFile::new(dir.join("admin.key"))
            .write(&crate::Secret::new("pw_x").unwrap())
            .unwrap_err();
        assert!(
            e.to_string().contains("other users create or change files"),
            "{e}"
        );
        set_sddl(&dir, &format!("D:P(A;OICI;FA;;;{me})(A;OICI;FA;;;SY)"));
        crate::SecretFile::new(dir.join("admin.key"))
            .write(&crate::Secret::new("pw_x").unwrap())
            .unwrap();
    }

    #[test]
    fn the_current_user_sid_is_a_user_sid() {
        assert!(current_user_sid().unwrap().starts_with("S-1-5-"));
    }
}
