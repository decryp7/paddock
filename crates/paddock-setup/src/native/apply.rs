//! `apply-update` on Windows (native glue): the calls behind [`crate::apply::UpdatePort`]. No decisions here.

use super::facts::RealDirs;
use super::handle::{self, Descriptor, Owned};
use super::mutex::{self, CreateError};
use super::staging_fs::UpdatesFolder;
use super::wide::to_wide;
use super::{Shared, folders};
use crate::apply::{
    Handover, INSTALLER_ARGS, Layout, Refusal, RunPaths, SourceFacts, UpdatePort, volume_root,
};
use crate::checkdir::{RUN_FOLDER_SDDL, STAGED_FILE_SDDL};
use crate::handoff::{self, Admit};
use crate::localpath::LocalPath;
use crate::log::Sink;
use paddock_updater::signature::{UpdateKey, VerifyError, trusted_comment, verify_file};
use semver::Version;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{
    GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_STANDARD_INFO, FILE_TYPE_DISK,
    FileBasicInfo, FileStandardInfo, GetDriveTypeW, GetFileInformationByHandle,
    GetFileInformationByHandleEx, GetFileType, GetFinalPathNameByHandleW, OPEN_EXISTING,
    SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, INFINITE, PROCESS_INFORMATION, ReleaseMutex,
    STARTUPINFOW, WaitForMultipleObjects,
};

const FILE_SHARE_READ_ONLY: u32 = FILE_SHARE_READ;
/// `GetDriveTypeW` answer for a local fixed disk.
const DRIVE_FIXED: u32 = 3;

/// The machine behind `apply-update`.
pub struct Real<'a> {
    pub layout: Layout,
    pub main: String,
    pub identifier: String,
    pub key: Option<UpdateKey>,
    pub log: Shared<'a>,
    source: Option<File>,
    held: Option<File>,
    namespace: Option<mutex::Namespace>,
    lock: Option<Owned>,
    lock_owned: bool,
    handoff: Option<Owned>,
    signal: Option<Owned>,
    child: Option<Owned>,
}

impl<'a> Real<'a> {
    /// A port for the app installed in `layout.instdir`.
    pub fn new(
        layout: Layout,
        main: String,
        identifier: String,
        key: Option<UpdateKey>,
        log: Shared<'a>,
    ) -> Self {
        Real {
            layout,
            main,
            identifier,
            key,
            log,
            source: None,
            held: None,
            namespace: None,
            lock: None,
            lock_owned: false,
            handoff: None,
            signal: None,
            child: None,
        }
    }
}

fn io(log: &mut dyn Sink, what: &str, e: impl std::fmt::Display) -> Refusal {
    log.line("apply-update", &format!("{what} failed: {e}"));
    Refusal::Io
}

fn squat(log: &mut dyn Sink, what: &str, e: CreateError) -> Refusal {
    match e {
        CreateError::Squatted => {
            log.line(
                "apply-update",
                &format!("the {what} name is taken by an object no administrator made"),
            );
            Refusal::Squatted
        }
        CreateError::Os(code) => io(log, what, code),
    }
}

