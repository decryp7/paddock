//! Native glue (crates/README.md): code only a real desktop runs, which tests cannot reach by rule 7 (they never touch
//! the real Login Items or elevation prompt). Each function makes one platform call or builds the real adapters and
//! hands over to tested code; it is reviewed by hand and left out of the coverage job.
//!
//! - `SmAppService`: `SMAppService` (macOS 13+) for one plist in the app bundle (macOS builds only).
//! - `RunAsElevator`: Windows' own elevation prompt (`ShellExecuteExW` with `runas`; Windows builds only).
//! - [`current_uid`] and [`host_backend`]: the user, and the backend for this OS from [`crate::backend::choose`].
#![allow(unsafe_code)]

use crate::backend::{Backend, Choice, Side, Target, choose};
use crate::error::ServiceError;
use crate::spec::ServiceSpec;
use crate::system::RealSystem;
use std::path::Path;

/// The user id (`gui/<uid>` is their launchd domain); 0 on Windows.
pub fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// The backend for `spec` on this computer: `program` is the background program (next to the app, or this process
/// on the program's side), `data_dir` the app's data folder (an AppImage's program is copied there first).
pub fn host_backend(
    spec: &ServiceSpec,
    program: &Path,
    side: Side,
    data_dir: &Path,
) -> Result<Box<dyn Backend>, ServiceError> {
    let vars = paddock_env::ProcessVars;
    let appimage = std::env::var("APPIMAGE").ok();
    let sys = || Box::new(RealSystem::new());
    match choose(
        paddock_env::Os::current(),
        spec,
        program,
        side,
        appimage.as_deref(),
        &vars,
    )? {
        #[cfg(target_os = "macos")]
        Choice::Bundled => Ok(Box::new(crate::backend::LaunchdBundled::new(
            spec.clone(),
            current_uid(),
            Box::new(mac::SmAppService::new(spec)),
            sys(),
        ))),
        Choice::LaunchdFile { plist, target } => Ok(Box::new(crate::backend::LaunchdFile::new(
            spec.clone(),
            current_uid(),
            plist,
            target,
            sys(),
        ))),
        Choice::Systemd {
            unit,
            target,
            copy_from,
        } => {
            let target = match copy_from {
                Some(src) => Target {
                    exe: crate::system::stable_copy(&src, data_dir).map_err(|source| {
                        ServiceError::Io {
                            op: "copy the program to",
                            path: data_dir.join("bin"),
                            source,
                        }
                    })?,
                    ..target
                },
                None => target,
            };
            Ok(Box::new(crate::backend::Systemd::new(
                spec.clone(),
                unit,
                target,
                sys(),
            )))
        }
        Choice::WindowsRun { target } => Ok(Box::new(crate::backend::WindowsRun::new(
            spec.clone(),
            target,
            sys(),
        ))),
        Choice::WindowsService {
            elevated: false,
            target,
        } => Ok(Box::new(crate::backend::WindowsService::new(
            spec.clone(),
            target,
            sys(),
        ))),
        #[cfg(windows)]
        Choice::WindowsService {
            elevated: true,
            target,
        } => Ok(Box::new(crate::backend::ElevatedWindowsService::new(
            spec.clone(),
            target.exe,
            sys(),
            Box::new(win::RunAsElevator),
        ))),
        #[allow(unreachable_patterns)]
        _ => Err(ServiceError::Unavailable(
            "This kind of background service is not available on this system.".to_string(),
        )),
    }
}

#[cfg(target_os = "macos")]
pub use mac::SmAppService;

#[cfg(target_os = "macos")]
mod mac {
    use crate::port::{AppService, AppServiceStatus, RegisterError};
    use crate::spec::{Kind, ServiceSpec};
    use objc2::rc::Retained;
    use objc2_foundation::NSString;
    use objc2_service_management::{SMAppService as Sm, SMAppServiceStatus};

