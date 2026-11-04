//! The machine behind the ports of `guard`, `check-dir` and `clean-run` (native glue: reads and acts, decides nothing).

use super::handle::{self, Owned};
use super::registry::{self, Root, View};
use super::wide::to_wide;
use crate::checkdir::{DirFacts, DirInspect, UpdatesInspect, components};
use crate::guard::{InstallFacts, UninstallEntry, expand_local_app_data};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, HANDLE};
use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO, FileBasicInfo, GetFileAttributesW, GetFileInformationByHandleEx,
    INVALID_FILE_ATTRIBUTES,
};
use windows_sys::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

/// The shell process of the installer's session, opened for its token.
fn shell_process_token(access: u32) -> Option<Owned> {
    // SAFETY: plain calls with valid out pointers.
    unsafe {
        let hwnd = GetShellWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let process = Owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid));
        if process.0.is_null() {
            return None;
        }
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(process.0, access, &mut token) == 0 {
            return None;
        }
        Some(Owned(token))
    }
}

/// The desktop user's SID: the owner of the installer session's shell process.
pub fn desktop_user() -> Option<String> {
    handle::token_user_sid(&shell_process_token(TOKEN_QUERY)?)
}

/// A duplicate of the shell process's token (to start a program as the desktop user), `None` when it cannot be had.
pub fn desktop_token() -> Option<Owned> {
    use windows_sys::Win32::Security::{DuplicateTokenEx, SecurityImpersonation, TOKEN_ALL_ACCESS, TokenPrimary};
    let src = shell_process_token(TOKEN_DUPLICATE | TOKEN_QUERY)?;
    let mut dup: HANDLE = std::ptr::null_mut();
    // SAFETY: a valid source token and out pointer.
    let ok = unsafe { DuplicateTokenEx(src.0, TOKEN_ALL_ACCESS, std::ptr::null(), SecurityImpersonation, TokenPrimary, &mut dup) };
    (ok != 0).then(|| Owned(dup))
}

/// Registry and file facts of this machine.
pub struct Machine {
    pub product: String,
}

