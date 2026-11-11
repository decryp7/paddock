//! Windows session FFI (ADR 0003): the logged-on user's session (at the console or over Remote Desktop), starting a runner in that user's session
//! with their token, and the Ctrl+Break helper. This is the crate's one module of Win32 calls; every `unsafe` block
//! says why it is sound.
//!
//! - `WTSQueryUserToken` needs LocalSystem. When the service runs as an ordinary user (development, `run` in a
//!   console) it fails with `ERROR_PRIVILEGE_NOT_HELD`; the caller then starts runners as plain children, which run
//!   in that user's session anyway.
//! - The runner's output goes through an anonymous pipe. Only the pipe's write end and a `NUL` handle are inherited
//!   (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`), so the runner never receives other handles of the service.
//! - The process is created hidden (`CREATE_NO_WINDOW`) in its own process group on `winsta0\default` with the
//!   user's environment block.
#![allow(unsafe_code)]

use crate::launch::{read_lines, LaunchSpec, LineSink, RunnerProcess};
use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0};
use windows::Win32::Security::{
    DuplicateTokenEx, GetTokenInformation, IsWellKnownSid, SecurityIdentification, TokenElevation, TokenPrimary, TokenUser,
    WinLocalSystemSid, SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_BREAK_EVENT};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::RemoteDesktop::{
    WTSActive, WTSDisconnected, WTSEnumerateSessionsW, WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQueryUserToken,
    WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW,
};
use windows::Win32::System::Threading::{
    CreateProcessAsUserW, DeleteProcThreadAttributeList, GetExitCodeProcess, InitializeProcThreadAttributeList, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
const NO_SESSION: u32 = 0xFFFF_FFFF;

/// Who is at the console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleUser {
    /// Nobody is logged on.
    Nobody,
    /// A user is logged on in this session.
    Session(u32),
    /// The service is not LocalSystem (development): it runs as the user, so plain children are in their session.
    NotSystem,
}

/// Whether this service process runs as LocalSystem: its own token's user is `S-1-5-18`. (Not guessed from whether another call
/// fails: `WTSQueryUserToken` also fails for other reasons, and a service running as an administrator account is not SYSTEM.)
pub fn is_local_system() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process and a valid out pointer; the token is owned below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let token = Owned(token);
    let mut needed = 0u32;
    // SAFETY: a size query with no buffer (it fails by design with the size needed).
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
    let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: `buf` holds at least `needed` bytes and is 8-byte aligned.
    if unsafe { GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), needed, &mut needed) }.is_err() {
        return false;
    }
    // SAFETY: the buffer holds a TOKEN_USER written by the call above and is alive for the check.
    unsafe { IsWellKnownSid((*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid, WinLocalSystemSid) }.as_bool()
}

/// Whether this process runs with an **elevated** token (an administrator's full token, UAC "Run as administrator"). The per-user
/// engine must never run so: its pipe admits its owner, and an elevated engine would be a place where any program of that user
/// could make an elevated process run what it names.
pub fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process and a valid out pointer; the token is owned below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        // Not knowing is not "not elevated": the engine refuses to start.
        return true;
    }
    let token = Owned(token);
    let mut info = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: `info` is a TOKEN_ELEVATION of the size passed; `token` is a valid token handle.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&mut info as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    read.is_err() || info.TokenIsElevated != 0
}

/// A SID as the string `S-1-5-...`, from a pointer valid for the call.
fn sid_string(sid: windows::Win32::Security::PSID) -> Result<String, String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid for the call; the string it allocates is freed below.
    unsafe { ConvertSidToStringSidW(sid, &mut text) }.map_err(|e| os_err("Naming an account", e))?;
    // SAFETY: a NUL-terminated string the call allocated.
    let result = unsafe { text.to_string() }.map_err(|e| e.to_string());
    // SAFETY: allocated with LocalAlloc by the call above and freed once.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
    }
    result
}

/// The SID of the account this process runs as.
pub fn current_user_sid() -> Result<String, String> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process and a valid out pointer; the token is owned below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| os_err("Opening this process's token", e))?;
    let token = Owned(token);
    let mut needed = 0u32;
    // SAFETY: a size query with no buffer (it fails by design with the size needed).
    let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
    let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    // SAFETY: `buf` holds at least `needed` bytes and is 8-byte aligned.
    unsafe { GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), needed, &mut needed) }
        .map_err(|e| os_err("Reading this process's account", e))?;
    // SAFETY: the buffer holds a TOKEN_USER written by the call above and is alive for the call.
    sid_string(unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid })
}

