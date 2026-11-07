//! Owned handles, SIDs and security descriptors (native glue: no decisions).

use super::wide::{from_wide, to_wide};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFinalPathNameByHandleW, OPEN_EXISTING,
    VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// A kernel handle, closed on drop.
pub struct Owned(pub HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the handle is owned and closed once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// The last Win32 error.
pub fn last_error() -> u32 {
    // SAFETY: no arguments.
    unsafe { GetLastError() }
}

/// Opens a file or folder for attribute and security reads without following a final reparse point.
pub fn open_for_query(path: &str) -> Result<Owned, u32> {
    let wide = to_wide(path);
    // SAFETY: `wide` is NUL-terminated; the flags open a directory too and do not follow a final link.
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        Err(last_error())
    } else {
        Ok(Owned(h))
    }
}

/// `GetFinalPathNameByHandleW` with DOS volume names, `\\?\` prefix removed for drive paths.
pub fn final_path(h: &Owned) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    // SAFETY: `buf` is a valid buffer of the stated length.
    let n = unsafe {
        GetFinalPathNameByHandleW(h.0, buf.as_mut_ptr(), buf.len() as u32, VOLUME_NAME_DOS)
    } as usize;
    if n == 0 || n >= buf.len() {
        return None;
    }
    Some(from_wide(&buf[..n]))
}

/// The string form of a SID.
pub fn sid_string(sid: PSID) -> Option<String> {
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: `sid` is a valid SID owned by the caller; `out` is freed with LocalFree.
    if unsafe { ConvertSidToStringSidW(sid, &mut out) } == 0 || out.is_null() {
        return None;
    }
    // SAFETY: `out` is NUL-terminated.
    let len = (0..).take_while(|&i| unsafe { *out.add(i) } != 0).count();
    let s = from_wide(unsafe { std::slice::from_raw_parts(out, len) });
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(out.cast()) };
    Some(s)
}

/// The owner SID of an open file or folder.
pub fn owner_sid(h: &Owned) -> Option<String> {
    let mut owner: PSID = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: out pointers are valid; the descriptor is freed below.
    let rc = unsafe {
        GetSecurityInfo(
            h.0,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut sd,
        )
    };
    if rc != 0 || owner.is_null() {
        return None;
    }
    let s = sid_string(owner);
    // SAFETY: allocated by GetSecurityInfo.
    unsafe { LocalFree(sd) };
    s
}

/// The user SID of a process token (`token` opened with `TOKEN_QUERY`).
pub fn token_user_sid(token: &Owned) -> Option<String> {
    let mut len = 0u32;
    // SAFETY: a size query.
    unsafe { GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut len) };
    if len == 0 || len > 4096 {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: `buf` has `len` bytes.
    if unsafe { GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) }
        == 0
    {
        return None;
    }
    // SAFETY: the buffer starts with a TOKEN_USER (aligned by Vec's allocation for u8? read unaligned to be safe).
    let user = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<TOKEN_USER>()) };
    sid_string(user.User.Sid)
}

/// The SID of the account running this process.
pub fn current_account() -> Option<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the current process pseudo-handle and a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return None;
    }
    token_user_sid(&Owned(token))
}

/// `DOMAIN\name` of a SID (for the message only).
pub fn account_name(sid: &str) -> Option<String> {
    let wide = to_wide(sid);
    let mut psid: PSID = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated; the SID is freed below.
    if unsafe {
        windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW(
            wide.as_ptr(),
            &mut psid,
        )
    } == 0
    {
        return None;
    }
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut nl, mut dl) = (name.len() as u32, domain.len() as u32);
    let mut kind = 0;
    // SAFETY: buffers and lengths agree.
    let ok = unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            psid,
            name.as_mut_ptr(),
            &mut nl,
            domain.as_mut_ptr(),
            &mut dl,
            &mut kind,
        )
    };
    // SAFETY: allocated by ConvertStringSidToSidW.
    unsafe { LocalFree(psid) };
    (ok != 0).then(|| format!("{}\\{}", from_wide(&domain), from_wide(&name)))
}

/// A security descriptor made from SDDL, freed on drop, to pass as `lpSecurityAttributes`.
pub struct Descriptor {
    sd: PSECURITY_DESCRIPTOR,
}

impl Descriptor {
    /// Parses `sddl`.
    pub fn from_sddl(sddl: &str) -> Result<Descriptor, u32> {
        let wide = to_wide(sddl);
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated; `sd` is freed in Drop.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            Err(last_error())
        } else {
            Ok(Descriptor { sd })
        }
    }

    /// Attributes for `CreateDirectoryW` or `CreateMutexW`.
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd,
            bInheritHandle: 0,
        }
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(self.sd) };
    }
}