impl InstallFacts for Machine {
    fn machine_key_exists(&self) -> bool {
        registry::exists(Root::Machine, &format!(r"{UNINSTALL}\{}", self.product), View::Wide)
    }
    fn installer_account(&self) -> Option<String> {
        handle::current_account()
    }
    fn desktop_user(&self) -> Option<String> {
        desktop_user()
    }
    fn hku_subkeys(&self) -> Vec<String> {
        registry::open_read(Root::Users, "", View::Wide).map(|k| k.subkeys()).unwrap_or_default()
    }
    fn uninstall_key(&self, sid: &str) -> Option<UninstallEntry> {
        let key = registry::open_read(Root::Users, &format!(r"{sid}\{UNINSTALL}\{}", self.product), View::Wide)?;
        Some(UninstallEntry { uninstall_string: key.string("UninstallString") })
    }
    fn local_app_data(&self, sid: &str) -> Option<String> {
        let shell = registry::open_read(Root::Users, &format!(r"{sid}\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders"), View::Wide)?;
        let raw = shell.string("Local AppData")?;
        let profile = registry::open_read(Root::Machine, &format!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList\{sid}"), View::Wide)?
            .string("ProfileImagePath")?;
        expand_local_app_data(&raw, &profile)
    }
    fn file_exists(&self, path: &str) -> bool {
        let wide = to_wide(path);
        // SAFETY: NUL-terminated path.
        unsafe { GetFileAttributesW(wide.as_ptr()) != INVALID_FILE_ATTRIBUTES }
    }
    fn account_name(&self, sid: &str) -> Option<String> {
        handle::account_name(sid)
    }
}

/// The `Program Files` folders of this machine, from the registry (administrators write it; the environment is not trusted).
fn program_files_roots() -> Vec<String> {
    let Some(key) = registry::open_read(Root::Machine, r"SOFTWARE\Microsoft\Windows\CurrentVersion", View::Wide) else {
        return Vec::new();
    };
    ["ProgramFilesDir", "ProgramFilesDir (x86)"]
        .into_iter()
        .filter_map(|n| key.string(n))
        .filter_map(|p| handle::open_for_query(&p).ok().and_then(|h| handle::final_path(&h)))
        .collect()
}

fn strip_prefix(p: &str) -> String {
    p.strip_prefix(r"\\?\").unwrap_or(p).to_string()
}

fn full_path(path: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetFullPathNameW;
    let wide = to_wide(path);
    let mut buf = vec![0u16; 1024];
    // SAFETY: valid buffers.
    let n = unsafe { GetFullPathNameW(wide.as_ptr(), buf.len() as u32, buf.as_mut_ptr(), std::ptr::null_mut()) } as usize;
    (n > 0 && n < buf.len()).then(|| super::wide::from_wide(&buf[..n]))
}

fn attributes(h: &Owned) -> Option<u32> {
    let mut info = std::mem::MaybeUninit::<FILE_BASIC_INFO>::zeroed();
    // SAFETY: the buffer is a FILE_BASIC_INFO of the stated size.
    let ok = unsafe { GetFileInformationByHandleEx(h.0, FileBasicInfo, info.as_mut_ptr().cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) };
    // SAFETY: initialised by the call when it succeeded.
    (ok != 0).then(|| unsafe { info.assume_init() }.FileAttributes)
}

/// The real folder rules of `check-dir`.
pub struct RealDirs;

impl DirFacts for RealDirs {
    fn program_files(&self) -> Vec<String> {
        program_files_roots().iter().map(|p| strip_prefix(p)).collect()
    }

    fn inspect(&self, dir: &str) -> Result<DirInspect, String> {
        let requested = full_path(dir).ok_or_else(|| "the folder name is not a valid path".to_string())?;
        match handle::open_for_query(&requested) {
            Ok(h) => {
                let canonical = strip_prefix(&handle::final_path(&h).ok_or("the final path could not be read")?);
                let attrs = attributes(&h).ok_or("the attributes could not be read")?;
                let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !crate::samefolder::same(Some(&requested), Some(&canonical));
                Ok(DirInspect { canonical, exists: true, reparse_component: reparse, owner_sid: handle::owner_sid(&h) })
            }
            Err(e) if e == ERROR_FILE_NOT_FOUND || e == ERROR_PATH_NOT_FOUND => {
                let parts = components(&requested);
                let (leaf, parent_parts) = parts.split_last().ok_or("the folder name is empty")?;
                let parent = parent_parts.join("\\") + "\\";
                let h = handle::open_for_query(&parent).map_err(|e| format!("the parent folder cannot be opened (error {e})"))?;
                let parent_final = strip_prefix(&handle::final_path(&h).ok_or("the final path could not be read")?);
                let attrs = attributes(&h).ok_or("the attributes could not be read")?;
                let reparse = attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !crate::samefolder::same(Some(&parent), Some(&parent_final));
                Ok(DirInspect { canonical: format!("{}\\{}", parent_final.trim_end_matches('\\'), leaf), exists: false, reparse_component: reparse, owner_sid: None })
            }
            Err(e) => Err(format!("the folder cannot be opened (error {e})")),
        }
    }
}

/// What is at `path` for the `updates\` rule.
pub fn inspect_updates(path: &str) -> UpdatesInspect {
    match handle::open_for_query(path) {
        Ok(h) => {
            let attrs = attributes(&h).unwrap_or(0);
            UpdatesInspect {
                exists: true,
                is_dir: attrs & FILE_ATTRIBUTE_DIRECTORY != 0,
                reparse: attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0,
                owner_sid: handle::owner_sid(&h),
            }
        }
        Err(_) => UpdatesInspect { exists: false, is_dir: false, reparse: false, owner_sid: None },
    }
}
