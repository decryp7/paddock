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

    fn walk(&mut self, path: &str, limit: usize) -> io::Result<Vec<(String, Node)>> {
        walk_no_follow(self, path, limit)
    }

    fn create_dir(&mut self, path: &str) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        // One level, create-new, exactly 0755 (the umask cannot widen it; narrowing is undone by `lock_dir`).
        std::fs::DirBuilder::new().mode(0o755).create(path)
    }

    fn lock_dir(&mut self, path: &str) -> io::Result<()> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // By descriptor of a folder opened without following a link, so the mode lands on the folder that was checked.
        let dir = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(path)?;
        dir.set_permissions(std::fs::Permissions::from_mode(0o755))
    }

    fn rename_aside(&mut self, path: &str) -> io::Result<String> {
        rename_aside_free(path)
    }

    fn remove_tree(&mut self, path: &str) -> io::Result<()> {
        remove_tree_no_follow(path)
    }
}

/// Every entry below `root` (a link is listed and not entered), at most `limit`.
fn walk_no_follow(
    port: &mut dyn TrustPort,
    root: &str,
    limit: usize,
) -> io::Result<Vec<(String, Node)>> {
    let sep = if port.style() == Style::Windows {
        '\\'
    } else {
        '/'
    };
    let mut out = Vec::new();
    let mut todo = vec![root.to_string()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            let path = format!("{}{sep}{name}", dir.trim_end_matches(sep));
            let Some(node) = port.node(&path)? else {
                continue;
            };
            if node.kind == FileKind::Dir {
                todo.push(path.clone());
            }
            out.push((path, node));
            if out.len() > limit {
                return Err(io::Error::other(format!("more than {limit} entries")));
            }
        }
    }
    Ok(out)
}

/// Renames `path` next to itself to a free name; the entry is never opened or followed.
fn rename_aside_free(path: &str) -> io::Result<String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for n in 0..100 {
        let aside = format!("{path}.untrusted-{secs}-{n}");
        if std::fs::symlink_metadata(&aside).is_ok() {
            continue;
        }
        std::fs::rename(path, &aside)?;
        return Ok(aside);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name to move the folder aside",
    ))
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

    fn walk(&mut self, path: &str, limit: usize) -> io::Result<Vec<(String, Node)>> {
        walk_no_follow(self, path, limit)
    }

    fn create_dir(&mut self, path: &str) -> io::Result<()> {
        win::create_dir_protected(path)
    }

    fn lock_dir(&mut self, path: &str) -> io::Result<()> {
        win::lock_dir(path)
    }

    fn rename_aside(&mut self, path: &str) -> io::Result<String> {
        rename_aside_free(path)
    }

    fn remove_tree(&mut self, path: &str) -> io::Result<()> {
        remove_tree_no_follow(path)
    }
}

