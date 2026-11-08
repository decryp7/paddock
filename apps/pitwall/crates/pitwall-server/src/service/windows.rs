//! The Windows side of Pitwall's service verbs: the LocalSystem host (`--service`), `install` with the Program Files check
//! (plan B6) before the shared elevated step, `uninstall` and `status` over the Service Control Manager alone, `check-install`,
//! and the read-back of the registration through one handle with DELETE access. The verbs run elevated (the installer,
//! Windows' prompt), so they use no manager, no record and no data folder: nothing in a folder the user controls, and no path
//! from the environment (paddock-service `elevated`). The decisions are pure and tested on every OS ([`super::placement`], [`super::install_planned`]); this file
//! only passes the real ports.
//!
//! The service writes no file and reads no environment variable of its own (B6 security review P1-1, P1-2): a log folder a
//! user could make first (a junction to an object-manager link) would let that user have SYSTEM create or delete any file.
//! It writes to the OS log, the Application event log under the source `Pitwall` (paddock-service `os_log`, ADR 0054 §2.3);
//! the lines hold no request content and no user's paths. A test keeps file and environment calls out of this file and of the
//! shared host code it calls.
#![allow(unsafe_code)]

use super::placement;
use paddock_service::OsLog;
use pitwall_service::{spec, NAME};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Storage::FileSystem::DELETE;
use windows::Win32::System::Services::{
    CloseServiceHandle, DeleteService, OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QUERY_SERVICE_CONFIGW, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_QUERY_CONFIG,
};

/// The service's OS log, opened once.
fn os_log() -> &'static Arc<dyn OsLog> {
    static LOG: OnceLock<Arc<dyn OsLog>> = OnceLock::new();
    LOG.get_or_init(|| paddock_service::os_log::open(&spec()))
}

fn log(line: &str) {
    os_log().write(log::Level::Info, line);
}

/// Runs under the Windows Service Control Manager: the shared host starts this program in the logged-on user's session and
/// writes to the OS log.
pub(super) fn host(program: &Path) -> i32 {
    match paddock_service::native::host_program(&spec(), program.to_path_buf()) {
        Ok(code) => i32::try_from(code).unwrap_or(1),
        Err(e) => super::report(Err(e.to_string())),
    }
}

/// The plan for this program over the real folders and security, given what the service manager holds now.
fn plan(program: &str) -> Result<placement::Install, placement::Refusal> {
    let registered = Service::open(SERVICE_QUERY_CONFIG).and_then(|s| s.image_path());
    placement::plan_install(
        program,
        registered.as_deref(),
        &paddock_service::privileged::OsKnownFolders,
        &mut paddock_service::privileged::trust_real::RealTrust,
    )
}

/// What `install` would decide, without changing anything (no elevation: the folders' security and the registration can be
/// read by a standard user). Logs nothing: the caller shows the words.
pub(super) fn check_install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the program's own path cannot be found: {e}"))?;
    super::check_verdict(plan(&exe.to_string_lossy()))
}

/// A registration verb, elevated or not: `install` (the plan first, then the shared elevated step, then the read-back; see
/// [`super::install_planned`]), `uninstall` and `status` over the Service Control Manager alone ([`super::scm_verb`]).
pub(super) fn run_verb(verb: paddock_service::Verb, program: &Path) -> Result<String, String> {
    let mut scm = paddock_service::elevated::windows_service(&spec(), program);
    if verb != paddock_service::Verb::Install {
        return super::scm_verb(verb, &mut scm);
    }
    let text = program.to_string_lossy().into_owned();
    let mut reg = ReadBack(None);
    super::install_planned(plan(&text), &text, &mut scm, &mut reg, &mut log)
}

/// The read-back: one handle with query and DELETE access, opened at the first read and used for the removal.
struct ReadBack(Option<Service>);

impl super::Registered for ReadBack {
    fn held(&mut self) -> Option<String> {
        self.0 = Service::open(SERVICE_QUERY_CONFIG | DELETE.0);
        self.0
            .as_ref()
            .and_then(Service::image_path)
            .and_then(|p| placement::program_of_image_path(&p))
    }

    fn remove(&mut self) -> Result<(), String> {
        let service = self.0.as_ref().ok_or("the registration could not be opened")?;
        // SAFETY: a service handle opened with DELETE access, alive while `service` is.
        unsafe { DeleteService(service.service) }.map_err(|e| e.to_string())
    }
}

/// An open service of Pitwall's name, with the service manager handle it came from.
struct Service {
    manager: SC_HANDLE,
    service: SC_HANDLE,
}

impl Service {
    /// Pitwall's service with `access`; `None` when there is none or it cannot be opened.
    fn open(access: u32) -> Option<Service> {
        // SAFETY: plain calls with a NUL-terminated name; each handle is closed by `Drop` (or here, on failure).
        unsafe {
            let manager = OpenSCManagerW(None, None, SC_MANAGER_CONNECT).ok()?;
            match OpenServiceW(manager, &HSTRING::from(NAME), access) {
                Ok(service) => Some(Service { manager, service }),
                Err(_) => {
                    paddock_proc::cleanup::logged("closing the service manager handle", CloseServiceHandle(manager));
                    None
                }
            }
        }
    }

    /// The registration's ImagePath (the whole command line, `"<program>" --service`).
    fn image_path(&self) -> Option<String> {
        let mut needed = 0u32;
        // SAFETY: the size query writes only `needed`.
        let _size = unsafe { QueryServiceConfigW(self.service, None, 0, &mut needed) };
        let words = usize::try_from(needed).ok()?.div_ceil(std::mem::size_of::<u64>()).max(1);
        // u64 words keep the buffer aligned for QUERY_SERVICE_CONFIGW.
        let mut buf = vec![0u64; words];
        let config = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
        let size = u32::try_from(words * std::mem::size_of::<u64>()).ok()?;
        // SAFETY: `buf` holds `size` bytes, aligned, and lives until the strings in it have been copied out.
        unsafe { QueryServiceConfigW(self.service, Some(config), size, &mut needed) }.ok()?;
        // SAFETY: the call succeeded, so `config` is filled and its string points into `buf` (or is null).
        let path: PWSTR = unsafe { (*config).lpBinaryPathName };
        if path.is_null() {
            return None;
        }
        // SAFETY: a NUL-terminated string inside `buf`.
        unsafe { path.to_string() }.ok()
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: both handles were opened by `Service::open` and are closed once.
        unsafe {
            paddock_proc::cleanup::logged("closing the service handle", CloseServiceHandle(self.service));
            paddock_proc::cleanup::logged("closing the service manager handle", CloseServiceHandle(self.manager));
        }
    }
}
