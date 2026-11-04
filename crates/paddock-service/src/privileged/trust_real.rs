//! The real [`TrustPort`]: the facts of each path component are read from an object opened for that purpose and never
//! followed through a link (`O_NOFOLLOW` semantics on Unix, `FILE_FLAG_OPEN_REPARSE_POINT` on Windows).
//!
//! Native glue (crates/README.md): it reads the real disk, so it is covered by the VM checks and by the pure rule's tests in
//! [`super::trust`], not by unit tests here. The `ls -lde` parsing is tested in `trust`.
#![allow(unsafe_code)]

use super::files::FileKind;
use super::trust::{Node, Security, Style, TrustPort};
use std::io;

/// The real machine.
#[derive(Debug, Default)]
pub struct RealTrust;

#[cfg(unix)]
impl TrustPort for RealTrust {
    fn style(&self) -> Style {
        Style::Unix
    }

    fn node(&mut self, path: &str) -> io::Result<Option<Node>> {
        use std::os::unix::fs::MetadataExt;
        let meta = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let t = meta.file_type();
        let kind = if t.is_symlink() {
            FileKind::Symlink
        } else if t.is_dir() {
            FileKind::Dir
        } else if t.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
        Ok(Some(Node {
            kind,
            security: Security::Unix {
                owner: meta.uid(),
                mode: meta.mode() & 0o7777,
                write_acl: cfg!(target_os = "macos")
                    && !t.is_symlink()
                    && mac_acl_allows_write(path),
            },
        }))
    }

    fn canonical(&mut self, path: &str) -> io::Result<String> {
        Ok(std::fs::canonicalize(path)?.to_string_lossy().into_owned())
    }

    fn create_dir_all(&mut self, path: &str) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(path)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    fn icacls(&mut self, _args: &[String]) -> io::Result<String> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn remove_tree(&mut self, path: &str) -> io::Result<()> {
        remove_tree_no_follow(path)
    }
}

/// `std::fs::remove_dir_all` removes a link inside the tree (and the root, if it is one) and never follows it, on Unix
/// and on Windows (it opens each entry with `FILE_FLAG_OPEN_REPARSE_POINT`); a root that is itself a link is removed as the
/// link it is.
fn remove_tree_no_follow(path: &str) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            // A directory symlink or junction is removed with remove_dir on Windows, a file symlink with remove_file.
            std::fs::remove_file(path).or_else(|_| std::fs::remove_dir(path))
        }
        Ok(_) => std::fs::remove_dir_all(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(target_os = "macos")]