#[cfg(windows)]
mod win {
    use super::super::trust::{Ace, Descriptor, is_trusted_sid};
    use super::super::windows_acl::PROTECTED_FOLDER_SDDL;
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
    };
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, GetAce, GetSecurityDescriptorDacl,
        GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    };
    use windows::Win32::Storage::FileSystem::CreateDirectoryW;
    use windows::core::{BOOL, PCWSTR};

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

    struct Sd(PSECURITY_DESCRIPTOR);

    impl Drop for Sd {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW with LocalAlloc.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }

    fn sd_from_sddl(sddl: &str) -> io::Result<Sd> {
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `sd` is a valid out-pointer.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                SDDL_REVISION_1,
                &mut sd,
                None,
            )
        }
        .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Sd(sd))
    }

    /// Creates one folder, create-new, with the protected descriptor given at creation (the DACL of
    /// [`PROTECTED_FOLDER_SDDL`]; the owner is whoever the creator's token makes it and `lock_dir` then sets Administrators).
    pub fn create_dir_protected(path: &str) -> io::Result<()> {
        // The DACL part only: a creator may not be allowed to name an owner.
        let sd = sd_from_sddl(&PROTECTED_FOLDER_SDDL[4..])?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0.0,
            bInheritHandle: false.into(),
        };
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated; `attributes` and the descriptor it points to outlive the call.
        unsafe { CreateDirectoryW(PCWSTR(wide.as_ptr()), Some(&attributes)) }
            .map_err(|e| io::Error::from_raw_os_error(e.code().0 & 0xffff))
    }

    /// Applies the protected DACL and, when the owner is not SYSTEM or Administrators, Administrators as owner, to the folder
    /// by a handle opened without following a link, after checking that it is a real folder.
    pub fn lock_dir(path: &str) -> io::Result<()> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        const READ_CONTROL: u32 = 0x0002_0000;
        const WRITE_DAC: u32 = 0x0004_0000;
        const WRITE_OWNER: u32 = 0x0008_0000;
        const FILE_READ_ATTRIBUTES: u32 = 0x0000_0080;
        const BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const REPARSE: u32 = 0x400;
        let file = std::fs::OpenOptions::new()
            .access_mode(READ_CONTROL | WRITE_DAC | WRITE_OWNER | FILE_READ_ATTRIBUTES)
            .share_mode(7)
            .custom_flags(BACKUP_SEMANTICS | OPEN_REPARSE_POINT)
            .open(path)?;
        let meta = file.metadata()?;
        if meta.file_attributes() & REPARSE != 0 || !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a real folder (a link, or not a folder)",
            ));
        }
        let current = descriptor(&file)?;
        let sd = sd_from_sddl(PROTECTED_FOLDER_SDDL)?;
        let (mut owner, mut dacl) = (PSID::default(), std::ptr::null_mut::<ACL>());
        let (mut present, mut defaulted) = (BOOL(0), BOOL(0));
        // SAFETY: `sd` is a valid descriptor; the out-pointers are valid and point into `sd`.
        unsafe {
            GetSecurityDescriptorOwner(sd.0, &mut owner, &mut defaulted)
                .map_err(|e| io::Error::other(e.to_string()))?;
            GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted)
                .map_err(|e| io::Error::other(e.to_string()))?;
        }
        let mut what = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
        let set_owner = !is_trusted_sid(&current.owner);
        if set_owner {
            what |= OWNER_SECURITY_INFORMATION;
        }
        // SAFETY: the handle is open with WRITE_DAC and WRITE_OWNER for the whole call; owner and dacl point into `sd`.
        let status = unsafe {
            SetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_FILE_OBJECT,
                what,
                set_owner.then_some(owner),
                None,
                Some(dacl),
                None,
            )
        };
        if status.0 != 0 {
            return Err(io::Error::from_raw_os_error(status.0 as i32));
        }
        Ok(())
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
    // The size is asked for at most twice (the second call has the size the first one reported); a path longer than the
    // longest the system allows is refused.
    for _ in 0..3 {
        // SAFETY: `handle` is open for the whole call and `buf` is a valid writable buffer.
        let n =
            unsafe { GetFinalPathNameByHandleW(handle, &mut buf, FILE_NAME_NORMALIZED) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buf.len() {
            return Ok(String::from_utf16_lossy(&buf[..n]));
        }
        if n > 32_768 {
            break;
        }
        buf.resize(n + 1, 0);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "the final path is longer than Windows allows",
    ))
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn text(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_symlink_is_reported_as_a_link_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        let mut port = RealTrust;
        assert_eq!(
            port.node(&text(&link)).unwrap().unwrap().kind,
            FileKind::Symlink
        );
        assert_eq!(
            port.node(&text(&target)).unwrap().unwrap().kind,
            FileKind::Dir
        );
        assert!(
            port.node(&text(&dir.path().join("none")))
                .unwrap()
                .is_none()
        );
        // The rule refuses a chain through it.
        let through = format!("{}/file", text(&link));
        std::fs::write(target.join("file"), b"x").unwrap();
        let e = super::super::trust::verify_chain(&mut port, &through, FileKind::File, None, false)
            .unwrap_err();
        assert!(
            matches!(
                e.reason,
                super::super::trust::Reason::Link | super::super::trust::Reason::UntrustedOwner(_)
            ),
            "{e}"
        );
    }

    #[test]
    fn remove_tree_removes_links_and_never_what_they_point_at() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::write(tree.join("sub/file"), b"x").unwrap();
        symlink(&outside, tree.join("sub/link")).unwrap();
        symlink(outside.join("keep.txt"), tree.join("filelink")).unwrap();
        let mut port = RealTrust;
        port.remove_tree(&text(&tree)).unwrap();
        assert!(!tree.exists());
        assert_eq!(std::fs::read(outside.join("keep.txt")).unwrap(), b"keep");
        // A root that is itself a link goes as the link it is.
        let root_link = dir.path().join("root-link");
        symlink(&outside, &root_link).unwrap();
        port.remove_tree(&text(&root_link)).unwrap();
        assert!(std::fs::symlink_metadata(&root_link).is_err());
        assert!(outside.join("keep.txt").exists());
        port.remove_tree(&text(&dir.path().join("missing")))
            .unwrap();
    }

    #[test]
    fn walk_lists_links_without_entering_them_and_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("deep")).unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("a")).unwrap();
        std::fs::write(tree.join("a/f"), b"x").unwrap();
        symlink(&outside, tree.join("a/link")).unwrap();
        let mut port = RealTrust;
        let walked = port.walk(&text(&tree), 100).unwrap();
        let names: Vec<String> = walked
            .iter()
            .map(|(p, _)| p.trim_start_matches(&text(&tree)).to_string())
            .collect();
        assert!(
            names.contains(&"/a".to_string())
                && names.contains(&"/a/f".to_string())
                && names.contains(&"/a/link".to_string()),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("deep")),
            "a link is not entered: {names:?}"
        );
        assert!(port.walk(&text(&tree), 1).is_err(), "bounded");
    }

    #[test]
    fn create_dir_makes_one_level_exactly_0755_and_never_over_something() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut port = RealTrust;
        let made = dir.path().join("made");
        port.create_dir(&text(&made)).unwrap();
        assert_eq!(
            std::fs::metadata(&made).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        assert_eq!(
            port.create_dir(&text(&made)).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(
            port.create_dir(&text(&dir.path().join("a/b"))).is_err(),
            "the parent must exist"
        );
        std::fs::set_permissions(&made, std::fs::Permissions::from_mode(0o777)).unwrap();
        port.lock_dir(&text(&made)).unwrap();
        assert_eq!(
            std::fs::metadata(&made).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        let link = dir.path().join("link");
        symlink(&made, &link).unwrap();
        assert!(port.lock_dir(&text(&link)).is_err(), "a link is not locked");
    }

    #[test]
    fn rename_aside_moves_the_entry_and_not_what_a_link_points_at() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let link = dir.path().join("Dyno");
        symlink(&outside, &link).unwrap();
        let mut port = RealTrust;
        let aside = port.rename_aside(&text(&link)).unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(
            std::fs::symlink_metadata(&aside)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(outside.exists());
    }

    #[test]
    fn a_held_image_must_be_a_regular_file_owned_by_root() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prog");
        std::fs::write(&file, b"\x7fELFxxxx").unwrap();
        // Owned by the user running the test, so not trusted (unless the test runs as root).
        let r = HeldImage::open(&text(&file));
        if unsafe { libc::getuid() } != 0 {
            assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        }
        assert!(
            HeldImage::open(&text(dir.path())).is_err(),
            "a folder is not a program"
        );
        assert!(HeldImage::open(&text(&dir.path().join("missing"))).is_err());
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    //! Run on a Windows runner as an ordinary user (no administrator needed), and on the Windows test VM.
    use super::super::trust::{Reason, verify_chain};
    use super::*;
    use std::process::Command;

    fn text(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn icacls(args: &[&str]) {
        let out = Command::new(super::super::windows_acl::system_tool("icacls"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "icacls {args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
    }

    #[test]
    fn a_folder_a_user_made_under_a_user_writable_place_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let made = dir.path().join("Helper");
        std::fs::create_dir(&made).unwrap();
        let mut port = RealTrust;
        let e = verify_chain(
            &mut port,
            &text(&made),
            FileKind::Dir,
            Some(&text(&made)),
            false,
        )
        .unwrap_err();
        assert!(
            matches!(e.reason, Reason::UntrustedOwner(_) | Reason::Grants { .. }),
            "{e}"
        );
    }

    #[test]
    fn the_descriptor_read_back_after_icacls_has_the_sids_and_masks_that_were_set() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("locked");
        std::fs::create_dir(&folder).unwrap();
        let path = text(&folder);
        icacls(&[
            &path,
            "/inheritance:r",
            "/grant:r",
            "*S-1-5-18:(OI)(CI)F",
            "/grant:r",
            "*S-1-5-32-544:(OI)(CI)F",
            "/grant:r",
            "*S-1-5-32-545:(OI)(CI)RX",
        ]);
        let node = RealTrust.node(&path).unwrap().unwrap();
        assert_eq!(node.kind, FileKind::Dir);
        let Security::Windows(d) = node.security else {
            panic!("a Windows descriptor")
        };
        let aces = d.dacl.expect("a DACL");
        let find = |sid: &str| {
            aces.iter()
                .find(|a| a.sid == sid)
                .unwrap_or_else(|| panic!("{sid} in {aces:?}"))
        };
        assert_eq!(aces.len(), 3, "{aces:?}");
        assert_eq!(find("S-1-5-18").mask, 0x001f_01ff);
        assert_eq!(find("S-1-5-32-544").mask, 0x001f_01ff);
        assert_eq!(find("S-1-5-32-545").mask, 0x0012_00a9);
        assert!(aces.iter().all(|a| a.allow && !a.inherit_only));
    }

    #[test]
    fn create_dir_gives_the_protected_descriptor_at_creation_and_never_over_something() {
        let dir = tempfile::tempdir().unwrap();
        let made = text(&dir.path().join("made"));
        let mut port = RealTrust;
        port.create_dir(&made).unwrap();
        let Security::Windows(d) = port.node(&made).unwrap().unwrap().security else {
            panic!()
        };
        let aces = d.dacl.unwrap();
        let sids: Vec<&str> = aces.iter().map(|a| a.sid.as_str()).collect();
        assert_eq!(sids.len(), 3, "{aces:?}");
        for sid in ["S-1-5-18", "S-1-5-32-544", "S-1-5-32-545"] {
            assert!(sids.contains(&sid), "{sids:?}");
        }
        assert_eq!(
            port.create_dir(&made).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn a_junction_in_the_middle_of_a_path_is_a_link_and_the_held_final_path_differs() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("prog.exe"), b"MZ").unwrap();
        let junction = dir.path().join("junction");
        let out = Command::new("cmd")
            .args(["/C", "mklink", "/J", &text(&junction), &text(&target)])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let mut port = RealTrust;
        assert_eq!(
            port.node(&text(&junction)).unwrap().unwrap().kind,
            FileKind::Symlink
        );
        let through = format!("{}\\prog.exe", text(&junction));
        let e = verify_chain(&mut port, &through, FileKind::File, None, false).unwrap_err();
        assert_eq!(e.reason, Reason::Link, "{e}");
        // Windows may refuse to traverse a junction made by an ordinary user at all ("untrusted mount point", 448); if it
        // opens, the handle's final path is where the junction leads, which is not the requested path.
        match HeldImage::open(&through) {
            Ok(held) => {
                assert!(
                    super::super::trust::require_same_path(
                        &through,
                        held.final_path(),
                        Style::Windows
                    )
                    .is_err(),
                    "the handle's final path {} is not the requested {through}",
                    held.final_path()
                );
                assert!(
                    held.final_path()
                        .to_lowercase()
                        .ends_with("target\\prog.exe"),
                    "{}",
                    held.final_path()
                );
            }
            Err(e) => assert!(matches!(e.raw_os_error(), Some(448 | 1920 | 4390)), "{e}"),
        }
        // Removing the tree removes the junction and not its target.
        port.remove_tree(&text(&junction)).unwrap();
        assert!(target.join("prog.exe").exists());
    }

    #[test]
    fn a_held_image_cannot_be_written_renamed_or_deleted_but_can_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prog.exe");
        std::fs::write(&file, b"MZ").unwrap();
        let held = HeldImage::open(&text(&file)).unwrap();
        let sharing_violation =
            |r: io::Result<()>| assert_eq!(r.unwrap_err().raw_os_error(), Some(32));
        sharing_violation(
            std::fs::OpenOptions::new()
                .write(true)
                .open(&file)
                .map(drop),
        );
        sharing_violation(std::fs::remove_file(&file));
        sharing_violation(std::fs::rename(&file, dir.path().join("moved.exe")));
        assert_eq!(std::fs::read(&file).unwrap(), b"MZ");
        drop(held);
        std::fs::remove_file(&file).unwrap();
    }
}
