//! The Windows service (adapted from Pitcrew's `platform/windows.rs` and `win_session.rs`): the entry point the
//! Service Control Manager calls (`pitwall-server --service`), registering and removing the service, and starting the
//! server itself in the logged-on user's session.
//!
//! Why not serve from the service process: Pitwall's data (`%APPDATA%\Pitwall`), engine builds, GPU access and the
//! admin key belong to the *user*; a LocalSystem service has another profile and lives in session 0. So the service is a
//! thin supervisor: it picks the user's session (`WTSEnumerateSessionsW`: connected before disconnected, the console
//! before other sessions, then the lowest id; `spec::pick_user_session`), starts `pitwall-server` there with the user's
//! token and environment (`PITWALL_SERVICE=1`, so the child watches for updates and restarts itself when idle), and
//! starts it again when it exits or the session changes. Over Remote Desktop the console shows the sign-in screen, which
//! is why the console session is not simply assumed.
//!
//! `WTSQueryUserToken` needs LocalSystem; when the service runs as an ordinary user (development) it fails with
//! `ERROR_PRIVILEGE_NOT_HELD` and the child is a plain child process, which is in that user's session anyway.
//! Registration needs an elevated caller (the installer, or Windows' own prompt); Pitwall never handles passwords.
//! Every `unsafe` block says why it is sound.
#![allow(unsafe_code)]