    /// `SMAppService` for the plist `Contents/Library/Launch{Agents,Daemons}/<label>.plist` in the app bundle.
    pub struct SmAppService {
        plist: String,
        daemon: bool,
    }

    impl SmAppService {
        /// The bundled plist of `spec`.
        pub fn new(spec: &ServiceSpec) -> Self {
            Self {
                plist: format!("{}.plist", spec.label()),
                daemon: spec.kind() == Kind::SystemService,
            }
        }

        fn service(&self) -> Retained<Sm> {
            let name = NSString::from_str(&self.plist);
            // SAFETY: class methods taking a valid NSString.
            unsafe {
                if self.daemon {
                    Sm::daemonServiceWithPlistName(&name)
                } else {
                    Sm::agentServiceWithPlistName(&name)
                }
            }
        }
    }

    impl AppService for SmAppService {
        fn status(&mut self) -> AppServiceStatus {
            // SAFETY: a property read on a valid object.
            let s = unsafe { self.service().status() };
            if s == SMAppServiceStatus::Enabled {
                AppServiceStatus::Enabled
            } else if s == SMAppServiceStatus::RequiresApproval {
                AppServiceStatus::RequiresApproval
            } else if s == SMAppServiceStatus::NotFound {
                AppServiceStatus::NotFound
            } else {
                AppServiceStatus::NotRegistered
            }
        }

        fn register(&mut self) -> Result<(), RegisterError> {
            let s = self.service();
            // SAFETY: a method call on a valid object; the error comes back as a retained NSError.
            match unsafe { s.registerAndReturnError() } {
                Ok(()) => Ok(()),
                Err(e) => match self.status() {
                    AppServiceStatus::RequiresApproval => Err(RegisterError::NeedsApproval),
                    // Still no record after a register that failed: it cannot be registered from where it is.
                    AppServiceStatus::NotFound => Err(RegisterError::Unavailable(format!(
                        "macOS cannot register the launch agent in the app ({}): open the app from the Applications folder.",
                        e.localizedDescription()
                    ))),
                    _ => Err(RegisterError::Other(e.localizedDescription().to_string())),
                },
            }
        }

        fn unregister(&mut self) -> Result<(), String> {
            // SAFETY: a method call on a valid object.
            unsafe { self.service().unregisterAndReturnError() }
                .map_err(|e| e.localizedDescription().to_string())
        }

        fn open_login_items(&mut self) {
            // SAFETY: a class method without arguments.
            unsafe { Sm::openSystemSettingsLoginItems() };
        }
    }
}

#[cfg(windows)]
pub use win::RunAsElevator;

#[cfg(windows)]
mod win {
    use crate::port::{Elevator, RegisterError};
    use std::path::Path;
    use std::time::Duration;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    use windows::core::{HSTRING, PCWSTR};

    /// Windows' elevation prompt (UAC). The app never sees a password.
    pub struct RunAsElevator;

    impl Elevator for RunAsElevator {
        fn run_elevated(
            &mut self,
            program: &Path,
            args: &[&str],
            timeout: Duration,
        ) -> Result<i32, RegisterError> {
            const ERROR_CANCELLED: i32 = 1223;
            let verb = HSTRING::from("runas");
            let file = HSTRING::from(program.as_os_str());
            let params = HSTRING::from(
                args.iter()
                    .map(|a| crate::render::windows_quote(a))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            let mut info = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
                lpVerb: PCWSTR(verb.as_ptr()),
                lpFile: PCWSTR(file.as_ptr()),
                lpParameters: PCWSTR(params.as_ptr()),
                nShow: SW_HIDE.0,
                ..Default::default()
            };
            // SAFETY: every pointer in `info` refers to an HSTRING that outlives the call.
            if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
                return Err(if e.code().0 & 0xFFFF == ERROR_CANCELLED {
                    RegisterError::Declined
                } else {
                    RegisterError::Other(format!("{} could not be started: {e}", program.display()))
                });
            }
            if info.hProcess.is_invalid() {
                return Ok(0);
            }
            let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: a valid process handle we own; waited on and closed once.
            let code = unsafe {
                let waited = WaitForSingleObject(info.hProcess, ms);
                let mut code = 1u32;
                if waited == WAIT_OBJECT_0 {
                    let _ = GetExitCodeProcess(info.hProcess, &mut code);
                }
                let _ = CloseHandle(info.hProcess);
                code
            };
            Ok(i32::try_from(code).unwrap_or(1))
        }
    }
}