impl UpdatePort for Real<'_> {
    fn has_key(&self) -> bool {
        self.key.is_some()
    }

    fn prepare_folders(&mut self, run: &RunPaths) -> Result<(), Refusal> {
        // The running program's folder is $INSTDIR; it must pass the same rule as the installer's.
        if let Err(refusal) = crate::checkdir::check(&RealDirs, &self.layout.instdir) {
            self.log.line(
                "apply-update",
                &format!("install folder refused: {refusal}"),
            );
            return Err(Refusal::InstallDir);
        }
        let made = folders::ensure_updates(&self.layout.updates)
            .and_then(|_| folders::create_new(&run.dir, RUN_FOLDER_SDDL))
            .and_then(|_| folders::create_new(&run.tmp, RUN_FOLDER_SDDL));
        made.map_err(|e| {
            self.log
                .line("apply-update", &format!("folder not usable: {e}"));
            Refusal::InstallDir
        })
    }

    fn acquire_mutex(&mut self) -> Result<(), Refusal> {
        // The names are in a private namespace only an elevated process can enter: a standard user cannot take them first.
        let ns = mutex::enter(&self.identifier)
            .map_err(|e| squat(&mut self.log, "installer namespace", e))?;
        let (lock, _existed) = mutex::create_lock(&ns, &self.identifier)
            .map_err(|e| squat(&mut self.log, "installer lock", e))?;
        let tried = mutex::wait(&lock, Some(std::time::Duration::ZERO));
        // Admission looks only at objects inside the private namespace, which no standard user can hold or make.
        let verdict = handoff::admit(tried, mutex::handoff_pending(&ns, &self.identifier));
        if verdict != Admit::Go
            && matches!(
                tried,
                handoff::WaitOutcome::Released | handoff::WaitOutcome::Abandoned
            )
        {
            // The try-acquire succeeded but a hand-over is pending: give the lock back, do not abandon it.
            // SAFETY: this thread just acquired the mutex by the wait above.
            unsafe { ReleaseMutex(lock.0) };
        }
        match verdict {
            Admit::Go => {}
            Admit::Busy => return Err(Refusal::Busy),
            Admit::Failed => {
                return Err(io(&mut self.log, "wait for the lock", handle::last_error()));
            }
        }
        self.lock_owned = true;
        let handoff_event = mutex::create_handoff(&ns, &self.identifier)
            .map_err(|e| squat(&mut self.log, "hand-over event", e))?;
        // A squatted signal never stops an update: the private lock excludes a second install; the app just sees no signal.
        match mutex::create_signal(&self.identifier) {
            Ok(event) => self.signal = Some(event),
            Err(CreateError::Squatted) => self.log.line(
                "apply-update",
                "the app-facing installer signal name is taken by a program no administrator made; going on without it",
            ),
            Err(CreateError::Os(code)) => self.log.line("apply-update", &format!("the installer signal could not be made (error {code}); going on without it")),
        }
        self.handoff = Some(handoff_event);
        self.lock = Some(lock);
        self.namespace = Some(ns);
        Ok(())
    }

    fn clean_previous(&mut self, keep: &RunPaths) {
        let skip = Path::new(&keep.dir)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let mut fs = UpdatesFolder {
            dir: PathBuf::from(&self.layout.updates),
        };
        let mut log = self.log;
        if let Err(e) = crate::staging::clean(&mut fs, skip.as_deref(), &mut log) {
            self.log
                .line("apply-update", &format!("old staging not listed: {e}"));
        }
    }

    fn installed_version(&mut self) -> Result<Version, Refusal> {
        let exe = PathBuf::from(&self.layout.instdir).join(&self.main);
        super::fileversion::file_version(&exe)
            .map_err(|e| io(&mut self.log, "installed version", e))
    }

    fn open_source(&mut self, path: &LocalPath) -> Result<SourceFacts, Refusal> {
        // A directory symlink or junction anywhere above the file could point at a share: refuse it before opening.
        if let Some(parent) = crate::checkdir::ancestors(path.as_str()).last() {
            match super::facts::any_prefix_is_reparse(parent) {
                Ok(false) => {}
                Ok(true) => {
                    self.log
                        .line("apply-update", "a folder above the download is a link");
                    return Err(Refusal::SourceNotLocal);
                }
                Err(why) => {
                    return Err(io(
                        &mut self.log,
                        "check the folders above the download",
                        why,
                    ));
                }
            }
        }
        let wide = to_wide(path.as_str());
        // SAFETY: NUL-terminated path (already parsed as a plain local path). Identification level: a named-pipe server behind
        // the path can identify the elevated caller, never impersonate it. Reparse points are opened, not followed, and refused.
        let h = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ_ONLY,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(io(&mut self.log, "open the download", handle::last_error()));
        }
        // Wrapping it in a File closes it on every path out of here.
        // SAFETY: `h` is a valid owned handle that nothing else closes.
        let file = unsafe { File::from_raw_handle(h as RawHandle) };
        // SAFETY: valid handle for all queries below.
        let is_disk_file = unsafe { GetFileType(h) } == FILE_TYPE_DISK;
        let mut basic = std::mem::MaybeUninit::<FILE_BASIC_INFO>::zeroed();
        let mut std_info = std::mem::MaybeUninit::<FILE_STANDARD_INFO>::zeroed();
        let mut by_handle = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
        // SAFETY: the buffers are the stated structures.
        let (b_ok, s_ok, h_ok) = unsafe {
            (
                GetFileInformationByHandleEx(
                    h,
                    FileBasicInfo,
                    basic.as_mut_ptr().cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                ),
                GetFileInformationByHandleEx(
                    h,
                    FileStandardInfo,
                    std_info.as_mut_ptr().cast(),
                    std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
                ),
                GetFileInformationByHandle(h, by_handle.as_mut_ptr()),
            )
        };
        if b_ok == 0 || s_ok == 0 || h_ok == 0 {
            return Err(io(
                &mut self.log,
                "query the download",
                handle::last_error(),
            ));
        }
        // SAFETY: all three calls succeeded.
        let (basic, std_info, by_handle) = unsafe {
            (
                basic.assume_init(),
                std_info.assume_init(),
                by_handle.assume_init(),
            )
        };
        let mut buf = vec![0u16; 1024];
        // SAFETY: valid buffer.
        let n = unsafe {
            GetFinalPathNameByHandleW(h, buf.as_mut_ptr(), buf.len() as u32, VOLUME_NAME_DOS)
        } as usize;
        let final_path = if n > 0 && n < buf.len() {
            super::wide::from_wide(&buf[..n])
        } else {
            String::new()
        };
        let volume_fixed = volume_root(&final_path).is_some_and(|root| {
            let w = to_wide(&root);
            // SAFETY: NUL-terminated root.
            unsafe { GetDriveTypeW(w.as_ptr()) == DRIVE_FIXED }
        });
        self.source = Some(file);
        Ok(SourceFacts {
            is_disk_file,
            is_directory: basic.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
                || std_info.Directory,
            is_reparse: basic.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
            final_path,
            volume_fixed,
            size: std_info.EndOfFile.max(0) as u64,
            links: by_handle.nNumberOfLinks,
        })
    }

    fn copy_source(&mut self, dest: &str, limit: u64) -> Result<u64, Refusal> {
        let mut src = self.source.take().ok_or(Refusal::Io)?;
        // Create-new, no sharing while writing, and an explicit descriptor: Administrators and SYSTEM only, so a user never
        // reads (or races) an update that has not passed its signature check.
        let sd = Descriptor::from_sddl(STAGED_FILE_SDDL)
            .map_err(|e| io(&mut self.log, "staged copy descriptor", e))?;
        let attrs = sd.attributes();
        let wide = to_wide(dest);
        // SAFETY: NUL-terminated path; live attributes.
        let h = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_WRITE,
                0,
                &attrs,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(io(
                &mut self.log,
                "create the staged copy",
                handle::last_error(),
            ));
        }
        // SAFETY: a valid owned handle.
        let mut out = unsafe { File::from_raw_handle(h as RawHandle) };
        let mut copied = 0u64;
        let mut buf = vec![0u8; 1 << 20];
        let result = loop {
            let n = match src.read(&mut buf) {
                Ok(0) => break Ok(copied),
                Ok(n) => n,
                Err(e) => break Err(io(&mut self.log, "read the download", e)),
            };
            copied += n as u64;
            if copied > limit {
                break Err(Refusal::TooLarge);
            }
            if let Err(e) = out.write_all(&buf[..n]) {
                break Err(io(&mut self.log, "write the staged copy", e));
            }
        };
        let result = result.and_then(|n| {
            out.sync_all()
                .map(|_| n)
                .map_err(|e| io(&mut self.log, "flush the staged copy", e))
        });
        drop(out);
        drop(src);
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }

    fn hold_copy(&mut self, dest: &str) -> Result<(), Refusal> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ_ONLY)
            .open(dest)
            .map_err(|e| io(&mut self.log, "reopen the staged copy", e))?;
        self.held = Some(f);
        Ok(())
    }

    fn verify(
        &mut self,
        dest: &str,
        signature: &str,
        announced: &Version,
    ) -> Result<String, Refusal> {
        let Some(key) = &self.key else {
            return Err(Refusal::NoKey);
        };
        // The held handle denies every writer and deleter, so reading the copy by its path reads the bytes that will run.
        match verify_file(key, Path::new(dest), signature, announced, None) {
            Ok(()) => trusted_comment(signature).ok_or(Refusal::SignatureInvalid),
            Err(VerifyError::Io(e)) => Err(io(&mut self.log, "verify the staged copy", e)),
            Err(other) => {
                self.log.line(
                    "apply-update",
                    &format!("signature check failed: {}", other.into_update_error()),
                );
                Err(Refusal::SignatureInvalid)
            }
        }
    }

    fn start(&mut self, dest: &str, run: &RunPaths) -> Result<(), Refusal> {
        // CreateProcessW with no handle inheritance: the installer gets neither the caller's console nor any pipe (an
        // inherited pipe would keep the caller waiting), and TEMP/TMP point into the run folder's `tmp\`.
        let mut command = to_wide(&format!("\"{dest}\" {}", INSTALLER_ARGS.join(" ")));
        let app = to_wide(dest);
        let cwd = to_wide(&run.tmp);
        let mut env: Vec<u16> = Vec::new();
        for (key, value) in std::env::vars_os() {
            let name = key.to_string_lossy().to_ascii_uppercase();
            if name == "TEMP" || name == "TMP" {
                continue;
            }
            env.extend(key.encode_wide());
            env.push(u16::from(b'='));
            env.extend(value.encode_wide());
            env.push(0);
        }
        for name in ["TEMP", "TMP"] {
            env.extend(name.encode_utf16());
            env.push(u16::from(b'='));
            env.extend(run.tmp.encode_utf16());
            env.push(0);
        }
        env.push(0);
        // SAFETY: zeroed structures of the right types; every pointer is to a live, NUL-terminated buffer.
        let created = unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(
                app.as_ptr(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_UNICODE_ENVIRONMENT,
                env.as_ptr().cast(),
                cwd.as_ptr(),
                &si,
                &mut pi,
            );
            if ok == 0 {
                Err(handle::last_error())
            } else {
                drop(Owned(pi.hThread));
                Ok(Owned(pi.hProcess))
            }
        };
        match created {
            Ok(process) => {
                self.child = Some(process);
                Ok(())
            }
            Err(code) => {
                self.log
                    .line("apply-update", &format!("start failed (error {code})"));
                Err(Refusal::StartFailed)
            }
        }
    }

    fn handover(&mut self) -> Handover {
        // Give the lock up so that `hold-installing` can take it. The signal stays active (and a second apply-update stays
        // busy) because this process keeps its handle until the installer has its own.
        if let Some(l) = &self.lock
            && self.lock_owned
        {
            // SAFETY: this thread owns the mutex (taken in acquire_mutex).
            unsafe { ReleaseMutex(l.0) };
            self.lock_owned = false;
        }
        let (Some(event), Some(child)) = (&self.handoff, &self.child) else {
            return Handover::InstallerEnded;
        };
        let handles = [event.0, child.0];
        // A wait on two handles, not a poll: it ends when the installer holds the lock or its process has ended.
        // SAFETY: both handles are valid for the length of the call.
        let r = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        if r == WAIT_OBJECT_0 {
            self.signal = None; // the installer's hold-installing has its own handle now
            Handover::InstallerHoldsIt
        } else {
            Handover::InstallerEnded
        }
    }

    fn discard(&mut self, run: &RunPaths) {
        self.held = None;
        self.source = None;
        self.child = None;
        // Rename at once (a program that just ended, or a scanner reading the new file, may still hold something), then
        // delete best-effort; what is left is `trash-` and the next `clean-staging` removes it. No waiting.
        let dir = Path::new(&run.dir);
        let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
            return;
        };
        let trash = parent.join(format!(
            "{}{}",
            crate::staging::TRASH_PREFIX,
            name.to_string_lossy()
        ));
        let target = match std::fs::rename(dir, &trash) {
            Ok(()) => trash,
            Err(e) => {
                self.log.line(
                    "apply-update",
                    &format!("the run folder could not be renamed: {e}"),
                );
                dir.to_path_buf()
            }
        };
        if let Err(e) = std::fs::remove_dir_all(&target) {
            self.log.line(
                "apply-update",
                &format!("the run folder is left for the next clean-up: {e}"),
            );
        }
    }

    fn release(&mut self) {
        self.held = None;
        self.source = None;
        if let Some(l) = &self.lock
            && self.lock_owned
        {
            // SAFETY: this thread owns the mutex.
            unsafe { ReleaseMutex(l.0) };
            self.lock_owned = false;
        }
        // An install that never started (a refusal) or ended early is over: tell anyone waiting on the signal.
        if let Some(signal) = self.signal.take() {
            mutex::set(&signal);
        }
        self.handoff = None;
        self.lock = None;
        self.namespace = None;
    }
}