use super::spec::{
    env_block_with, pick_user_session, UserSession, SCM_RESET_AFTER, SCM_RESTART_DELAYS, SUPERVISED_ENV, WINDOWS_DESCRIPTION,
    WINDOWS_DISPLAY_NAME, WINDOWS_SERVICE_NAME,
};
use super::Verb;
use std::ffi::{c_void, OsString};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::{DuplicateTokenEx, SecurityIdentification, TokenPrimary, SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
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
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceFailureActions, ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_EXISTS: i32 = 1073;
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
const NO_SESSION: u32 = 0xFFFF_FFFF;
/// Pause between a child's exit and its restart (a crash loop must not spin).
const RESTART_PAUSE: Duration = Duration::from_secs(10);
/// How often the supervisor looks at its child and the session.
const POLL: Duration = Duration::from_millis(500);
const MAX_LOG_BYTES: u64 = 1024 * 1024;

pub(super) fn run_verb(verb: Verb) -> i32 {
    let result = match verb {
        Verb::Install => install().map(|()| "Pitwall's service is installed and started".to_string()),
        Verb::Uninstall => uninstall().map(|()| "Pitwall's service is removed".to_string()),
        Verb::Status => status_text(),
        Verb::PrintUnit => Err("print-unit is for Linux (systemd)".to_string()),
        Verb::WindowsService => dispatch().map(|()| String::new()),
    };
    match result {
        Ok(m) => {
            if !m.is_empty() {
                println!("{m}");
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn os_error(what: &str, e: &windows_service::Error) -> String {
    if raw_code(e) == Some(ERROR_ACCESS_DENIED) {
        return format!("{what} needs administrator rights: run it from an elevated prompt (the Pitwall installer does this for you)");
    }
    format!("{what} failed: {e}")
}

fn raw_code(e: &windows_service::Error) -> Option<i32> {
    match e {
        windows_service::Error::Winapi(io) => io.raw_os_error(),
        _ => None,
    }
}

define_windows_service!(ffi_service_main, service_main);

fn dispatch() -> Result<(), String> {
    service_dispatcher::start(WINDOWS_SERVICE_NAME, ffi_service_main).map_err(|e| os_error("Starting as a Windows service", &e))
}

fn service_main(_arguments: Vec<OsString>) {
    // Nobody sees a service's standard error; problems go to the service log.
    if let Err(e) = run_service() {
        log(&format!("service stopped with an error: {e}"));
    }
}

fn status(state: ServiceState, code: u32) -> ServiceStatus {
    let controls = if state == ServiceState::Running {
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN | ServiceControlAccept::SESSION_CHANGE
    } else {
        ServiceControlAccept::empty()
    };
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: controls,
        exit_code: ServiceExitCode::Win32(code),
        checkpoint: 0,
        wait_hint: Duration::from_secs(if state == ServiceState::StopPending { 30 } else { 0 }),
        process_id: None,
    }
}

enum Event {
    Stop,
    Session,
}

fn run_service() -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<Event>();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = tx.send(Event::Stop);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::SessionChange(_) => {
            let _ = tx.send(Event::Session);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let handle =
        service_control_handler::register(WINDOWS_SERVICE_NAME, handler).map_err(|e| os_error("Registering the control handler", &e))?;
    handle
        .set_service_status(status(ServiceState::Running, 0))
        .map_err(|e| os_error("Reporting the service status", &e))?;
    log("service started");
    supervise(&rx);
    log("service stopping");
    let _ = handle.set_service_status(status(ServiceState::StopPending, 0));
    let _ = handle.set_service_status(status(ServiceState::Stopped, 0));
    Ok(())
}

/// A running server child.
enum Child {
    Session(Process, u32),
    Plain(std::process::Child),
}

impl Child {
    fn exited(&mut self) -> bool {
        match self {
            Child::Session(p, _) => exit_code(p.0).is_some(),
            Child::Plain(c) => !matches!(c.try_wait(), Ok(None)),
        }
    }
    fn session(&self) -> Option<u32> {
        match self {
            Child::Session(_, s) => Some(*s),
            Child::Plain(_) => None,
        }
    }
    fn kill(&mut self) {
        match self {
            // SAFETY: a valid process handle; failure (already gone) is ignored.
            Child::Session(p, _) => unsafe {
                // conformance: pid-kill — a process handle we own (the process object cannot be handed on while it is open)
                let _ = TerminateProcess(p.0, 1);
            },
            Child::Plain(c) => {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

enum Target {
    Nobody,
    NotSystem,
    Session(u32),
}

fn supervise(rx: &mpsc::Receiver<Event>) {
    let exe = std::env::current_exe().unwrap_or_default();
    let mut child: Option<Child> = None;
    let mut next_start = Instant::now();
    loop {
        match rx.recv_timeout(POLL) {
            Ok(Event::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Event::Session) | Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let target = user_target();
        if let Some(c) = child.as_mut() {
            let moved =
                matches!((&target, c.session()), (Target::Session(now), Some(was)) if *now != was) || matches!(target, Target::Nobody);
            if c.exited() {
                log("the server exited; starting it again shortly");
                child = None;
                next_start = Instant::now() + RESTART_PAUSE;
            } else if moved {
                log("the user's session changed; moving the server");
                c.kill();
                child = None;
                next_start = Instant::now();
            }
        }
        if child.is_none() && Instant::now() >= next_start {
            let started = match target {
                Target::Nobody => None,
                Target::NotSystem => start_plain(&exe).map(Child::Plain).map_err(|e| log(&e)).ok(),
                Target::Session(id) => start_in_session(id, &exe).map(|p| Child::Session(p, id)).map_err(|e| log(&e)).ok(),
            };
            if started.is_some() {
                log("the server was started");
            } else {
                next_start = Instant::now() + RESTART_PAUSE;
            }
            child = started;
        }
    }
    if let Some(mut c) = child {
        c.kill();
    }
}

#[allow(clippy::disallowed_methods)] // development only: the service runs as the user, the child is our own program
fn start_plain(exe: &Path) -> Result<std::process::Child, String> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(exe)
        .env(SUPERVISED_ENV, "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("the server could not be started: {e}"))
}

/// An owned Win32 handle, closed on drop.
struct Process(HANDLE);

impl Drop for Process {
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
unsafe impl Send for Process {}

fn exit_code(h: HANDLE) -> Option<Option<i32>> {
    // SAFETY: `h` is a valid process handle owned by the caller.
    unsafe {
        if WaitForSingleObject(h, 0) != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        Some(GetExitCodeProcess(h, &mut code).ok().map(|()| code as i32))
    }
}

/// Where the server should run now.
fn user_target() -> Target {
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
        // SAFETY: `token` is a valid out pointer; the handle is closed by `Process`'s drop.
        match unsafe { WTSQueryUserToken(id, &mut token) } {
            Ok(()) => {
                drop(Process(token));
                users.push(UserSession { id, active });
            }
            Err(e) if e.code().0 & 0xFFFF == ERROR_PRIVILEGE_NOT_HELD => return Target::NotSystem,
            // No user in this session (the sign-in screen) or it went away meanwhile.
            Err(_) => {}
        }
    }
    pick_user_session(console, &users).map_or(Target::Nobody, Target::Session)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win_err(what: &str, e: windows::core::Error) -> String {
    format!("{what} failed: {}", e.message())
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

/// Starts `exe` hidden in `session` with that user's token and environment plus `PITWALL_SERVICE=1`; its standard
/// handles are `NUL` and it inherits no other handle of the service.
fn start_in_session(session: u32, exe: &Path) -> Result<Process, String> {
    let mut user = HANDLE::default();
    // SAFETY: valid out pointer; owned below.
    unsafe { WTSQueryUserToken(session, &mut user) }.map_err(|e| win_err("Getting the logged-on user's token", e))?;
    let user = Process(user);
    let mut primary = HANDLE::default();
    // SAFETY: `user.0` is a valid token; `primary` is a valid out pointer, owned below.
    unsafe { DuplicateTokenEx(user.0, TOKEN_ALL_ACCESS, None, SecurityIdentification, TokenPrimary, &mut primary) }
        .map_err(|e| win_err("Preparing the user's token", e))?;
    let primary = Process(primary);

    let mut env: *mut c_void = std::ptr::null_mut();
    // SAFETY: valid out pointer and token; the block is destroyed by the guard below.
    unsafe { CreateEnvironmentBlock(&mut env, Some(primary.0), false) }.map_err(|e| win_err("Building the user's environment", e))?;
    struct EnvGuard(*mut c_void);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: created by CreateEnvironmentBlock and destroyed once.
            unsafe {
                let _ = DestroyEnvironmentBlock(self.0);
            }
        }
    }
    let _env_guard = EnvGuard(env);
    // SAFETY: the block is a list of NUL-terminated UTF-16 strings ended by an empty one, as documented; it is read
    // up to that end only, before it is destroyed.
    let block = unsafe {
        let p = env.cast::<u16>();
        let mut n = 0usize;
        while !(*p.add(n) == 0 && *p.add(n + 1) == 0) {
            n += 1;
        }
        std::slice::from_raw_parts(p, n + 1).to_vec()
    };
    let mut block = env_block_with(&block, &format!("{SUPERVISED_ENV}=1"));

    let inheritable = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let nul = wide("NUL");
    // SAFETY: `nul` is NUL-terminated and outlives the call; the handle is owned below.
    let null_h = unsafe {
        CreateFileW(
            PCWSTR(nul.as_ptr()),
            FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&inheritable),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| win_err("Opening NUL", e))?;
    let null_h = Process(null_h);
    let handles = [null_h.0];

    let mut size = 0usize;
    // SAFETY: the first call only computes the size (it fails with ERROR_INSUFFICIENT_BUFFER by design).
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
    let mut list_buf = vec![0u8; size.max(64)];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(list_buf.as_mut_ptr().cast());
    // SAFETY: `list_buf` has the size the first call asked for and outlives the list's use.
    unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &mut size) }
        .map_err(|e| win_err("Preparing the process attributes", e))?;
    struct ListGuard(LPPROC_THREAD_ATTRIBUTE_LIST);
    impl Drop for ListGuard {
        fn drop(&mut self) {
            // SAFETY: initialised above and deleted once, before its buffer is freed.
            unsafe { DeleteProcThreadAttributeList(self.0) }
        }
    }
    let _list_guard = ListGuard(list);
    // SAFETY: `handles` outlives CreateProcessAsUserW below; the size is that of the array.
    unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            Some(handles.as_ptr().cast()),
            std::mem::size_of_val(&handles),
            None,
            None,
        )
    }
    .map_err(|e| win_err("Limiting the inherited handles", e))?;

    let mut desktop = wide("winsta0\\default");
    let mut si = STARTUPINFOEXW::default();
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.lpDesktop = PWSTR(desktop.as_mut_ptr());
    si.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
    si.StartupInfo.wShowWindow = 0; // SW_HIDE
    si.StartupInfo.hStdInput = null_h.0;
    si.StartupInfo.hStdOutput = null_h.0;
    si.StartupInfo.hStdError = null_h.0;
    si.lpAttributeList = list;

    let program = exe.display().to_string();
    let app = wide(&program);
    let mut line = wide(&quote_arg(&program));
    let mut pi = PROCESS_INFORMATION::default();
    // SAFETY: every pointer refers to a buffer that lives until the call returns; `si` is a STARTUPINFOEXW with
    // EXTENDED_STARTUPINFO_PRESENT; the command line buffer is mutable as the API requires; the environment block is
    // UTF-16 (CREATE_UNICODE_ENVIRONMENT).
    unsafe {
        CreateProcessAsUserW(
            Some(primary.0),
            PCWSTR(app.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            true,
            CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | EXTENDED_STARTUPINFO_PRESENT,
            Some(block.as_mut_ptr().cast()),
            PCWSTR::null(),
            &si.StartupInfo,
            &mut pi,
        )
    }
    .map_err(|e| win_err(&format!("Starting {program} in the user's session"), e))?;
    drop(Process(pi.hThread));
    Ok(Process(pi.hProcess))
}

/// `%ProgramData%\Pitwall\logs\service.log`, kept under 1 MiB (the log holds no request content).
fn log_path() -> Option<PathBuf> {
    std::env::var_os("ProgramData").map(|p| PathBuf::from(p).join("Pitwall").join("logs").join("service.log"))
}

fn log(line: &str) {
    use std::io::Write;
    let Some(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = writeln!(f, "{secs} {line}");
    }
}

/// Registers the service (automatic delayed start, LocalSystem, restart on failure) and starts it.
fn install() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)
        .map_err(|e| os_error("Installing the service", &e))?;
    let exe = std::env::current_exe().map_err(|e| format!("the program's own path cannot be found: {e}"))?;
    let info = ServiceInfo {
        name: OsString::from(WINDOWS_SERVICE_NAME),
        display_name: OsString::from(WINDOWS_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![OsString::from("--service")],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let service = match manager.create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START) {
        Ok(s) => s,
        Err(e) if raw_code(&e) == Some(ERROR_SERVICE_EXISTS) => {
            return Err("Pitwall's service is installed already; uninstall it first".into())
        }
        Err(e) => return Err(os_error("Installing the service", &e)),
    };
    service
        .set_description(WINDOWS_DESCRIPTION)
        .map_err(|e| os_error("Describing the service", &e))?;
    service
        .set_delayed_auto_start(true)
        .map_err(|e| os_error("Setting delayed start", &e))?;
    let actions = SCM_RESTART_DELAYS
        .iter()
        .map(|d| ServiceAction {
            action_type: ServiceActionType::Restart,
            delay: *d,
        })
        .collect();
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(SCM_RESET_AFTER),
            reboot_msg: None,
            command: None,
            actions: Some(actions),
        })
        .map_err(|e| os_error("Setting the recovery actions", &e))?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(|e| os_error("Setting the recovery actions", &e))?;
    service
        .start::<&std::ffi::OsStr>(&[])
        .map_err(|e| os_error("Starting the service", &e))
}

/// Stops and removes the service. Removing a service that is not installed succeeds.
fn uninstall() -> Result<(), String> {
    let manager =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).map_err(|e| os_error("Removing the service", &e))?;
    let service = match manager.open_service(
        WINDOWS_SERVICE_NAME,
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    ) {
        Ok(s) => s,
        Err(e) if raw_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => return Ok(()),
        Err(e) => return Err(os_error("Removing the service", &e)),
    };
    let state = service
        .query_status()
        .map_err(|e| os_error("Removing the service", &e))?
        .current_state;
    if state != ServiceState::Stopped {
        service.stop().map_err(|e| os_error("Stopping the service", &e))?;
        // Wait (a minute at most) so an installer can replace the binary afterwards.
        for _ in 0..120 {
            match service.query_status() {
                Ok(s) if s.current_state == ServiceState::Stopped => break,
                Ok(_) => std::thread::sleep(Duration::from_millis(500)),
                Err(_) => break,
            }
        }
    }
    service.delete().map_err(|e| os_error("Removing the service", &e))
}

fn status_text() -> Result<String, String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| os_error("Asking the service manager", &e))?;
    match manager.open_service(WINDOWS_SERVICE_NAME, ServiceAccess::QUERY_STATUS) {
        Ok(s) => {
            let st = s.query_status().map_err(|e| os_error("Asking the service manager", &e))?;
            Ok(format!("installed; state: {:?}", st.current_state))
        }
        Err(e) if raw_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => Ok("not installed".into()),
        Err(e) => Err(os_error("Asking the service manager", &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_denied_asks_for_elevation() {
        let denied = windows_service::Error::Winapi(std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED));
        assert!(os_error("Installing the service", &denied).contains("administrator"));
        let exists = windows_service::Error::Winapi(std::io::Error::from_raw_os_error(ERROR_SERVICE_EXISTS));
        assert_eq!(raw_code(&exists), Some(ERROR_SERVICE_EXISTS));
    }

    #[test]
    fn status_reports_accept_stop_only_while_running() {
        assert!(status(ServiceState::Running, 0)
            .controls_accepted
            .contains(ServiceControlAccept::STOP));
        assert!(status(ServiceState::Stopped, 0).controls_accepted.is_empty());
        assert!(status(ServiceState::StopPending, 0).wait_hint > Duration::ZERO);
    }

    #[test]
    fn quotes_arguments_like_the_c_runtime_parses_them() {
        assert_eq!(quote_arg("daemon"), "daemon");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg(r"C:\Program Files\x"), r#""C:\Program Files\x""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }
}