#[cfg(windows)]
pub use scm::run_as_service;

/// The Windows service host's Win32 side: the SCM dispatcher, the sessions and starting the program in one
/// ([`crate::scm_host`] has the logic and its tests). Adapted from Pitwall's `service/windows.rs`.
#[cfg(windows)]
mod scm {
    use crate::render::windows_quote;
    use crate::scm_host::{
        Control, HostEvent, HostTimings, HostedChild, Sessions, Target, UserSession,
        env_block_with, pick_user_session, supervise,
    };
    use crate::spec::ServiceSpec;
    use crate::verbs::Verb;
    use std::ffi::{OsString, c_void};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Security::{
        DuplicateTokenEx, SECURITY_ATTRIBUTES, SecurityIdentification, TOKEN_ALL_ACCESS,
        TokenPrimary,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
    use windows::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSActive, WTSDisconnected,
        WTSEnumerateSessionsW, WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQueryUserToken,
    };
    use windows::Win32::System::Threading::{
        CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT,
        CreateProcessAsUserW, DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT,
        InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
        STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
        WaitForSingleObject,
    };
    use windows::core::{PCWSTR, PWSTR};
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
    const NO_SESSION: u32 = 0xFFFF_FFFF;
    /// How long the program's `stop` verb may take.
    const STOP_VERB_TIMEOUT: Duration = Duration::from_secs(10);

    struct Config {
        spec: ServiceSpec,
        exe: PathBuf,
    }

    static CONFIG: OnceLock<Config> = OnceLock::new();

    define_windows_service!(ffi_service_main, service_main);

    /// Hands this process to the Service Control Manager (the `--service` verb) and returns when the service stopped.
    /// The program `exe` (normally this executable) is started in the user's session with the spec's arguments and
    /// supervised variable. The app's log (`paddock-log`) is set up before; the host writes its events there.
    pub fn run_as_service(spec: &ServiceSpec, exe: PathBuf) -> Result<(), String> {
        let name = spec.name().to_string();
        let _ = CONFIG.set(Config {
            spec: spec.clone(),
            exe,
        });
        service_dispatcher::start(name, ffi_service_main)
            .map_err(|e| format!("Starting as a Windows service failed: {e}"))
    }