fn mac_acl_allows_write(path: &str) -> bool {
    // `ls -lde` lists the ACL entries under the line of the folder itself; a failure to run it is treated as "has a write
    // entry" so that the rule refuses rather than guesses.
    match std::process::Command::new("/bin/ls")
        .args(["-lde", "--", path])
        .output()
    {
        Ok(o) if o.status.success() => {
            super::trust::acl_allows_write(&String::from_utf8_lossy(&o.stdout))
        }
        _ => true,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mac_acl_allows_write(_path: &str) -> bool {
    false
}

#[cfg(windows)]
impl TrustPort for RealTrust {
    fn style(&self) -> Style {
        Style::Windows
    }

    fn node(&mut self, path: &str) -> io::Result<Option<Node>> {
        use std::os::windows::fs::OpenOptionsExt;
        const READ_CONTROL: u32 = 0x0002_0000;
        const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
        const SHARE_ALL: u32 = 7;
        const BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        let file = match std::fs::OpenOptions::new()
            .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
            .share_mode(SHARE_ALL)
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        let kind = {
            use std::os::windows::fs::MetadataExt;
            const REPARSE: u32 = 0x400;
            if meta.file_attributes() & REPARSE != 0 {
                FileKind::Symlink
            } else if meta.is_dir() {
                FileKind::Dir
            } else if meta.is_file() {
                FileKind::File
            } else {
                FileKind::Other
            }
        };
        let descriptor = win::descriptor(&file)?;
        Ok(Some(Node {
            kind,
            security: Security::Windows(descriptor),
        }))
    }

    fn canonical(&mut self, path: &str) -> io::Result<String> {
        Ok(std::fs::canonicalize(path)?.to_string_lossy().into_owned())
    }

    fn create_dir_all(&mut self, path: &str) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn chmod(&mut self, _path: &str, _mode: u32) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn remove_tree(&mut self, path: &str) -> io::Result<()> {
        remove_tree_no_follow(path)
    }

    fn icacls(&mut self, args: &[String]) -> io::Result<String> {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        use crate::port::System as _;
        let out = crate::system::RealSystem::new()
            .run(super::windows_acl::ICACLS, &refs)
            .map_err(|e| io::Error::other(format!("icacls {e}")))?;
        if out.success() {
            Ok(out.stdout)
        } else {
            Err(io::Error::other(format!(
                "icacls exited with {:?}: {}",
                out.code,
                out.stderr.lines().next().unwrap_or("")
            )))
        }
    }
}

#[cfg(windows)]
mod win {
    use super::super::trust::{Ace, Descriptor};
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetAce, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID,
    };

    const ACCESS_ALLOWED: u8 = 0;
    const ACCESS_DENIED: u8 = 1;
    const ACCESS_ALLOWED_CALLBACK: u8 = 9;
    const ACCESS_DENIED_CALLBACK: u8 = 10;
    const INHERIT_ONLY: u8 = 0x08;

    unsafe fn sid_text(sid: PSID) -> io::Result<String> {
        let mut text = windows::core::PWSTR::null();
        // SAFETY: `sid` points into the descriptor that outlives this call.
        unsafe { ConvertSidToStringSidW(sid, &mut text) }
            .map_err(|e| io::Error::other(e.to_string()))?;
        // SAFETY: `text` is a NUL-terminated string the call allocated; it is freed right after the copy.
        let s = unsafe { text.to_string() }.map_err(|e| io::Error::other(e.to_string()));
        // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
        s
    }

    /// The owner and DACL of the open object.
    pub fn descriptor(file: &std::fs::File) -> io::Result<Descriptor> {
        let handle = HANDLE(file.as_raw_handle());
        let (mut owner, mut dacl) = (PSID::default(), std::ptr::null_mut::<ACL>());
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `handle` is open for READ_CONTROL for the whole call; the out-pointers are valid; the returned pointers
        // point into `sd`, which is freed only after they are read.
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                Some(&mut dacl),
                None,
                Some(&mut sd),
            )
        };
        if status.0 != 0 {
            return Err(io::Error::from_raw_os_error(status.0 as i32));
        }
        let result = (|| {
            // SAFETY: `owner` points into `sd`, alive here.
            let owner_text = unsafe { sid_text(owner) }?;
            let aces = if dacl.is_null() {
                None
            } else {
                // SAFETY: `dacl` points to a valid ACL inside `sd`.
                let count = unsafe { (*dacl).AceCount };
                let mut list = Vec::with_capacity(usize::from(count));
                for i in 0..u32::from(count) {
                    let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
                    // SAFETY: `i` is below the ACL's entry count.
                    unsafe { GetAce(dacl, i, &mut ace) }
                        .map_err(|e| io::Error::other(e.to_string()))?;
                    // SAFETY: an ACE starts with type, flags and size; allowed and denied entries (and their callback forms)
                    // continue with the 32-bit mask and the SID.
                    let (kind, flags) =
                        unsafe { (*(ace as *const u8), *(ace as *const u8).add(1)) };
                    let allow = match kind {
                        ACCESS_ALLOWED | ACCESS_ALLOWED_CALLBACK => true,
                        ACCESS_DENIED | ACCESS_DENIED_CALLBACK => false,
                        other => {
                            return Err(io::Error::other(format!(
                                "an access entry of type {other} is not understood"
                            )));
                        }
                    };
                    // SAFETY: see above; the mask is at byte 4, the SID at byte 8.
                    let (mask, sid) = unsafe {
                        (
                            *((ace as *const u8).add(4) as *const u32),
                            PSID((ace as *mut u8).add(8).cast()),
                        )
                    };
                    // SAFETY: the SID lies inside the ACE.
                    list.push(Ace {
                        sid: unsafe { sid_text(sid) }?,
                        allow,
                        mask,
                        inherit_only: flags & INHERIT_ONLY != 0,
                    });
                }
                Some(list)
            };
            Ok(Descriptor {
                owner: owner_text,
                dacl: aces,
            })
        })();
        // SAFETY: allocated by GetSecurityInfo with LocalAlloc.
        unsafe { LocalFree(Some(HLOCAL(sd.0))) };
        result
    }
}

