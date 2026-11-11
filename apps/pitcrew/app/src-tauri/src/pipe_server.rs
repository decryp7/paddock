//! Who owns the pipe the app is about to talk to (Windows). The app sends nothing on a pipe, not even the first `Hello`, before
//! it has checked that the pipe object belongs to the service: SYSTEM or Administrators. A program of some user that hosts a
//! pipe of the same name owns it as that user, and is refused before it can read a registration token or a settings change.
//!
//! The pipe's owner is read through the client's own handle (`GetSecurityInfo`, the handle was opened with `READ_CONTROL`), which
//! is bound to the object the connection is made to: it cannot name another process (no pid is read, so there is no reused id to
//! trust, rule 53). The service's ACL gives interactive users no right to create an instance of the pipe, and clients open at
//! identification level (`pitcrew_core::service_paths::open_pipe_options`); this is the third layer, and the one that keeps the
//! request from ever reaching a stranger.

/// SYSTEM (`S-1-5-18`): the account the service runs as.
const SYSTEM: &str = "S-1-5-18";
/// BUILTIN\Administrators (`S-1-5-32-544`): the owner Windows gives what a SYSTEM process creates.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// What the OS says about the other end of the pipe. A port, so the check is tested without a pipe, and so a test can show that
/// nothing is written before it passes.
pub trait ServerIdentity {
    /// The owner of the pipe object, as a SID string (`S-1-5-...`).
    ///
    /// # Errors
    /// The OS could not tell; the connection is refused.
    fn owner_sid(&self) -> Result<String, String>;
}

/// Decides from the owner. Pure.
///
/// # Errors
/// A message that says what is wrong and what to do; nothing has been sent.
pub fn judge_owner(owner: &str) -> Result<(), String> {
    if owner == SYSTEM || owner == ADMINISTRATORS {
        return Ok(());
    }
    Err(format!(
        "the Pitcrew pipe is owned by {owner}, not by SYSTEM or Administrators, so it is not the Pitcrew service; nothing was sent. \
         Check the service (Settings > Service) and do not approve a permission prompt you did not ask for"
    ))
}

/// Refuses a pipe that the service does not own.
///
/// # Errors
/// The owner could not be read, or is not the service's; in both cases nothing was sent.
pub fn verify(pipe: &dyn ServerIdentity) -> Result<(), String> {
    let owner = pipe
        .owner_sid()
        .map_err(|e| format!("the Pitcrew pipe could not be checked ({e}); nothing was sent"))?;
    judge_owner(&owner)
}

#[cfg(windows)]
pub use imp::PipeOwner;

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use super::ServerIdentity;
    use std::os::windows::io::AsRawHandle;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};

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
            let mut text = PWSTR::null();
            // SAFETY: `owner` is a valid SID inside `descriptor`; the string is allocated by the call and freed below.
            let converted = unsafe { ConvertSidToStringSidW(owner, &mut text) };
            let result = match converted {
                // SAFETY: a NUL-terminated string the call allocated.
                Ok(()) => unsafe { text.to_string() }.map_err(|e| e.to_string()),
                Err(e) => Err(format!("ConvertSidToStringSid: {e}")),
            };
            // SAFETY: both blocks were allocated with LocalAlloc by the calls above and are freed once.
            unsafe {
                if !text.is_null() {
                    let _ = LocalFree(Some(HLOCAL(text.0.cast())));
                }
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
            let refused = super::super::verify(&PipeOwner(&client)).unwrap_err();
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
                super::super::verify(&PipeOwner(&installed)).expect("the installed service's pipe is owned by SYSTEM or Administrators");
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
        assert!(judge_owner("S-1-5-18").is_ok());
        assert!(judge_owner("S-1-5-32-544").is_ok());
        let e = judge_owner("S-1-5-21-1-2-3-1001").unwrap_err();
        assert!(e.contains("S-1-5-21-1-2-3-1001") && e.contains("nothing was sent"), "{e}");
        assert!(judge_owner("").is_err());
        assert!(judge_owner("S-1-5-32-545").is_err(), "Users");
    }

    #[test]
    fn a_pipe_whose_owner_cannot_be_read_is_refused() {
        let e = verify(&Fake(Err("access denied"))).unwrap_err();
        assert!(e.contains("could not be checked") && e.contains("nothing was sent"), "{e}");
        assert!(verify(&Fake(Ok("S-1-5-18"))).is_ok());
    }
}
