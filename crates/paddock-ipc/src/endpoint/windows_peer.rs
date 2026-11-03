//! Windows: who is on a pipe (the server's view) and who owns it (the client's view). Thin FFI around two pure decisions
//! ([`crate::identity::identify`], [`crate::trust::check_pipe_owner`]).
//!
//! The server never asks for the client's process id to learn who it is (`GetNamedPipeClientProcessId` is a hint for the
//! log): it impersonates the client for the moment it takes to read the client's own token (`OpenThreadToken`), takes the
//! account SID and the group list from that token, and reverts. The impersonation level is the one the client chose
//! (`SECURITY_IDENTIFICATION`), so the server can read the token but not act as the client (WP-0, ADR 0027).
#![allow(unsafe_code)]

use crate::identity::{AccountId, AdminEvidence, AuditHint, PeerError, RawPeer};
use crate::trust::Untrusted;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{
    ERROR_CANNOT_IMPERSONATE, ERROR_INSUFFICIENT_BUFFER, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, RevertToSelf,
    TOKEN_GROUPS, TOKEN_QUERY, TOKEN_USER, TokenGroups, TokenUser,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient};
use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

/// The Administrators group, which also marks a UAC-filtered token (the group is present with "deny only").
const ADMINISTRATORS_SID: &str = "S-1-5-32-544";

fn unreadable(what: &'static str) -> PeerError {
    PeerError::Unreadable {
        what,
        cause: io::Error::last_os_error().to_string(),
    }
}

/// A SID as the string form `S-1-5-...`.
fn sid_string(sid: PSID) -> Option<String> {
    let mut text: *mut u16 = null_mut();
    // SAFETY: `sid` is a valid SID for the call; the allocated string is freed below.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return None;
    }
    // SAFETY: `text` is NUL-terminated and valid until `LocalFree`.
    let s = unsafe {
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, n))
    };
    // SAFETY: allocated by `ConvertSidToStringSidW` above.
    unsafe {
        LocalFree(text.cast());
    }
    Some(s)
}

/// Ends the impersonation of the calling thread on every path out.
struct Revert;

impl Drop for Revert {
    fn drop(&mut self) {
        // SAFETY: always valid; failing means there was nothing to revert.
        unsafe {
            RevertToSelf();
        }
    }
}

/// `GetTokenInformation` into an aligned buffer of the size the OS asks for.
fn token_info(token: HANDLE, class: i32, what: &'static str) -> Result<Vec<u64>, PeerError> {
    let mut needed = 0u32;
    // SAFETY: a size query with no buffer.
    unsafe {
        GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0
        || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
    {
        return Err(unreadable(what));
    }
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buf` holds at least `needed` bytes and is 8-byte aligned.
    if unsafe { GetTokenInformation(token, class, buf.as_mut_ptr().cast(), needed, &mut needed) }
        == 0
    {
        return Err(unreadable(what));
    }
    Ok(buf)
}

/// The facts of the client on `pipe`. The documentation asks for one read first; the OS may answer `ERROR_CANNOT_IMPERSONATE`
/// before it, which is [`PeerError::NotReady`].
pub(super) fn raw_peer(pipe: HANDLE) -> Result<RawPeer, PeerError> {
    let mut pid = 0u32;
    // SAFETY: a connected pipe server handle and a valid out pointer. A hint for the log only.
    let pid = (unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } != 0).then_some(pid);
    // SAFETY: a connected pipe server handle.
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(match io::Error::last_os_error().raw_os_error() {
            Some(code) if code as u32 == ERROR_CANNOT_IMPERSONATE => PeerError::NotReady,
            _ => unreadable("ImpersonateNamedPipeClient"),
        });
    }
    let _revert = Revert;
    let mut token: HANDLE = null_mut();
    // SAFETY: opens the impersonation token of the calling thread as the process (`open as self`).
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return Err(unreadable("OpenThreadToken"));
    }
    // SAFETY: a fresh handle from the call above, closed when the wrapper drops.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let raw = token.as_raw_handle();

    let user = token_info(raw, TokenUser, "GetTokenInformation(TokenUser)")?;
    // SAFETY: the buffer holds a TOKEN_USER written by the call above.
    let account = sid_string(unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid })
        .ok_or_else(|| unreadable("ConvertSidToStringSid(user)"))?;

    let groups = token_info(raw, TokenGroups, "GetTokenInformation(TokenGroups)")?;
    // SAFETY: the buffer holds a TOKEN_GROUPS followed by `GroupCount` entries, written by the call above.
    let admin = unsafe {
        let groups = groups.as_ptr().cast::<TOKEN_GROUPS>();
        let entries =
            std::slice::from_raw_parts((*groups).Groups.as_ptr(), (*groups).GroupCount as usize);
        entries
            .iter()
            .filter_map(|g| sid_string(g.Sid))
            .any(|sid| sid == ADMINISTRATORS_SID)
    };
    Ok(RawPeer {
        account: AccountId::Sid(account),
        admin: AdminEvidence::Token(admin),
        audit: AuditHint::new(pid, None),
    })
}

/// The owner SID of the kernel object `handle` (a pipe the client opened), checked to be SYSTEM.
pub(super) fn check_owner_is_system(handle: HANDLE) -> Result<(), Untrusted> {
    let mut owner: PSID = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: a valid handle; out pointers valid; the descriptor is freed below.
    let rc = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if rc != 0 {
        return Err(Untrusted::Unreadable {
            what: "GetSecurityInfo",
            cause: io::Error::from_raw_os_error(rc as i32).to_string(),
        });
    }
    let sid = sid_string(owner);
    // SAFETY: the owner SID points into `descriptor`, allocated by `GetSecurityInfo`; `sid` is already copied out.
    unsafe {
        LocalFree(descriptor);
    }
    let sid = sid.ok_or_else(|| Untrusted::Unreadable {
        what: "ConvertSidToStringSid(owner)",
        cause: io::Error::last_os_error().to_string(),
    })?;
    crate::trust::check_pipe_owner(&sid)
}
