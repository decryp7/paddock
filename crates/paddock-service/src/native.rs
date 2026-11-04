//! Native glue (crates/README.md): code only a real desktop runs, which tests cannot reach by rule 7 (they never touch
//! the real Login Items or elevation prompt). Each function makes one platform call or builds the real adapters and
//! hands over to tested code; it is reviewed by hand and left out of the coverage job.
//!
//! - `SmAppService`: `SMAppService` (macOS 13+) for one plist in the app bundle (macOS builds only).
//! - `RunAsElevator`: Windows' own elevation prompt (`ShellExecuteExW` with `runas`; Windows builds only).
//! - [`current_uid`] and [`host_backend`]: the user, and the backend for this OS from [`crate::backend::choose`].
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]
#![allow(unsafe_code)]

use crate::backend::{Backend, Choice, Side, Target, choose};
use crate::error::ServiceError;
use crate::os_log::OsLog;
use crate::service_main::{Context, Hosting, Outcome, ServiceMainError};
use crate::spec::ServiceSpec;
use crate::system::RealSystem;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
        main_app: bool,
    }

    impl SmAppService {
        /// The bundled plist of `spec`.
        pub fn new(spec: &ServiceSpec) -> Self {
            Self {
                plist: format!("{}.plist", spec.label()),
                daemon: spec.kind() == Kind::SystemService,
                main_app: false,
            }
        }

        /// The app itself as a login item (`SMAppService.mainApp`: one row in Login Items, nothing in `~/Library/LaunchAgents`).
        pub fn main_app() -> Self {
            Self {
                plist: String::new(),
                daemon: false,
                main_app: true,
            }
        }

        fn service(&self) -> Retained<Sm> {
            let name = NSString::from_str(&self.plist);
            // SAFETY: class methods taking a valid NSString.
            unsafe {
                if self.main_app {
                    Sm::mainAppService()
                } else if self.daemon {
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
                    paddock_proc::cleanup::logged(
                        "reading the elevated program's exit code (reported as 1)",
                        GetExitCodeProcess(info.hProcess, &mut code),
                    );
                }
                paddock_proc::cleanup::logged(
                    "closing the elevated program's handle",
                    CloseHandle(info.hProcess),
                );
                code
            };
            Ok(i32::try_from(code).unwrap_or(1))
        }
    }
}

/// What `service_main` hands the host: the body to run and whether it wants session-change events.
pub(crate) struct Job {
    pub(crate) run: Box<dyn FnOnce(Context) -> Outcome + Send>,
    /// The body is the Windows service host: it moves a program with the user's session (read by the SCM entry point).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) sessions: bool,
}

pub(crate) type Slot = Arc<Mutex<Option<Job>>>;

/// Runs the job in `slot` under `hosting` and returns the code it ended with.
pub(crate) fn host(
    spec: &ServiceSpec,
    hosting: Hosting,
    os_log: Arc<dyn OsLog>,
    slot: Slot,
) -> Result<u32, ServiceMainError> {
    log::info!(service = spec.name(), hosting:? = hosting; "service: hosting starts");
    match hosting {
        #[cfg(windows)]
        Hosting::ServiceManager => scm::dispatch(spec.name(), os_log, slot),
        #[cfg(not(windows))]
        Hosting::ServiceManager => Err(ServiceMainError::NotStartedByManager(
            "The service command is for Windows; use `serve`.".to_string(),
        )),
        Hosting::Foreground => foreground(os_log, slot, true),
        Hosting::Embedded => foreground(os_log, slot, false),
    }
}

