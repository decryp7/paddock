//! Who owns the pipe the app is about to talk to (Windows). The app sends nothing on a pipe, not even the first `Hello`, before
//! it has checked that the pipe object belongs to the service: SYSTEM or Administrators. A program of some user that hosts a
//! pipe of the same name owns it as that user, and is refused before it can read a registration token or a settings change.
//!
//! The pipe's owner is read through the client's own handle (`GetSecurityInfo`, the handle was opened with `READ_CONTROL`), which
//! is bound to the object the connection is made to: it cannot name another process (no pid is read, so there is no reused id to
//! trust, rule 53). The service's ACL gives interactive users no right to create an instance of the pipe, and clients open at
//! identification level (`pitcrew_core::service_paths::open_pipe_options`); this is the third layer, and the one that keeps the
//! request from ever reaching a stranger.

/// What the OS says about the other end of the pipe. A port, so the check is tested without a pipe, and so a test can show that
/// nothing is written before it passes.
pub trait ServerIdentity {
    /// The owner of the pipe object, as a SID string (`S-1-5-...`).
    ///
    /// # Errors
    /// The OS could not tell; the connection is refused.
    fn owner_sid(&self) -> Result<String, String>;
}

/// Decides from the owner (pure; the rule is `pitcrew_core::service_paths::pipe_owner_ok`). `own_user` is the SID of this
/// process's account for the **per-user engine's** pipe, which that account's own process hosts, and `None` for the system
/// service's pipe, which only SYSTEM or Administrators may own.
///
/// # Errors
/// A message that says what is wrong and what to do; nothing has been sent.
pub fn judge_owner(owner: &str, own_user: Option<&str>) -> Result<(), String> {
    if pitcrew_core::service_paths::pipe_owner_ok(owner, own_user) {
        return Ok(());
    }
    Err(format!(
        "the Pitcrew pipe is owned by {owner}, not by SYSTEM or Administrators{}, so it is not the Pitcrew service; nothing was sent. \
         Check the service (Settings > Service) and do not approve a permission prompt you did not ask for",
        if own_user.is_some() { " or by this account" } else { "" }
    ))
}

/// Refuses a pipe that the service (or, with `own_user`, this account's own engine) does not own.
///
/// # Errors
/// The owner could not be read, or is not the service's; in both cases nothing was sent.
pub fn verify(pipe: &dyn ServerIdentity, own_user: Option<&str>) -> Result<(), String> {
    let owner = pipe
        .owner_sid()
        .map_err(|e| format!("the Pitcrew pipe could not be checked ({e}); nothing was sent"))?;
    judge_owner(&owner, own_user)
}

