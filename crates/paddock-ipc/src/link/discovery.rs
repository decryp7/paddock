//! The discovery file: how an app finds its running companion (agent, service, worker). It holds the private endpoint,
//! the token, the companion's pid and build, so it is written owner-only (atomically, through `paddock-env`) and removed
//! when the companion stops. A file whose process is gone is stale and ignored; a file anyone else could have written is
//! not trusted.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::frame::ProtocolVersion;
use crate::token::Token;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Largest discovery file read.
pub const MAX_DISCOVERY: u64 = 16 * 1024;

/// What a client needs to connect, and what it compares to decide whether to replace the companion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    /// The endpoint name (socket path or pipe name).
    pub endpoint: String,
    /// The token, 64 hex digits.
    pub token: String,
    /// The companion's process id.
    pub pid: u32,
    /// The companion's build (its version, or version plus commit): a client of another build replaces it.
    pub build: String,
    /// The protocol the companion serves.
    pub protocol: ProtocolVersion,
    /// When it started, ms since the epoch (for messages).
    pub started_ms: i64,
}

/// What [`read`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// No file, or a damaged or oversized one.
    Nothing,
    /// A file whose process is not running any more.
    Stale(Discovery),
    /// A file that others could have written or read (not owner-only, or not ours); never used.
    Untrusted(String),
    /// A file whose process runs.
    Live(Discovery),
}

impl Discovery {
    /// The token, if the file holds a valid one.
    pub fn token(&self) -> Option<Token> {
        Token::from_hex(&self.token)
    }

    /// Writes the file atomically, readable by the owner only.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        paddock_env::write_atomic(path, &body, true).map_err(std::io::Error::other)
    }

    /// Removes the file if it still names process `pid` (a companion leaves another one's file alone).
    pub fn remove_if_owned_by(path: &Path, pid: u32) {
        if let Found::Live(d) | Found::Stale(d) = read(path, |_| true)
            && d.pid == pid
        {
            paddock_proc::cleanup::remove_file(path);
        }
    }
}

/// Whether the process `d` names still runs: its pid is held by a process that started before the file was written
/// (`paddock_proc::ProcessId::recorded_presence`). A pid handed on to a newer process makes the file stale (ADR 0033).
/// When the process table or the holder's start time cannot be read the answer is "running": a file is removed, and a
/// second companion started, only on a proven "gone" (rule 53).
pub fn is_running(d: &Discovery) -> bool {
    is_running_by(d, &look_now)
}

/// [`is_running`] over a look at the recorded pid, given the pid and the record's time (tests give one that cannot read
/// the process table).
fn is_running_by(d: &Discovery, look: &dyn Fn(u32, i64) -> paddock_proc::Presence) -> bool {
    running_from(look(d.pid, d.started_ms))
}

/// The real look at a recorded pid; under test a thread-local look stands in, so the real [`is_running`] can be driven
/// through an unreadable process table.
fn look_now(pid: u32, written_ms: i64) -> paddock_proc::Presence {
    #[cfg(test)]
    if let Some(p) = tests_look::OVERRIDE.with(std::cell::Cell::get) {
        return p;
    }
    paddock_proc::ProcessId::recorded_presence(pid, written_ms)
}

#[cfg(test)]
mod tests_look {
    use std::cell::Cell;
    thread_local! {
        pub(super) static OVERRIDE: Cell<Option<paddock_proc::Presence>> = const { Cell::new(None) };
    }
}

/// The decision of [`is_running`] over a look at the process: only `Gone` is stale.
fn running_from(p: paddock_proc::Presence) -> bool {
    p.may_be_alive()
}

/// Reads the discovery file at `path`, with `is_alive` deciding whether its process runs ([`is_running`] in apps; tests
/// inject one).
pub fn read(path: &Path, is_alive: impl Fn(&Discovery) -> bool) -> Found {
    let Ok(meta) = std::fs::metadata(path) else {
        return Found::Nothing;
    };
    if let Err(why) = private(path, &meta) {
        return Found::Untrusted(why);
    }
    if meta.len() > MAX_DISCOVERY {
        return Found::Nothing;
    }
    let Ok(d) = std::fs::read(path)
        .map_err(|_| ())
        .and_then(|b| serde_json::from_slice::<Discovery>(&b).map_err(|_| ()))
    else {
        return Found::Nothing;
    };
    if is_alive(&d) {
        Found::Live(d)
    } else {
        Found::Stale(d)
    }
}

#[cfg(unix)]
fn private(_path: &Path, meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and cannot fail.
    #[allow(unsafe_code)]
    let me = unsafe { libc::geteuid() };
    if meta.uid() != me {
        return Err(format!("the discovery file belongs to user {}", meta.uid()));
    }
    if meta.mode() & 0o077 != 0 {
        return Err(format!(
            "the discovery file is readable or writable by others (mode {:o})",
            meta.mode() & 0o777
        ));
    }
    Ok(())
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn private(path: &Path, _meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        WinBuiltinAdministratorsSid,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut owner: PSID = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: a NUL-terminated path; the out pointers are valid; the descriptor is freed below and `owner` points into it.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(format!(
            "the discovery file's owner cannot be read (error {rc})"
        ));
    }
    let mut token: HANDLE = null_mut();
    let mut buf = vec![0u8; 256];
    let mut len = 0u32;
    // SAFETY: the current process pseudo-handle; `token` is closed below; `buf` is valid for `buf.len()` bytes.
    let mine = unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0
            && GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut len,
            ) != 0
    };
    // SAFETY: `buf` holds a TOKEN_USER when `mine`; both SIDs stay valid until the frees below.
    let ok = mine
        && unsafe {
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            EqualSid(owner, user.User.Sid) != 0
                || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0
        };
    // SAFETY: handles and memory obtained above, released once.
    unsafe {
        if !token.is_null() {
            CloseHandle(token);
        }
        LocalFree(descriptor);
    }
    if ok {
        Ok(())
    } else {
        Err("the discovery file belongs to another user".into())
    }
}

#[cfg(not(any(unix, windows)))]
fn private(_path: &Path, _meta: &std::fs::Metadata) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod running_tests {
    use super::*;
    use paddock_proc::Presence::{Gone, Running, Unknown};

    fn sample(pid: u32, started_ms: i64) -> Discovery {
        Discovery {
            endpoint: "e".into(),
            token: "t".into(),
            pid,
            build: "b".into(),
            protocol: crate::frame::ProtocolVersion::UNVERSIONED,
            started_ms,
        }
    }

    #[test]
    fn the_real_is_running_counts_an_unreadable_process_table_as_running() {
        let d = sample(4242, 9);
        for (look, expected) in [(Unknown, true), (Running, true), (Gone, false)] {
            tests_look::OVERRIDE.with(|o| o.set(Some(look)));
            assert_eq!(is_running(&d), expected, "{look:?}");
        }
        tests_look::OVERRIDE.with(|o| o.set(None));
    }

    #[test]
    fn is_running_by_asks_the_three_valued_look_with_the_recorded_pid_and_time() {
        let d = sample(4242, 9);
        assert!(is_running_by(&d, &|pid, ms| {
            assert_eq!((pid, ms), (4242, 9));
            Unknown
        }));
        assert!(is_running_by(&d, &|_, _| Running));
        assert!(!is_running_by(&d, &|_, _| Gone));
    }

    #[test]
    fn a_file_is_stale_only_when_the_process_is_proven_gone() {
        assert!(running_from(Running));
        assert!(
            running_from(Unknown),
            "an unreadable process table never makes a file stale (rule 53)"
        );
        assert!(!running_from(Gone));
    }
}