/// A program held open for a privileged run: opened without following a link, shown to be a regular file, and kept open
/// until it is dropped (after the process started), so the file that was checked is the file that runs.
///
/// - **Windows:** opened with `FILE_SHARE_READ` only (nobody can write to, rename or delete it while held) and with
///   `FILE_FLAG_OPEN_REPARSE_POINT`; [`final_path`](Self::final_path) is what `GetFinalPathNameByHandle` returns for this
///   handle, so a junction in the requested path shows as a different path.
/// - **Unix:** opened with `O_NOFOLLOW` after resolving the path; on Linux the program runs through
///   `/proc/self/fd/<fd>` of this descriptor (the kernel's `execve` of that path runs the open file); elsewhere it runs by the
///   resolved path and the protected folder chain is what prevents a swap.
#[derive(Debug)]
pub struct HeldImage {
    _file: std::fs::File,
    final_path: String,
    spawn_path: String,
}

impl HeldImage {
    /// Holds `requested`.
    ///
    /// # Errors
    /// The path is missing, is a link or reparse point, or is not a regular file.
    pub fn open(requested: &str) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            const GENERIC_READ: u32 = 0x8000_0000;
            const FILE_SHARE_READ: u32 = 1;
            const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            const REPARSE: u32 = 0x400;
            let file = std::fs::OpenOptions::new()
                .access_mode(GENERIC_READ)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(OPEN_REPARSE_POINT)
                .open(requested)?;
            let meta = file.metadata()?;
            if meta.file_attributes() & REPARSE != 0 || !meta.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            let final_path = final_path_of(&file)?;
            Ok(HeldImage {
                spawn_path: final_path.clone(),
                final_path,
                _file: file,
            })
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
            let resolved = std::fs::canonicalize(requested)?
                .to_string_lossy()
                .into_owned();
            let file = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&resolved)?;
            let meta = file.metadata()?;
            if !meta.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            // The facts of the open descriptor, not of the path.
            if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "not owned by root or writable by others",
                ));
            }
            #[cfg(target_os = "linux")]
            let spawn_path = {
                use std::os::fd::AsRawFd;
                let via_fd = format!("/proc/self/fd/{}", file.as_raw_fd());
                if std::path::Path::new(&via_fd).exists() {
                    via_fd
                } else {
                    resolved.clone()
                }
            };
            #[cfg(not(target_os = "linux"))]
            let spawn_path = resolved.clone();
            Ok(HeldImage {
                _file: file,
                final_path: resolved,
                spawn_path,
            })
        }
    }

    /// The path the open object really has (links resolved).
    pub fn final_path(&self) -> &str {
        &self.final_path
    }

    /// What to run: the final path, or the descriptor's `/proc` path on Linux.
    pub fn spawn_path(&self) -> &str {
        &self.spawn_path
    }
}

#[cfg(windows)]
fn final_path_of(file: &std::fs::File) -> io::Result<String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    // FILE_NAME_NORMALIZED and VOLUME_NAME_DOS are both 0: the normalized path with the drive letter.
    use windows::Win32::Storage::FileSystem::{FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW};
    let handle = HANDLE(file.as_raw_handle());
    let mut buf = vec![0u16; 1024];
    loop {
        // SAFETY: `handle` is open for the whole call and `buf` is a valid writable buffer.
        let n =
            unsafe { GetFinalPathNameByHandleW(handle, &mut buf, FILE_NAME_NORMALIZED) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buf.len() {
            return Ok(String::from_utf16_lossy(&buf[..n]));
        }
        buf.resize(n + 1, 0);
    }
}