#[cfg(windows)]
pub use imp::{current_user_sid, PipeOwner};

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use super::ServerIdentity;
    use std::os::windows::io::AsRawHandle;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};

    /// The SID of the account this process runs as (the owner of the per-user engine's pipe).
    pub fn current_user_sid() -> Result<String, String> {
        use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        // SAFETY: the pseudo handle of this process and a valid out pointer; the token is closed below.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| format!("OpenProcessToken: {e}"))?;
        let mut needed = 0u32;
        // SAFETY: a size query with no buffer (it fails by design with the size needed).
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
        let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        // SAFETY: `buf` holds at least `needed` bytes and is 8-byte aligned.
        let read = unsafe { GetTokenInformation(token, TokenUser, Some(buf.as_mut_ptr().cast()), needed, &mut needed) };
        let result = read.map_err(|e| format!("GetTokenInformation: {e}")).and_then(|()| {
            // SAFETY: the buffer holds a TOKEN_USER written by the call above.
            sid_text(unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid })
        });
        // SAFETY: the handle opened above, closed once.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(token);
        }
        result
    }

    /// A SID as `S-1-5-...`.
    fn sid_text(sid: PSID) -> Result<String, String> {
        let mut text = PWSTR::null();
        // SAFETY: `sid` is valid for the call; the string is freed below.
        unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(|e| format!("ConvertSidToStringSid: {e}"))?;
        // SAFETY: a NUL-terminated string the call allocated.
        let result = unsafe { text.to_string() }.map_err(|e| e.to_string());
        // SAFETY: allocated with LocalAlloc by the call above, freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        }
        result
    }

    /// The owner of the pipe object an open client handle is connected to.
    pub struct PipeOwner<'a>(pub &'a std::fs::File);

    impl ServerIdentity for PipeOwner<'_> {
        fn owner_sid(&self) -> Result<String, String> {
            let mut owner = PSID::default();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: the handle is open for READ_CONTROL for the whole call; the out pointers are valid; `owner` points into
            // `descriptor`, which is freed only after the SID is copied out.
            let status = unsafe {
                GetSecurityInfo(
                    HANDLE(self.0.as_raw_handle()),
                    SE_KERNEL_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    Some(&mut owner),
                    None,
                    None,
                    None,
                    Some(&mut descriptor),
                )
            };
            if status.0 != 0 {
                return Err(format!("GetSecurityInfo: error {}", status.0));
            }
            // SAFETY: `owner` is a valid SID inside `descriptor`, freed only after the string is made.
            let result = sid_text(owner);
            // SAFETY: allocated with LocalAlloc by GetSecurityInfo and freed once.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            }
            result
        }
    }

    /// On the Windows test VM (`--include-ignored`): the real owner check over real pipes. A pipe hosted by the test's own (not
    /// SYSTEM) user is refused with nothing sent; the installed service (`PITCREW_TEST_INSTALLED=1`) is accepted.
    #[cfg(test)]
    mod vm {
        use super::*;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX};
        use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};

        #[test]
        #[ignore = "VM: PITCREW_TEST_INSTALLED=1 on the Windows test VM"]
        fn a_pipe_hosted_by_a_user_is_refused_and_the_installed_service_is_accepted() {
            let name: Vec<u16> = format!(r"\\.\pipe\pitcrew-squat-test-{}", std::process::id())
                .encode_utf16()
                .chain(Some(0))
                .collect();
            // SAFETY: a NUL-terminated name that outlives the call; the handle is closed at the end of the test.
            let server = unsafe {
                CreateNamedPipeW(
                    PCWSTR(name.as_ptr()),
                    FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_DUPLEX.0),
                    PIPE_TYPE_BYTE | PIPE_WAIT,
                    4,
                    4096,
                    4096,
                    0,
                    None,
                )
            };
            assert!(!server.is_invalid(), "the test could not host a pipe");
            let path = String::from_utf16_lossy(&name[..name.len() - 1]);
            let client = pitcrew_core::service_paths::open_pipe_options().open(&path).unwrap();
            let refused = super::super::verify(&PipeOwner(&client), None).unwrap_err();
            assert!(
                refused.contains("not by SYSTEM or Administrators") && refused.contains("nothing was sent"),
                "{refused}"
            );
            // SAFETY: the handle created above, closed once.
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(server);
            }
            if std::env::var("PITCREW_TEST_INSTALLED").as_deref() == Ok("1") {
                let installed = pitcrew_core::service_paths::open_pipe_options().open(r"\\.\pipe\pitcrew").unwrap();
                super::super::verify(&PipeOwner(&installed), None)
                    .expect("the installed service's pipe is owned by SYSTEM or Administrators");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Result<&'static str, &'static str>);

    impl ServerIdentity for Fake {
        fn owner_sid(&self) -> Result<String, String> {
            self.0.map(String::from).map_err(String::from)
        }
    }

    #[test]
    fn only_the_service_s_accounts_own_the_pipe() {
        assert!(judge_owner("S-1-5-18", None).is_ok());
        assert!(judge_owner("S-1-5-32-544", None).is_ok());
        let e = judge_owner("S-1-5-21-1-2-3-1001", None).unwrap_err();
        assert!(e.contains("S-1-5-21-1-2-3-1001") && e.contains("nothing was sent"), "{e}");
        assert!(judge_owner("", None).is_err());
        assert!(judge_owner("S-1-5-32-545", None).is_err(), "Users");
        // The per-user engine's pipe: this account's own SID is accepted there and nowhere else.
        assert!(judge_owner("S-1-5-21-1-2-3-1001", Some("S-1-5-21-1-2-3-1001")).is_ok());
        assert!(
            judge_owner("S-1-5-21-1-2-3-1002", Some("S-1-5-21-1-2-3-1001")).is_err(),
            "another user's pipe"
        );
    }

    #[test]
    fn a_pipe_whose_owner_cannot_be_read_is_refused() {
        let e = verify(&Fake(Err("access denied")), None).unwrap_err();
        assert!(e.contains("could not be checked") && e.contains("nothing was sent"), "{e}");
        assert!(verify(&Fake(Ok("S-1-5-18")), None).is_ok());
    }
}
