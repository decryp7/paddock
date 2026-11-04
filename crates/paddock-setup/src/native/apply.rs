//! `apply-update` on Windows (native glue): the calls behind [`crate::apply::UpdatePort`]. No decisions here.

use super::facts::RealDirs;
use super::folders;
use super::handle::{self, Descriptor, Owned};
use super::wide::to_wide;
use crate::apply::{INSTALLER_ARGS, Layout, MAX_SIGNATURE_BYTES, Refusal, SourceFacts, UpdatePort};
use crate::handoff::{MUTEX_SDDL, mutex_name};
use crate::log::Sink;
use paddock_updater::signature::{UpdateKey, VerifyError, verify_file};
use semver::Version;
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::{INVALID_HANDLE_VALUE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_STANDARD_INFO, FILE_TYPE_DISK,
    FileBasicInfo, FileStandardInfo, GetDriveTypeW, GetFileInformationByHandleEx, GetFileType,
    GetFileVersionInfoSizeW, GetFileVersionInfoW, OPEN_EXISTING, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows_sys::Win32::Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_DOS};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};

const FILE_SHARE_READ_ONLY: u32 = FILE_SHARE_READ;
/// `GetDriveTypeW` answer for a local fixed disk.
const DRIVE_FIXED: u32 = 3;

/// The machine behind `apply-update`.
pub struct Real<'a> {
    pub layout: Layout,
    pub main: String,
    pub identifier: String,
    pub key: Option<UpdateKey>,
    pub log: super::Shared<'a>,
    source: Option<File>,
    held: Option<File>,
    mutex: Option<Owned>,
}

impl<'a> Real<'a> {
    /// A port for the app installed in `layout.instdir`.
    pub fn new(
        layout: Layout,
        main: String,
        identifier: String,
        key: Option<UpdateKey>,
        log: super::Shared<'a>,
    ) -> Self {
        Real {
            layout,
            main,
            identifier,
            key,
            log,
            source: None,
            held: None,
            mutex: None,
        }
    }
}

fn io(log: &mut dyn crate::log::Sink, what: &str, e: impl std::fmt::Display) -> Refusal {
    log.line("apply-update", &format!("{what} failed: {e}"));
    Refusal::Io
}