/// Runs the job on this thread. With `signals` (on Unix) SIGTERM and SIGINT mean stop: handled once per process and
/// waited for on a thread of their own, so the stop is an event, not a poll.
fn foreground(os_log: Arc<dyn OsLog>, slot: Slot, signals: bool) -> Result<u32, ServiceMainError> {
    let Some(job) = crate::service_main::take_job(&slot) else {
        return Err(ServiceMainError::NotStartedByManager(
            "the service body already ran in this process".to_string(),
        ));
    };
    let (stop, tx, events) = crate::service_main::stop_pair();
    #[cfg(unix)]
    if signals && let Err(why) = signals::stop_on_signals(stop.clone(), tx.clone()) {
        os_log.write(
            log::Level::Warn,
            &format!("SIGTERM and SIGINT will not stop the service gracefully: {why}"),
        );
    }
    // Signal handlers are Unix's; on Windows the SCM's controls are the stop.
    #[cfg(not(unix))]
    debug_assert!(!signals || cfg!(windows));
    let context = Context {
        stop,
        events,
        os_log,
    };
    // `tx` lives to the end of the run, so a body waiting on its events is not woken by a closed channel.
    let code = (job.run)(context).code;
    drop(tx);
    Ok(code)
}

/// The device and inode of standard error (`None` where it cannot be read), to compare with `JOURNAL_STREAM`.
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn stderr_identity() -> Option<(u64, u64)> {
    // SAFETY: `stat` is written by fstat before it is read; fd 2 is only asked about.
    unsafe {
        let mut stat: libc::stat = std::mem::zeroed();
        (libc::fstat(2, &mut stat) == 0).then(|| (stat.st_dev as u64, stat.st_ino as u64))
    }
}

