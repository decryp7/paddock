//! Who serves the pipe the app is about to talk to (Windows). The elevated forwarder carries a registration token and an
//! administrator's approval, so before it sends anything it checks that the program on the other end of the pipe is the service
//! running as SYSTEM, not a program of some user that happens to host a pipe of the same name.
//!
//! The service's pipe ACL no longer lets an interactive user create an instance of it and the client opens at identification
//! level (`pitcrew_core::service_paths::open_pipe_options`), so a squatter could neither be created nor learn more than an
//! account; this is the third layer, and the one that keeps the request itself from reaching a stranger.
//!
//! Rule 53: a process id names a process only while that process lives. The check holds an open handle to the process it looked
//! at for the whole check (so the id cannot be reused meanwhile), reads the server's id from the pipe before and after, and
//! refuses when the two differ.

/// What the check found; judged by [`judge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// The server's process id when the check began.
    pub first_pid: u32,
    /// The server's process id when the check ended, read again from the same pipe.
    pub second_pid: u32,
    /// Whether the held process's token belongs to SYSTEM (`S-1-5-18`).
    pub is_system: bool,
}

/// Decides from the facts. Pure.
///
/// # Errors
/// A message that says what is wrong and what to do.
pub fn judge(f: Facts) -> Result<(), String> {
    if f.first_pid != f.second_pid {
        return Err("the program serving the Pitcrew pipe changed while it was being checked; nothing was sent".into());
    }
    if !f.is_system {
        return Err(format!(
            "the program serving the Pitcrew pipe (process {}) is not the Pitcrew service running as SYSTEM; nothing was sent. \
             Check the service (Settings > Service) and do not approve a permission prompt you did not ask for",
            f.first_pid
        ));
    }
    Ok(())
}

#[cfg(windows)]
pub use imp::require_system_server;

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use super::{judge, Facts};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, IsWellKnownSid, TokenUser, WinLocalSystemSid, TOKEN_QUERY, TOKEN_USER};
    use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};

    /// A handle closed on drop.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: a handle this value owns, closed once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn server_pid(pipe: &std::fs::File) -> Result<u32, String> {
        let mut pid = 0u32;
        // SAFETY: a valid pipe handle for the call and a valid out pointer.
        unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &mut pid) }
            .map_err(|e| format!("the pipe's server could not be identified (GetNamedPipeServerProcessId: {e}); nothing was sent"))?;
        Ok(pid)
    }

    fn runs_as_system(pid: u32) -> Result<bool, String> {
        let read = |what: &str, e: windows::core::Error| format!("the pipe's server could not be examined ({what}: {e}); nothing was sent");
        // SAFETY: opens a handle to the process with the least right that lets its token be read; owned below.
        let process = Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|e| read("OpenProcess", e))?);
        let mut token = HANDLE::default();
        // SAFETY: a valid process handle and out pointer; the token is owned below.
        unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }.map_err(|e| read("OpenProcessToken", e))?;
        let token = Owned(token);
        let mut needed = 0u32;
        // SAFETY: a size query with no buffer (it fails by design with the size needed).
        let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
        let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
        // SAFETY: `buf` holds at least `needed` bytes and is 8-byte aligned.
        unsafe { GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), needed, &mut needed) }
            .map_err(|e| read("GetTokenInformation", e))?;
        // SAFETY: the buffer holds a TOKEN_USER written by the call above, and `process` still pins the process.
        let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        // SAFETY: `sid` points into `buf`, alive here.
        Ok(unsafe { IsWellKnownSid(sid, WinLocalSystemSid) }.as_bool())
    }

    /// Refuses a pipe whose server is not the SYSTEM service.
    ///
    /// # Errors
    /// What could not be read, or that the server is not SYSTEM; in both cases nothing was sent.
    pub fn require_system_server(pipe: &std::fs::File) -> Result<(), String> {
        let first_pid = server_pid(pipe)?;
        let is_system = runs_as_system(first_pid)?;
        let second_pid = server_pid(pipe)?;
        judge(Facts {
            first_pid,
            second_pid,
            is_system,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_system_server_that_stayed_the_same_process_is_accepted() {
        assert!(judge(Facts {
            first_pid: 4,
            second_pid: 4,
            is_system: true
        })
        .is_ok());
        let e = judge(Facts {
            first_pid: 4,
            second_pid: 4,
            is_system: false,
        })
        .unwrap_err();
        assert!(e.contains("not the Pitcrew service") && e.contains("nothing was sent"), "{e}");
        let e = judge(Facts {
            first_pid: 4,
            second_pid: 9,
            is_system: true,
        })
        .unwrap_err();
        assert!(e.contains("changed while"), "{e}");
    }

    /// On the Windows test VM (`--include-ignored`): the real check over a real pipe. A pipe hosted by the test's own (not SYSTEM)
    /// user is refused with nothing sent; the installed service (`PITCREW_TEST_INSTALLED=1`) is accepted.
    #[cfg(windows)]
    #[allow(unsafe_code)]
    #[test]
    #[ignore = "VM: PITCREW_TEST_INSTALLED=1 on the Windows test VM"]
    fn a_pipe_hosted_by_a_user_is_refused_and_the_installed_service_is_accepted() {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{FILE_FLAGS_AND_ATTRIBUTES, PIPE_ACCESS_DUPLEX};
        use windows::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};
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
        let client = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let refused = imp::require_system_server(&client).unwrap_err();
        assert!(
            refused.contains("not the Pitcrew service") && refused.contains("nothing was sent"),
            "{refused}"
        );
        // SAFETY: the handle created above, closed once.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(server);
        }
        if std::env::var("PITCREW_TEST_INSTALLED").as_deref() == Ok("1") {
            let installed = pitcrew_core::service_paths::open_pipe_options().open(r"\\.\pipe\pitcrew").unwrap();
            imp::require_system_server(&installed).expect("the installed service runs as SYSTEM");
        }
    }
}