impl UpdatePort for Real<'_> {
    fn has_key(&self) -> bool {
        self.key.is_some()
    }

    fn prepare_folders(&mut self) -> Result<(), Refusal> {
        // The running program's folder is $INSTDIR (the caller checked); it must pass the same rule as the installer's.
        if crate::checkdir::check(&RealDirs, &self.layout.instdir).is_err() {
            return Err(Refusal::InstallDir);
        }
        for dir in [&self.layout.updates, &self.layout.tmp] {
            folders::ensure(dir).map_err(|e| {
                self.log
                    .line("apply-update", &format!("folder not usable: {e}"));
                Refusal::InstallDir
            })?;
        }
        Ok(())
    }

    fn acquire_mutex(&mut self) -> Result<(), Refusal> {
        let sd = Descriptor::from_sddl(MUTEX_SDDL)
            .map_err(|e| io(&mut self.log, "mutex descriptor", e))?;
        let attrs = sd.attributes();
        let name = to_wide(&mutex_name(&self.identifier));
        // SAFETY: NUL-terminated name; live attributes.
        let h = unsafe { CreateMutexW(&attrs, 0, name.as_ptr()) };
        let err = handle::last_error();
        if h.is_null() {
            return Err(io(&mut self.log, "create mutex", err));
        }
        let owned = Owned(h);
        // The name may exist because the app holds a handle to wait on it; only an owner counts as "an installer runs".
        // SAFETY: valid mutex handle.
        let wait = unsafe { WaitForSingleObject(owned.0, 0) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            return Err(Refusal::Busy);
        }
        self.mutex = Some(owned);
        Ok(())
    }

    fn clean_previous(&mut self) {
        let entries: Vec<crate::staging::Entry> = std::fs::read_dir(&self.layout.updates)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| crate::staging::Entry {
                        name: e.file_name().to_string_lossy().into_owned(),
                        is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                    })
                    .collect()
            })
            .unwrap_or_default();
        for name in crate::staging::removals(&entries) {
            let p = Path::new(&self.layout.updates).join(&name);
            let is_dir = entries.iter().any(|e| e.name == name && e.is_dir);
            let r = if is_dir {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
            if let Err(e) = r {
                self.log.line(
                    "apply-update",
                    &format!("old staging entry not removed: {e}"),
                );
            }
        }
        // `tmp` is part of the layout and must exist for the installer's TEMP.
        let _ = folders::ensure(&self.layout.tmp);
    }

    fn installed_version(&mut self) -> Result<Version, Refusal> {
        let exe = PathBuf::from(&self.layout.instdir).join(&self.main);
        let wide = to_wide(&exe.to_string_lossy());
        let mut dummy = 0u32;
        // SAFETY: NUL-terminated path.
        let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut dummy) };
        if size == 0 || size > 1 << 20 {
            return Err(io(&mut self.log, "installed version", handle::last_error()));
        }
        let mut data = vec![0u8; size as usize];
        // SAFETY: `data` holds `size` bytes.
        if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
            return Err(io(&mut self.log, "installed version", handle::last_error()));
        }
        let root = to_wide("\\");
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        // SAFETY: `data` is a version block; `ptr` points into it afterwards.
        if unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut ptr, &mut len) } == 0
            || ptr.is_null()
            || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return Err(io(
                &mut self.log,
                "installed version",
                "no fixed version info",
            ));
        }
        // SAFETY: checked length; read unaligned.
        let fixed = unsafe { std::ptr::read_unaligned(ptr.cast::<VS_FIXEDFILEINFO>()) };
        Ok(Version::new(
            (fixed.dwFileVersionMS >> 16) as u64,
            (fixed.dwFileVersionMS & 0xFFFF) as u64,
            (fixed.dwFileVersionLS >> 16) as u64,
        ))
    }

    fn read_signature(&mut self, path: &str) -> Result<String, Refusal> {
        let f = File::open(path).map_err(|e| io(&mut self.log, "read signature", e))?;
        let mut text = String::new();
        f.take(MAX_SIGNATURE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|e| io(&mut self.log, "read signature", e))?;
        if text.len() as u64 > MAX_SIGNATURE_BYTES {
            return Err(Refusal::SignatureInvalid);
        }
        Ok(text)
    }

    fn open_source(&mut self, path: &str) -> Result<SourceFacts, Refusal> {
        let wide = to_wide(path);
        // SAFETY: NUL-terminated path. Identification level: a named-pipe server behind the path can identify the elevated
        // caller, never impersonate it. Reparse points are opened, not followed, and then refused.
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
        // SAFETY: the buffers are the stated structures.
        let (b_ok, s_ok) = unsafe {
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
            )
        };
        if b_ok == 0 || s_ok == 0 {
            return Err(io(
                &mut self.log,
                "query the download",
                handle::last_error(),
            ));
        }
        // SAFETY: both calls succeeded.
        let (basic, std_info) = unsafe { (basic.assume_init(), std_info.assume_init()) };
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
        })
    }

    fn copy_source(&mut self, dest: &str, limit: u64) -> Result<u64, Refusal> {
        let mut src = self.source.take().ok_or(Refusal::Io)?;
        // Create-new, no sharing while writing; the folder's descriptor is inherited (administrators write, users read).
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(dest)
            .map_err(|e| io(&mut self.log, "create the staged copy", e))?;
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

    fn verify(&mut self, dest: &str, signature: &str, announced: &Version) -> Result<(), Refusal> {
        let Some(key) = &self.key else {
            return Err(Refusal::NoKey);
        };
        // The held handle denies every writer and deleter, so reading the copy by its path reads the bytes that will run.
        match verify_file(key, Path::new(dest), signature, announced, None) {
            Ok(()) => Ok(()),
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

    fn start(&mut self, dest: &str) -> Result<(), Refusal> {
        let tmp = &self.layout.tmp;
        let child = std::process::Command::new(dest)
            .args(INSTALLER_ARGS)
            .env("TEMP", tmp)
            .env("TMP", tmp)
            .current_dir(tmp)
            .spawn();
        match child {
            Ok(_) => Ok(()),
            Err(e) => {
                self.log.line("apply-update", &format!("start failed: {e}"));
                Err(Refusal::StartFailed)
            }
        }
    }

    fn release(&mut self) {
        self.held = None;
        self.source = None;
        if let Some(m) = self.mutex.take() {
            // SAFETY: this thread owns the mutex (taken in acquire_mutex).
            unsafe { ReleaseMutex(m.0) };
        }
    }
}

/// `C:\` of `\\?\C:\dir\file`, `None` for anything that is not a drive-letter path.
fn volume_root(final_path: &str) -> Option<String> {
    let p = final_path.strip_prefix(r"\\?\").unwrap_or(final_path);
    let mut chars = p.chars();
    let (d, colon, slash) = (chars.next()?, chars.next()?, chars.next()?);
    (d.is_ascii_alphabetic() && colon == ':' && slash == '\\').then(|| format!("{d}:\\"))
}