#[cfg(unix)]
mod signals {
    use crate::scm_host::Control;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex, OnceLock, PoisonError};

    /// The write end of the pipe the handler writes to (-1: not installed).
    static WAKE_FD: AtomicI32 = AtomicI32::new(-1);
    /// Where the stop goes: the run that is going now.
    static ROUTE: Mutex<Option<(Arc<AtomicBool>, Sender<Control>)>> = Mutex::new(None);
    /// The handlers are installed once per process.
    static INSTALLED: OnceLock<Result<(), String>> = OnceLock::new();

    /// Where this thread keeps `errno` (null when this Unix is not known: the handler then does not restore it).
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    fn errno_location() -> *mut libc::c_int {
        // SAFETY: returns the calling thread's errno cell.
        unsafe { libc::__error() }
    }

    /// See above.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn errno_location() -> *mut libc::c_int {
        // SAFETY: returns the calling thread's errno cell.
        unsafe { libc::__errno_location() }
    }

    /// See above.
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "linux",
        target_os = "android"
    )))]
    fn errno_location() -> *mut libc::c_int {
        std::ptr::null_mut()
    }

    /// The handler: async-signal-safe, it only writes one byte to the (non-blocking) pipe and keeps `errno`.
    extern "C" fn on_signal(_signal: libc::c_int) {
        let fd = WAKE_FD.load(Ordering::Relaxed);
        if fd >= 0 {
            // SAFETY: write(2) is async-signal-safe; the byte lives on this stack frame.
            unsafe {
                let errno = errno_location();
                let saved = if errno.is_null() { 0 } else { *errno };
                let byte = 1u8;
                // A full pipe already holds a wake-up; nothing is lost by not writing again.
                let _written = libc::write(fd, std::ptr::from_ref(&byte).cast(), 1);
                if !errno.is_null() {
                    *errno = saved;
                }
            }
        }
    }

    /// `fcntl` flags on `fd`: close on exec, and non-blocking when `nonblocking`.
    fn set_flags(fd: libc::c_int, nonblocking: bool) -> Result<(), String> {
        // SAFETY: plain fcntl calls on a descriptor this module just created.
        unsafe {
            if libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) != 0 {
                return Err(format!(
                    "fcntl(FD_CLOEXEC) failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if nonblocking {
                let flags = libc::fcntl(fd, libc::F_GETFL);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) != 0 {
                    return Err(format!(
                        "fcntl(O_NONBLOCK) failed: {}",
                        std::io::Error::last_os_error()
                    ));
                }
            }
        }
        Ok(())
    }

    /// Puts the default action back for SIGTERM and SIGINT: after the first stop request a second one ends the process.
    fn restore_default() {
        for signal in [libc::SIGTERM, libc::SIGINT] {
            // SAFETY: SIG_DFL is a valid disposition for these signals.
            if unsafe { libc::signal(signal, libc::SIG_DFL) } == libc::SIG_ERR {
                log::warn!(signal = signal; "service: the default action of a signal could not be restored");
            }
        }
    }

    fn install() -> Result<(), String> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` has room for the two descriptors pipe(2) returns.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(format!("pipe failed: {}", std::io::Error::last_os_error()));
        }
        let (read_fd, write_fd) = (fds[0], fds[1]);
        set_flags(read_fd, false)?;
        set_flags(write_fd, true)?;
        WAKE_FD.store(write_fd, Ordering::Relaxed);
        for signal in [libc::SIGTERM, libc::SIGINT] {
            // SAFETY: `action` is fully initialised; the handler is async-signal-safe.
            let rc = unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut())
            };
            if rc != 0 {
                return Err(format!(
                    "sigaction({signal}) failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
        std::thread::Builder::new()
            .name("signal-wait".into())
            .spawn(move || {
                let mut byte = 0u8;
                loop {
                    // SAFETY: reads one byte into a valid buffer from the pipe this module owns.
                    let n = unsafe { libc::read(read_fd, std::ptr::from_mut(&mut byte).cast(), 1) };
                    if n == 1 {
                        log::info!("service: stop requested by a signal");
                        // The default action is back before the body hears of the stop, so a second signal that
                        // follows the event always ends a body that ignores it.
                        restore_default();
                        let route = ROUTE.lock().unwrap_or_else(PoisonError::into_inner).clone();
                        if let Some((stop, tx)) = route {
                            crate::service_main::signal_stop(&stop, &tx);
                        }
                        return;
                    }
                    let error = std::io::Error::last_os_error();
                    if n < 0 && error.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    log::error!(error:% = error; "service: the signal pipe failed; signals no longer stop the service");
                    return;
                }
            })
            .map(drop)
            .map_err(|e| format!("the signal thread could not start: {e}"))
    }

    /// Routes SIGTERM and SIGINT to this run's stop. The handlers and the thread that waits on their pipe are installed
    /// once per process; the stop is an event, not a poll.
    pub(super) fn stop_on_signals(
        stop: Arc<AtomicBool>,
        tx: Sender<Control>,
    ) -> Result<(), String> {
        *ROUTE.lock().unwrap_or_else(PoisonError::into_inner) = Some((stop, tx));
        INSTALLED.get_or_init(install).clone()
    }
}

/// The host of a program that runs in the logged-on user's session (Pitwall, Pitcrew): the SCM runs `--service`, the body
/// supervises the program ([`crate::scm_host::supervise`]). Returns the exit code the service ended with.
///
/// # Errors
/// [`ServiceMainError`] when this process was not started by the SCM.
#[cfg(windows)]
pub fn host_program(spec: &ServiceSpec, exe: PathBuf) -> Result<u32, ServiceMainError> {
    scm::host_program(spec, exe)
}

/// The Windows Application event log under the service's own source (registered by the install plan,
/// [`crate::render::event_source_commands`]).
#[cfg(windows)]
pub use eventlog::EventLog;

#[cfg(windows)]
mod eventlog {
    use crate::os_log::{OsLog, cut};
    use log::Level;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::EventLog::{
        DeregisterEventSource, EVENTLOG_ERROR_TYPE, EVENTLOG_INFORMATION_TYPE,
        EVENTLOG_WARNING_TYPE, RegisterEventSourceW, ReportEventW,
    };
    use windows::core::PCWSTR;

    /// The longest string `ReportEventW` takes is 31 839 characters.
    const MAX_CHARS: usize = 30_000;
    /// The generic message table of `EventCreate.exe` prints ids 1 to 1000 with their one string.
    const EVENT_ID: u32 = 1;

    /// An open event source.
    pub struct EventLog(HANDLE);

    // SAFETY: an event log handle may be used and closed from any thread.
    unsafe impl Send for EventLog {}
    // SAFETY: ReportEventW serialises on the handle.
    unsafe impl Sync for EventLog {}

    impl EventLog {
        /// Opens the source `source` of the Application log on this computer.
        ///
        /// # Errors
        /// The Win32 error in words.
        pub fn open(source: &str) -> Result<EventLog, String> {
            let name: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `name` is NUL-terminated and outlives the call.
            unsafe { RegisterEventSourceW(PCWSTR::null(), PCWSTR(name.as_ptr())) }
                .map(EventLog)
                .map_err(|e| format!("RegisterEventSourceW failed: {}", e.message()))
        }
    }

    impl OsLog for EventLog {
        fn write(&self, level: Level, text: &str) {
            let kind = match level {
                Level::Error => EVENTLOG_ERROR_TYPE,
                Level::Warn => EVENTLOG_WARNING_TYPE,
                _ => EVENTLOG_INFORMATION_TYPE,
            };
            let text: Vec<u16> = cut(text, MAX_CHARS)
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let strings = [PCWSTR(text.as_ptr())];
            // SAFETY: the handle is open; `strings` holds one NUL-terminated string that outlives the call.
            let reported =
                unsafe { ReportEventW(self.0, kind, 0, EVENT_ID, None, 0, Some(&strings), None) };
            if let Err(e) = reported {
                // The event log is the log of last resort: say it on standard error and in the app log.
                log::warn!(error:% = e; "the event log refused a line");
                eprintln!("event log refused a line: {e}");
            }
        }
    }

    impl Drop for EventLog {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by `open` and is closed once.
            paddock_proc::cleanup::logged("closing the event source", unsafe {
                DeregisterEventSource(self.0)
            });
        }
    }
}

/// The macOS unified log through `syslog(3)`: the lines carry the process and the service's identifier.
#[cfg(target_os = "macos")]
pub use syslog::SysLog;

#[cfg(target_os = "macos")]
mod syslog {
    use crate::os_log::{OsLog, check_syslog_ident, unified_priority};
    use log::Level;
    use std::ffi::CString;
    use std::sync::OnceLock;

    /// `openlog` keeps the pointer to its identifier, so it lives as long as the process.
    static IDENT: OnceLock<CString> = OnceLock::new();

    /// An open syslog connection.
    pub struct SysLog;

    impl SysLog {
        /// Opens syslog under `ident` (the service's launchd label).
        ///
        /// # Errors
        /// When the identifier holds a NUL.
        pub fn open(ident: &str) -> Result<SysLog, String> {
            let wanted =
                CString::new(ident).map_err(|e| format!("the identifier is not valid: {e}"))?;
            let kept = IDENT.get_or_init(|| wanted);
            check_syslog_ident(kept.to_str().ok(), ident)?;
            // SAFETY: `kept` is a static NUL-terminated string.
            unsafe { libc::openlog(kept.as_ptr(), libc::LOG_PID, libc::LOG_DAEMON) };
            Ok(SysLog)
        }
    }

    impl OsLog for SysLog {
        fn write(&self, level: Level, text: &str) {
            let priority = libc::c_int::from(unified_priority(level));
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let Ok(line) = CString::new(line.replace('\0', " ")) else {
                    continue;
                };
                // SAFETY: "%s" with one NUL-terminated string argument.
                unsafe { libc::syslog(priority, c"%s".as_ptr(), line.as_ptr()) };
            }
        }
    }
}

/// The Windows service host's Win32 side: the SCM dispatcher, the sessions and starting the program in one
/// ([`crate::scm_host`] has the logic and its tests). Adapted from Pitwall's `service/windows.rs`.
#[cfg(windows)]
mod scm {
    use super::Slot;
    use crate::os_log::OsLog;
    use crate::render::windows_quote;
    use crate::scm_host::{
        Control, HostEvent, HostTimings, HostedChild, Sessions, Target, UserSession,
        env_block_with, pick_user_session, supervise,
    };
    use crate::service_main::{
        Context, HOST_EXIT_CODE, Hosting, STOP_TICK, ScmReport, ScmState, ScmStatus, ServiceExit,
        ServiceMainError, StatusPort, run_under_scm,
    };
    use crate::spec::ServiceSpec;
    use crate::verbs::Verb;
    use std::ffi::{OsString, c_void};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock, PoisonError};
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
    use windows_service::service_control_handler::{
        self, ServiceControlHandlerResult, ServiceStatusHandle,
    };
    use windows_service::{define_windows_service, service_dispatcher};

    const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
    const NO_SESSION: u32 = 0xFFFF_FFFF;
    /// How long the program's `stop` verb may take.
    const STOP_VERB_TIMEOUT: Duration = Duration::from_secs(10);

    /// The job `service_main` handed over, and how the SCM entry point reaches it (the SCM calls a plain function).
    struct Dispatch {
        os_log: Arc<dyn OsLog>,
        slot: Slot,
        name: String,
        code: Mutex<Option<u32>>,
    }

    static DISPATCH: OnceLock<Dispatch> = OnceLock::new();

    define_windows_service!(ffi_service_main, scm_service_main);

    /// Hands this process to the Service Control Manager and returns when the service stopped, with the exit code it
    /// reported.
    pub(super) fn dispatch(
        name: &str,
        os_log: Arc<dyn OsLog>,
        slot: Slot,
    ) -> Result<u32, ServiceMainError> {
        let dispatch = Dispatch {
            os_log,
            slot,
            name: name.to_string(),
            code: Mutex::new(None),
        };
        if DISPATCH.set(dispatch).is_err() {
            return Err(ServiceMainError::NotStartedByManager(
                "the service was already dispatched in this process".to_string(),
            ));
        }
        let began = Instant::now();
        service_dispatcher::start(name, ffi_service_main).map_err(|e| {
            ServiceMainError::NotStartedByManager(format!(
                "This program must be started by Windows as a service: {e}"
            ))
        })?;
        let code = DISPATCH
            .get()
            .and_then(|d| *d.code.lock().unwrap_or_else(PoisonError::into_inner))
            .unwrap_or(HOST_EXIT_CODE);
        log::info!(code = code, ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "service: dispatcher returned");
        Ok(code)
    }

    /// The service supervising a program in the user's session (see [`super::host_program`]).
    pub(super) fn host_program(spec: &ServiceSpec, exe: PathBuf) -> Result<u32, ServiceMainError> {
        let spec = spec.clone();
        crate::service_main::service_main_with(
            &spec.clone(),
            Hosting::ServiceManager,
            true,
            move |context| {
                let Context { events, os_log, .. } = context;
                let mut sessions = WtsSessions {
                    spec: spec.clone(),
                    exe,
                    rx: events,
                };
                supervise(
                    &mut sessions,
                    &HostTimings::default(),
                    &mut |e: HostEvent| {
                        log::info!(event:? = e; "service host");
                        os_log.write(log::Level::Info, &format!("service host: {e:?}"));
                    },
                );
                Ok::<(), HostError>(())
            },
        )
    }

    /// The host cannot fail once it runs; the type gives it the exit-code table `service_main` wants.
    #[derive(Debug)]
    struct HostError;

    impl std::fmt::Display for HostError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the service host stopped")
        }
    }

    impl std::error::Error for HostError {}

    impl ServiceExit for HostError {
        fn exit_code(&self) -> u32 {
            2
        }
    }

    /// The report as the SCM's own structure.
    pub(super) fn service_status(report: &ScmReport) -> ServiceStatus {
        let state = match report.state {
            ScmState::Running => ServiceState::Running,
            ScmState::StopPending => ServiceState::StopPending,
            ScmState::Stopped => ServiceState::Stopped,
        };
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                let base = ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN;
                if report.accepts_sessions {
                    base | ServiceControlAccept::SESSION_CHANGE
                } else {
                    base
                }
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: if report.exit_code == 0 {
                ServiceExitCode::Win32(0)
            } else {
                ServiceExitCode::ServiceSpecific(report.exit_code)
            },
            checkpoint: report.checkpoint,
            wait_hint: report.wait_hint,
            process_id: None,
        }
    }

    /// `SetServiceStatus` through the handle the control handler registration returned (set once, after it exists: the
    /// handler itself needs the status to report a stop).
    struct ScmPort {
        handle: OnceLock<ServiceStatusHandle>,
    }

    impl StatusPort for ScmPort {
        fn report(&self, report: &ScmReport) -> Result<(), String> {
            let handle = self
                .handle
                .get()
                .ok_or_else(|| "the control handler is not registered yet".to_string())?;
            handle
                .set_service_status(service_status(report))
                .map_err(|e| e.to_string())
        }
    }

    fn scm_service_main(_arguments: Vec<OsString>) {
        let Some(dispatch) = DISPATCH.get() else {
            return;
        };
        let os_log = dispatch.os_log.clone();
        let Some(job) = crate::service_main::take_job(&dispatch.slot) else {
            os_log.write(
                log::Level::Error,
                "The service main was called twice in this process.",
            );
            return;
        };
        let began = Instant::now();
        let (stop, tx, events) = crate::service_main::stop_pair();
        let port = Arc::new(ScmPort {
            handle: OnceLock::new(),
        });
        let status = Arc::new(ScmStatus::new(port.clone(), os_log.clone(), job.sessions));
        let (flag, sessions, on_stop) = (stop.clone(), job.sessions, status.clone());
        let handler = move |control| match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                // The SCM is told the stop has begun (with a wait hint) before the body is asked to stop.
                on_stop.stop_pending();
                crate::service_main::signal_stop(&flag, &tx);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::SessionChange(_) if sessions => {
                paddock_proc::cleanup::logged(
                    "forwarding a session change after the host loop ended",
                    tx.send(Control::SessionChange),
                );
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let handle = match service_control_handler::register(&dispatch.name, handler) {
            Ok(h) => h,
            Err(e) => {
                log::error!(error:% = e; "the service control handler could not be registered");
                os_log.write(
                    log::Level::Error,
                    &format!("The service control handler could not be registered: {e}"),
                );
                *dispatch.code.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(HOST_EXIT_CODE);
                return;
            }
        };
        if port.handle.set(handle).is_err() {
            log::error!("service: the status handle was set twice");
        }
        log::info!(registered_ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "service: control handler registered");
        let context = Context {
            stop,
            events,
            os_log: os_log.clone(),
        };
        let outcome = run_under_scm(&status, job, context, STOP_TICK);
        *dispatch.code.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome.code);
    }

    struct WtsSessions {
        spec: ServiceSpec,
        exe: PathBuf,
        rx: mpsc::Receiver<Control>,
    }

    impl Sessions for WtsSessions {
        fn target(&mut self) -> Target {
            user_target()
        }

        fn start(&mut self, target: Target) -> Result<Box<dyn HostedChild>, String> {
            let args = self.spec.args().to_vec();
            let env = self.spec.supervised_env();
            match target {
                Target::Nobody => Err("nobody is logged on".into()),
                Target::NotSystem => start_plain(&self.exe, &args, env)
                    .map(|c| Box::new(Plain(c, self.exe.clone())) as Box<dyn HostedChild>),
                Target::Session(id) => start_in_session(id, &self.exe, &args, env).map(|p| {
                    Box::new(InSession {
                        process: p,
                        session: id,
                        exe: self.exe.clone(),
                        env: env.map(str::to_string),
                    }) as Box<dyn HostedChild>
                }),
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
            paddock_proc::cleanup::logged("killing the hosted program", self.0.kill());
            paddock_proc::cleanup::logged("reaping the hosted program", self.0.wait());
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
                    paddock_proc::cleanup::logged("closing an owned handle", CloseHandle(self.0));
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
                        paddock_proc::cleanup::logged(
                            "ending the stop-verb helper",
                            TerminateProcess(helper.0, 1),
                        );
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
                paddock_proc::cleanup::logged(
                    "ending the hosted program",
                    TerminateProcess(self.process.0, 1),
                );
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
                    paddock_proc::cleanup::logged(
                        "destroying the environment block",
                        DestroyEnvironmentBlock(self.0),
                    );
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
        // silent: the sizing call fails with ERROR_INSUFFICIENT_BUFFER by design; the second call below is checked
        #[allow(clippy::let_underscore_must_use, clippy::let_underscore_untyped)]
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

/// Whether this process has a full elevated token (`TokenElevationType` is `TokenElevationTypeFull`). Never the plain
/// `TokenElevation` flag (true for every process with UAC off). Non-Windows systems and a failed query say no.
#[cfg(windows)]
pub fn process_has_full_token() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION_TYPE, TOKEN_QUERY, TokenElevationType,
        TokenElevationTypeFull,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process and an out parameter that lives through the call.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut kind = TOKEN_ELEVATION_TYPE(0);
    let mut returned = 0u32;
    // SAFETY: `kind` is a TOKEN_ELEVATION_TYPE and the size passed is its size.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevationType,
            Some(std::ptr::from_mut(&mut kind).cast()),
            std::mem::size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut returned,
        )
    }
    .is_ok();
    // SAFETY: the token was opened above.
    paddock_proc::cleanup::logged("closing the token handle", unsafe { CloseHandle(token) });
    ok && kind == TokenElevationTypeFull
}

/// Non-Windows: no elevated token.
#[cfg(not(windows))]
pub fn process_has_full_token() -> bool {
    false
}

/// The Windows adapters, exercised on a Windows runner without changing the machine: no registry write, no service
/// installed (ER-14). The event log test writes entries under a source nobody registered; Windows then files them under the
/// Application log with its generic "description cannot be found" wrapper, which is the same text path the real source uses.
#[cfg(all(test, windows))]
mod windows_tests {
    use super::EventLog;
    use super::scm::service_status;
    use crate::os_log::OsLog;
    use crate::service_main::{STOP_WAIT_HINT, ScmReport, ScmState};
    use log::Level;
    use std::time::Duration;
    use windows_service::service::{ServiceControlAccept, ServiceExitCode, ServiceState};

    #[test]
    fn an_event_source_opens_and_with_the_env_switch_takes_lines_of_every_level_and_a_long_one() {
        let source = format!("PaddockServiceTest{}", std::process::id());
        let log = EventLog::open(&source).expect("the event log opens");
        // Writing puts entries into the machine's real Application log (ER-14): only when asked for explicitly.
        if std::env::var_os("PADDOCK_WINDOWS_EVENTLOG_TEST").is_none_or(|v| v != "1") {
            return;
        }
        for level in [Level::Error, Level::Warn, Level::Info, Level::Debug] {
            log.write(level, "paddock-service windows test line");
        }
        log.write(Level::Info, &"é".repeat(50_000));
        log.write(Level::Info, "");
    }

    #[test]
    fn a_source_name_with_a_nul_is_refused_or_cut_without_a_panic() {
        // Windows takes the string up to the NUL; the open must not panic or hang whatever it answers.
        drop(EventLog::open("Paddock\0Test"));
    }

    fn report(state: ScmState, exit_code: u32, sessions: bool, checkpoint: u32) -> ScmReport {
        ScmReport {
            state,
            accepts_sessions: sessions,
            exit_code,
            checkpoint,
            wait_hint: if state == ScmState::StopPending {
                STOP_WAIT_HINT
            } else {
                Duration::ZERO
            },
        }
    }

    #[test]
    fn a_report_becomes_the_status_the_scm_gets() {
        let running = service_status(&report(ScmState::Running, 0, false, 0));
        assert_eq!(running.current_state, ServiceState::Running);
        assert_eq!(
            running.controls_accepted,
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        );
        let host = service_status(&report(ScmState::Running, 0, true, 0));
        assert!(
            host.controls_accepted
                .contains(ServiceControlAccept::SESSION_CHANGE)
        );
        let stopping = service_status(&report(ScmState::StopPending, 0, false, 3));
        assert_eq!(
            (
                stopping.current_state,
                stopping.checkpoint,
                stopping.wait_hint,
                stopping.controls_accepted
            ),
            (
                ServiceState::StopPending,
                3,
                STOP_WAIT_HINT,
                ServiceControlAccept::empty()
            )
        );
        let failed = service_status(&report(ScmState::Stopped, 3, false, 0));
        assert_eq!(failed.exit_code, ServiceExitCode::ServiceSpecific(3));
        let clean = service_status(&report(ScmState::Stopped, 0, false, 0));
        assert_eq!(clean.exit_code, ServiceExitCode::Win32(0));
    }
}