    fn status(state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP
                    | ServiceControlAccept::SHUTDOWN
                    | ServiceControlAccept::SESSION_CHANGE
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(if state == ServiceState::StopPending {
                30
            } else {
                0
            }),
            process_id: None,
        }
    }

    fn service_main(_arguments: Vec<OsString>) {
        let Some(config) = CONFIG.get() else { return };
        let (tx, rx) = mpsc::channel::<Control>();
        let handler = move |control| match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(Control::Stop);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::SessionChange(_) => {
                let _ = tx.send(Control::SessionChange);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let handle = match service_control_handler::register(config.spec.name(), handler) {
            Ok(h) => h,
            Err(e) => {
                log::error!(error:% = e; "the service control handler could not be registered");
                return;
            }
        };
        let _ = handle.set_service_status(status(ServiceState::Running));
        log::info!("service started");
        let mut sessions = WtsSessions { config, rx };
        supervise(
            &mut sessions,
            &HostTimings::default(),
            &mut |e: HostEvent| {
                log::info!(event:? = e; "service host");
            },
        );
        let _ = handle.set_service_status(status(ServiceState::StopPending));
        let _ = handle.set_service_status(status(ServiceState::Stopped));
        log::info!("service stopped");
    }

    struct WtsSessions {
        config: &'static Config,
        rx: mpsc::Receiver<Control>,
    }

    impl Sessions for WtsSessions {
        fn target(&mut self) -> Target {
            user_target()
        }

        fn start(&mut self, target: Target) -> Result<Box<dyn HostedChild>, String> {
            let args = self.config.spec.args().to_vec();
            let env = self.config.spec.supervised_env();
            match target {
                Target::Nobody => Err("nobody is logged on".into()),
                Target::NotSystem => start_plain(&self.config.exe, &args, env)
                    .map(|c| Box::new(Plain(c, self.config.exe.clone())) as Box<dyn HostedChild>),
                Target::Session(id) => {
                    start_in_session(id, &self.config.exe, &args, env).map(|p| {
                        Box::new(InSession {
                            process: p,
                            session: id,
                            exe: self.config.exe.clone(),
                            env: env.map(str::to_string),
                        }) as Box<dyn HostedChild>
                    })
                }
            }
        }

        fn next_event(&mut self, timeout: Duration) -> Option<Control> {
            match self.rx.recv_timeout(timeout) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => Some(Control::Stop),
            }
        }

        fn pause(&mut self, how_long: Duration) {
            std::thread::sleep(how_long);
        }
    }

    /// A plain child (the service runs as a user: development).
    struct Plain(std::process::Child, PathBuf);

    impl HostedChild for Plain {
        fn session(&self) -> Option<u32> {
            None
        }
        fn exited(&mut self) -> bool {
            !matches!(self.0.try_wait(), Ok(None))
        }
        fn ask_stop(&mut self) -> bool {
            let mut cmd = paddock_proc::command(&self.1);
            cmd.arg(Verb::Stop.word());
            paddock_proc::run_bounded(cmd, paddock_proc::Limits::new(STOP_VERB_TIMEOUT))
                .is_ok_and(|o| o.success())
        }
        fn kill(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn start_plain(
        exe: &Path,
        args: &[String],
        env: Option<&str>,
    ) -> Result<std::process::Child, String> {
        let mut cmd = paddock_proc::command(exe);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(var) = env {
            cmd.env(var, "1");
        }
        cmd.spawn()
            .map_err(|e| format!("{} could not be started: {e}", exe.display()))
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

    fn exited(h: HANDLE) -> bool {
        // SAFETY: `h` is a valid process handle owned by the caller; a zero timeout only polls.
        unsafe { WaitForSingleObject(h, 0) == WAIT_OBJECT_0 }
    }

    struct InSession {
        process: Owned,
        session: u32,
        exe: PathBuf,
        env: Option<String>,
    }

    impl HostedChild for InSession {
        fn session(&self) -> Option<u32> {
            Some(self.session)
        }
        fn exited(&mut self) -> bool {
            exited(self.process.0)
        }
        fn ask_stop(&mut self) -> bool {
            let args = [Verb::Stop.word().to_string()];
            let Ok(helper) = start_in_session(self.session, &self.exe, &args, self.env.as_deref())
            else {
                return false;
            };
            let deadline = Instant::now() + STOP_VERB_TIMEOUT;
            while !exited(helper.0) {
                if Instant::now() >= deadline {
                    // SAFETY: a valid process handle we own; failure (already gone) is ignored.
                    unsafe {
                        // conformance: pid-kill — a process handle we opened at the start and still hold
                        let _ = TerminateProcess(helper.0, 1);
                    }
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            true
        }
        fn kill(&mut self) {
            // SAFETY: a valid process handle we own; failure (already gone) is ignored.
            unsafe {
                // conformance: pid-kill — a process handle we opened at the start and still hold
                let _ = TerminateProcess(self.process.0, 1);
            }
        }
    }

    /// Where the program should run now (see `scm_host::pick_user_session`).
    fn user_target() -> Target {
        // SAFETY: no arguments; returns a session id or 0xFFFFFFFF.
        let console = unsafe { WTSGetActiveConsoleSessionId() };
        let console = (console != NO_SESSION).then_some(console);
        let mut candidates = Vec::new();
        let mut info: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: valid out pointers; the array is read below and freed with WTSFreeMemory exactly once.
        if unsafe {
            WTSEnumerateSessionsW(Some(WTS_CURRENT_SERVER_HANDLE), 0, 1, &mut info, &mut count)
        }
        .is_ok()
            && !info.is_null()
        {
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
            // SAFETY: `token` is a valid out pointer; the handle is closed by `Owned`'s drop.
            match unsafe { WTSQueryUserToken(id, &mut token) } {
                Ok(()) => {
                    drop(Owned(token));
                    users.push(UserSession { id, active });
                }
                Err(e) if e.code().0 & 0xFFFF == ERROR_PRIVILEGE_NOT_HELD => {
                    return Target::NotSystem;
                }
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

    /// Starts `exe args…` hidden in `session` with that user's token and environment (plus `env=1`); its standard
    /// handles are `NUL` and it inherits no other handle of the service.
    fn start_in_session(
        session: u32,
        exe: &Path,
        args: &[String],
        env: Option<&str>,
    ) -> Result<Owned, String> {
        let mut user = HANDLE::default();
        // SAFETY: valid out pointer; owned below.
        unsafe { WTSQueryUserToken(session, &mut user) }
            .map_err(|e| win_err("Getting the logged-on user's token", e))?;
        let user = Owned(user);
        let mut primary = HANDLE::default();
        // SAFETY: `user.0` is a valid token; `primary` is a valid out pointer, owned below.
        unsafe {
            DuplicateTokenEx(
                user.0,
                TOKEN_ALL_ACCESS,
                None,
                SecurityIdentification,
                TokenPrimary,
                &mut primary,
            )
        }
        .map_err(|e| win_err("Preparing the user's token", e))?;
        let primary = Owned(primary);

        let mut block_ptr: *mut c_void = std::ptr::null_mut();
        // SAFETY: valid out pointer and token; the block is destroyed by the guard below.
        unsafe { CreateEnvironmentBlock(&mut block_ptr, Some(primary.0), false) }
            .map_err(|e| win_err("Building the user's environment", e))?;
        struct EnvGuard(*mut c_void);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                // SAFETY: created by CreateEnvironmentBlock and destroyed once.
                unsafe {
                    let _ = DestroyEnvironmentBlock(self.0);
                }
            }
        }
        let _env_guard = EnvGuard(block_ptr);
        // SAFETY: the block is a list of NUL-terminated UTF-16 strings ended by an empty one, as documented; it is read up
        // to that end only, before it is destroyed.
        let block = unsafe {
            let p = block_ptr.cast::<u16>();
            let mut n = 0usize;
            while !(*p.add(n) == 0 && *p.add(n + 1) == 0) {
                n += 1;
            }
            std::slice::from_raw_parts(p, n + 1).to_vec()
        };
        let mut block = match env {
            Some(var) => env_block_with(&block, &format!("{var}=1")),
            None => [block, vec![0]].concat(),
        };

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
        let null_h = Owned(null_h);
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
        let mut line_text = windows_quote(&program);
        for a in args {
            line_text.push(' ');
            line_text.push_str(&windows_quote(a));
        }
        let mut line = wide(&line_text);
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
                CREATE_UNICODE_ENVIRONMENT
                    | CREATE_NO_WINDOW
                    | CREATE_NEW_PROCESS_GROUP
                    | EXTENDED_STARTUPINFO_PRESENT,
                Some(block.as_mut_ptr().cast()),
                PCWSTR::null(),
                &si.StartupInfo,
                &mut pi,
            )
        }
        .map_err(|e| win_err(&format!("Starting {program} in the user's session"), e))?;
        drop(Owned(pi.hThread));
        Ok(Owned(pi.hProcess))
    }
}
