//! Windows: the owner-only DACL of a secret file, and its check on read. Every `unsafe` block says why it is sound.
#![allow(unsafe_code)]

use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetSecurityDescriptorDacl,
    GetTokenInformation, INHERIT_ONLY_ACE, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    TokenUser, WinBuiltinAdministratorsSid, WinCreatorOwnerRightsSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// `ACCESS_ALLOWED_ACE_TYPE` (`winnt.h`): the one allow type a file's own DACL uses.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
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

/// A security descriptor (or SID text) allocated by Windows with `LocalAlloc`, freed on drop.
struct Local(*mut core::ffi::c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by a Windows API that hands out LocalAlloc memory, freed once here.
            unsafe { LocalFree(self.0) };
        }
    }
}

/// The token user's SID of this process, as SDDL text (`S-1-5-21-...`).
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
    // SAFETY: the buffer starts with a TOKEN_USER filled in by the call above; its SID points into the same buffer.
    let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: *mut u16 = null_mut();
    // SAFETY: `sid` is valid while `buf` lives; `text` is freed by `Local`.
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

/// Gives `path` a protected DACL that grants only the user running this process (no inherited entries, so neither the
/// folder's users nor SYSTEM nor Administrators keep access through it).
pub(crate) fn restrict_to_owner(path: &Path) -> io::Result<()> {
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
    let (mut present, mut defaulted, mut dacl) = (0, 0, null_mut::<ACL>());
    // SAFETY: `sd` is a valid descriptor; the out pointers are valid.
    if unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) } == 0
        || present == 0
    {
        return Err(io::Error::last_os_error());
    }
    let name = wide(path);
    // SAFETY: a NUL-terminated path and a DACL that lives as long as `sd`.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    Ok(())
}

/// Whether the DACL of `path` lets anyone read it besides its owner, the user running this process, SYSTEM and
/// Administrators (the same trust as the per-user data folder). A NULL DACL (everyone) counts as shared.
pub(crate) fn shared_with_others(path: &Path) -> io::Result<bool> {
    let name = wide(path);
    let (mut owner, mut dacl, mut sd): (PSID, *mut ACL, PSECURITY_DESCRIPTOR) =
        (null_mut(), null_mut(), null_mut());
    // SAFETY: a NUL-terminated path and valid out pointers; `sd` (which holds `owner` and `dacl`) is freed by `Local`.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
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
    if dacl.is_null() {
        return Ok(true);
    }
    let me = current_user_sid()?;
    // SAFETY: `dacl` points into `sd`, which lives until the end of this function.
    let count = unsafe { (*dacl).AceCount };
    for i in 0..u32::from(count) {
        let mut ace: *mut core::ffi::c_void = null_mut();
        // SAFETY: `i` is below the ACE count of a valid ACL.
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 || ace.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: every ACE starts with an ACE_HEADER.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 {
            continue; // applies to children only, not to this file
        }
        match header.AceType {
            ACCESS_ALLOWED_ACE_TYPE => {}
            // The other allow types (compound, object, callback, callback object): not ones a file of ours has, so shared.
            4 | 5 | 9 | 11 => return Ok(true),
            // Deny and audit types only take access away or log it.
            _ => continue,
        }
        // SAFETY: an ACCESS_ALLOWED_ACE: the SID starts at `sid_start`, inside the ACE.
        let sid: PSID = unsafe { core::ptr::addr_of!((*ace.cast::<AllowedAce>()).sid_start) }
            .cast_mut()
            .cast();
        if !trusted(sid, owner, &me)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether an allowed SID is one the secret may be shared with.
fn trusted(sid: PSID, owner: PSID, me: &str) -> io::Result<bool> {
    // SAFETY: both SIDs are valid for the duration of the calls.
    let well_known = unsafe {
        (!owner.is_null() && EqualSid(sid, owner) != 0)
            || IsWellKnownSid(sid, WinLocalSystemSid) != 0
            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
            || IsWellKnownSid(sid, WinCreatorOwnerRightsSid) != 0
    };
    if well_known {
        return Ok(true);
    }
    let mut text: *mut u16 = null_mut();
    // SAFETY: a valid SID; `text` is freed by `Local`.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(io::Error::last_os_error());
    }
    let _free = Local(text.cast());
    // SAFETY: `text` is NUL-terminated.
    let n = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: `n` u16s were just read.
    Ok(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, n) }) == me)
}

/// Opens `path` for reading without following a symbolic link or junction: a reparse point is opened as itself and
/// reported as `Ok(None)` inside, so the caller refuses it.
pub(crate) fn open_no_reparse(path: &Path) -> io::Result<Option<fs::File>> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let meta = file.metadata()?;
    Ok((meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0).then_some(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_sddl(path: &Path, sddl: &str) {
        let w: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: test-only, same calls as restrict_to_owner.
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
            assert_eq!(
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    dacl,
                    null_mut()
                ),
                0
            );
            LocalFree(sd);
        }
    }

    #[test]
    fn a_written_secret_has_an_owner_only_dacl_and_a_shared_one_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let f = crate::SecretFile::new(d.path().join("admin.key"));
        f.write(&crate::Secret::new("pw_x").unwrap()).unwrap();
        assert!(
            !shared_with_others(f.path()).unwrap(),
            "only the owner after a write"
        );
        assert_eq!(
            f.read().unwrap().unwrap(),
            crate::Secret::new("pw_x").unwrap()
        );
        // Everyone may read it: refused, with the path and no value.
        set_sddl(f.path(), "D:P(A;;FA;;;WD)");
        assert!(shared_with_others(f.path()).unwrap());
        let e = f.read().unwrap_err();
        assert!(matches!(e, crate::SecretsError::InsecureFile(_)), "{e}");
        // The usual per-user folder entries (the user, SYSTEM, Administrators): trusted, as the 1.0 files have them.
        set_sddl(
            f.path(),
            &format!(
                "D:P(A;;FA;;;{})(A;;FA;;;SY)(A;;FA;;;BA)",
                current_user_sid().unwrap()
            ),
        );
        assert!(!shared_with_others(f.path()).unwrap());
        // Rewriting makes it owner-only again.
        f.write(&crate::Secret::new("pw_y").unwrap()).unwrap();
        assert!(!shared_with_others(f.path()).unwrap());
        assert_eq!(
            f.read().unwrap().unwrap(),
            crate::Secret::new("pw_y").unwrap()
        );
    }

    #[test]
    fn the_current_user_sid_is_a_user_sid() {
        assert!(current_user_sid().unwrap().starts_with("S-1-5-"));
    }
}