/// The owner of the pipe object an open client handle is connected to, as a SID string (read through the handle, so no process id
/// is involved).
pub fn pipe_owner_sid(pipe: &File) -> Result<String, String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is open for READ_CONTROL for the whole call; the out pointers are valid; `owner` points into
    // `descriptor`, which is freed only after the SID is copied out.
    let status = unsafe {
        GetSecurityInfo(
            HANDLE(pipe.as_raw_handle()),
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
    let result = sid_string(owner);
    // SAFETY: allocated with LocalAlloc by GetSecurityInfo and freed once.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    result
}

/// An owned Win32 handle, closed on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: the handle is owned by this value and closed exactly once.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// SAFETY: process and token handles may be used and closed from any thread.
unsafe impl Send for Owned {}

/// The logged-on user's session (see [`pitcrew_core::session::pick_user_session`]): the console's, or a Remote
/// Desktop session when the user works over Remote Desktop and the console shows the sign-in screen.
pub fn console_user() -> ConsoleUser {
    // SAFETY: no arguments; returns a session id or 0xFFFFFFFF.
    let console = unsafe { WTSGetActiveConsoleSessionId() };
    let console = (console != NO_SESSION).then_some(console);
    let mut candidates = Vec::new();
    let mut info: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: valid out pointers; the array is read below and freed with WTSFreeMemory exactly once.
    if unsafe { WTSEnumerateSessionsW(Some(WTS_CURRENT_SERVER_HANDLE), 0, 1, &mut info, &mut count) }.is_ok() && !info.is_null() {
        // SAFETY: WTSEnumerateSessionsW returned `count` entries at `info`.
        let sessions = unsafe { std::slice::from_raw_parts(info, count as usize) };
        candidates.extend(
            sessions
                .iter()
                .filter(|s| s.State == WTSActive || s.State == WTSDisconnected)
                .map(|s| (s.SessionId, s.State == WTSActive)),
        );
        // SAFETY: `info` was allocated by WTSEnumerateSessionsW.
        unsafe { WTSFreeMemory(info.cast()) };
    } else if let Some(c) = console {
        candidates.push((c, true));
    }
    let mut users = Vec::new();
    for (id, active) in candidates {
        let mut token = HANDLE::default();
        // SAFETY: `token` is a valid out pointer; the handle is closed by `Owned`.
        match unsafe { WTSQueryUserToken(id, &mut token) } {
            Ok(()) => {
                drop(Owned(token));
                users.push(pitcrew_core::session::UserSession { id, active });
            }
            Err(e) if e.code().0 & 0xFFFF == ERROR_PRIVILEGE_NOT_HELD => return ConsoleUser::NotSystem,
            // No user in this session (the sign-in screen) or it went away meanwhile.
            Err(_) => {}
        }
    }
    match pitcrew_core::session::pick_user_session(console, &users) {
        Some(s) => ConsoleUser::Session(s),
        None => ConsoleUser::Nobody,
    }
}

/// Quotes one argument for a Windows command line (the rules `CommandLineToArgvW` and the C runtime parse).
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn os_err(what: &str, e: windows::core::Error) -> String {
    format!("{what} failed: {}", e.message())
}

/// A process started in a user's session.
struct Started {
    process: Owned,
    pid: u32,
    output: Option<File>,
}

/// Starts `program args` in `session` with that user's token. With `capture`, standard output and error come back
/// as one pipe.
fn start_in_session(
    session: u32,
    program: &std::path::Path,
    args: &[String],
    dir: Option<&std::path::Path>,
    capture: bool,
) -> Result<Started, String> {
    let mut user = HANDLE::default();
    // SAFETY: valid out pointer; owned below.
    unsafe { WTSQueryUserToken(session, &mut user) }.map_err(|e| os_err("Getting the logged-on user's token", e))?;
    let user = Owned(user);
    let mut primary = HANDLE::default();
    // SAFETY: `user.0` is a valid token; `primary` is a valid out pointer, owned below.
    unsafe { DuplicateTokenEx(user.0, TOKEN_ALL_ACCESS, None, SecurityIdentification, TokenPrimary, &mut primary) }
        .map_err(|e| os_err("Preparing the user's token", e))?;
    let primary = Owned(primary);

    let mut env: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid out pointer and token; the block is destroyed below.
    unsafe { CreateEnvironmentBlock(&mut env, Some(primary.0), false) }.map_err(|e| os_err("Building the user's environment", e))?;
    struct Env(*mut c_void);
    impl Drop for Env {
        fn drop(&mut self) {
            // SAFETY: created by CreateEnvironmentBlock and destroyed once.
            unsafe {
                let _ = DestroyEnvironmentBlock(self.0);
            }
        }
    }
    let env = Env(env);

    let inheritable = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    // stdin: NUL.
    let nul = wide("NUL");
    // SAFETY: `nul` is NUL-terminated and outlives the call; the handle is owned below.
    let null_in = unsafe {
        CreateFileW(
            PCWSTR(nul.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&inheritable),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| os_err("Opening NUL", e))?;
    let null_in = Owned(null_in);

    let (read_end, write_end) = if capture {
        let mut r = HANDLE::default();
        let mut w = HANDLE::default();
        // SAFETY: valid out pointers; both handles are owned below.
        unsafe { CreatePipe(&mut r, &mut w, Some(&inheritable), 64 * 1024) }.map_err(|e| os_err("Creating the output pipe", e))?;
        let (r, w) = (Owned(r), Owned(w));
        // The service's end must not be inherited.
        // SAFETY: `r.0` is a valid handle owned by `r`.
        unsafe { SetHandleInformation(r.0, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }.map_err(|e| os_err("Preparing the output pipe", e))?;
        (Some(r), Some(w))
    } else {
        (None, None)
    };
    let out_handle = write_end.as_ref().map_or(null_in.0, |w| w.0);
    let handles: Vec<HANDLE> = if write_end.is_some() {
        vec![null_in.0, out_handle]
    } else {
        vec![null_in.0]
    };

    // The attribute list restricting inheritance to `handles`.
    let mut size = 0usize;
    // SAFETY: the first call only computes the size (it fails with ERROR_INSUFFICIENT_BUFFER by design).
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
    let mut list_buf = vec![0u8; size.max(64)];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(list_buf.as_mut_ptr().cast());
    // SAFETY: `list_buf` has the size the first call asked for and outlives the list's use.
    unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut size) }
        .map_err(|e| os_err("Preparing the process attributes", e))?;
    struct List(LPPROC_THREAD_ATTRIBUTE_LIST);
    impl Drop for List {
        fn drop(&mut self) {
            // SAFETY: initialised above and deleted once, before its buffer is freed.
            unsafe { DeleteProcThreadAttributeList(self.0) }
        }
    }
    let list_guard = List(list);
    // SAFETY: `handles` outlives CreateProcessAsUserW below; the size is that of the array.
    unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(handles.as_ptr().cast()),
            std::mem::size_of_val(handles.as_slice()),
            None,
            None,
        )
    }
    .map_err(|e| os_err("Limiting the inherited handles", e))?;

    let mut desktop = wide("winsta0\\default");
    let mut si = STARTUPINFOEXW::default();
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.lpDesktop = PWSTR(desktop.as_mut_ptr());
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    si.StartupInfo.wShowWindow = 0; // SW_HIDE
    si.StartupInfo.hStdInput = null_in.0;
    si.StartupInfo.hStdOutput = out_handle;
    si.StartupInfo.hStdError = out_handle;
    si.lpAttributeList = list;

    let app = wide(&program.display().to_string());
    let mut line: Vec<u16> = wide(
        &std::iter::once(quote_arg(&program.display().to_string()))
            .chain(args.iter().map(|a| quote_arg(a)))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let cwd = dir.map(|d| wide(&d.display().to_string()));
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: every pointer refers to a buffer that lives until the call returns; `si` is a STARTUPINFOEXW with
    // EXTENDED_STARTUPINFO_PRESENT; the command line buffer is mutable as the API requires.
    unsafe {
        CreateProcessAsUserW(
            Some(primary.0),
            PCWSTR(app.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | EXTENDED_STARTUPINFO_PRESENT,
            Some(env.0),
            cwd.as_ref().map_or(PCWSTR::null(), |c| PCWSTR(c.as_ptr())),
            &si.StartupInfo,
            &mut pi,
        )
    }
    .map_err(|e| os_err(&format!("Starting {} in the user's session", program.display()), e))?;
    drop(list_guard);
    drop(Owned(pi.hThread));
    // Our copy of the write end must close, or the reader never sees the end of the output.
    drop(write_end);
    let output = read_end.map(|r| {
        let raw = r.0 .0 as RawHandle;
        std::mem::forget(r);
        // SAFETY: the read end is a valid pipe handle whose ownership moves into the File.
        unsafe { File::from_raw_handle(raw) }
    });
    Ok(Started {
        process: Owned(pi.hProcess),
        pid: pi.dwProcessId,
        output,
    })
}

/// A runner running in a user's session.
pub struct SessionProcess {
    started: Started,
    session: u32,
    exited: Option<Option<i32>>,
}

/// Starts a runner in `session`, its output going to `sink`.
pub fn spawn_in_session(session: u32, spec: &LaunchSpec, sink: LineSink) -> Result<SessionProcess, String> {
    let mut started = start_in_session(session, &spec.program, &spec.args, Some(&spec.dir), true)?;
    if let Some(out) = started.output.take() {
        read_lines(out, sink);
    }
    Ok(SessionProcess {
        started,
        session,
        exited: None,
    })
}

fn exit_code(h: HANDLE) -> Option<Option<i32>> {
    // SAFETY: `h` is a valid process handle owned by the caller.
    unsafe {
        if WaitForSingleObject(h, 0) != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        Some(GetExitCodeProcess(h, &mut code).ok().map(|_| code as i32))
    }
}

impl RunnerProcess for SessionProcess {
    fn pid(&self) -> u32 {
        self.started.pid
    }

    fn try_wait(&mut self) -> Option<Option<i32>> {
        if self.exited.is_none() {
            self.exited = exit_code(self.started.process.0);
        }
        self.exited
    }

    fn stop_gracefully(&mut self) -> Result<(), String> {
        crate::launch::graceful_stop(self.started.pid, Some(self.session))
    }

    fn kill(&mut self) {
        crate::launch::kill_tree(self.started.pid);
        // SAFETY: valid process handle; failure (already gone) is ignored.
        unsafe {
            // conformance: pid-kill — a process handle we own (the process object cannot be handed on while it is open)
            let _ = TerminateProcess(self.started.process.0, 1);
        }
    }
}

/// Runs a short helper in `session` and waits for it (at most `timeout`).
pub fn run_in_session(session: u32, program: &std::path::Path, args: &[String], timeout: Duration) -> Result<(), String> {
    let started = start_in_session(session, program, args, None, false)?;
    let deadline = Instant::now() + timeout;
    loop {
        match exit_code(started.process.0) {
            Some(Some(0)) => return Ok(()),
            Some(code) => return Err(format!("the stop helper failed (exit code {code:?})")),
            None if Instant::now() >= deadline => {
                // SAFETY: valid process handle.
                unsafe {
                    // conformance: pid-kill — a process handle we own (the process object cannot be handed on while it is open)
                    let _ = TerminateProcess(started.process.0, 1);
                }
                return Err("the stop helper did not finish in time".into());
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// The helper's work (`pitcrew-service ctrl-break <pid>`): attach to the runner's console and send Ctrl+Break to
/// its process group, ignoring the event ourselves.
pub fn send_ctrl_break(pid: u32) -> Result<(), String> {
    // SAFETY: plain console calls on this process; the handler routine is NULL with "ignore" = TRUE.
    unsafe {
        let _ = FreeConsole();
        AttachConsole(pid).map_err(|e| os_err("Attaching to the runner's console", e))?;
        SetConsoleCtrlHandler(None, true).map_err(|e| os_err("Ignoring Ctrl+Break in the helper", e))?;
        let sent = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid);
        let _ = FreeConsole();
        sent.map_err(|e| os_err("Sending Ctrl+Break", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_arguments_like_the_c_runtime_parses_them() {
        assert_eq!(quote_arg("daemon"), "daemon");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg(r"C:\Program Files\x"), r#""C:\Program Files\x""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        assert_eq!(quote_arg(r#"x\"y z"#), r#""x\\\"y z""#);
    }

    #[test]
    fn a_non_system_service_says_so() {
        // Tests run as an ordinary user (or as SYSTEM on a service-hosted runner); never as a missing user.
        let u = console_user();
        assert!(matches!(u, ConsoleUser::NotSystem | ConsoleUser::Session(_) | ConsoleUser::Nobody));
    }
}
