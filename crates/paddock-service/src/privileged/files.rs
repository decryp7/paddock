//! The file side of a protected install: the [`ProtectedFiles`] port (what is there, make a folder, copy the program with
//! one open handle while hashing it, remove), the real adapter, and the rule that decides whether a copy is protected.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use std::io;
use std::path::{Path, PathBuf};

/// What kind of thing a path is (never following a link).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A regular file.
    File,
    /// A folder.
    Dir,
    /// A symbolic link.
    Symlink,
    /// Anything else.
    Other,
}

/// What the OS says about a path, without following a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    /// What it is.
    pub kind: FileKind,
    /// The owner's user id (`None` where the OS has none: Windows, where the ACL is checked instead).
    pub owner: Option<u32>,
    /// The permission bits, with setuid, setgid and sticky (`None` on Windows).
    pub mode: Option<u32>,
    /// The size in bytes.
    pub len: u64,
}

/// The file operations of an install, so tests never touch the real disk.
pub trait ProtectedFiles: Send {
    /// What is at `path`; `Ok(None)` when nothing is.
    fn facts(&mut self, path: &Path) -> io::Result<Option<FileFacts>>;
    /// Creates the folder (and missing parents) with `mode` where the OS has modes.
    fn create_dir(&mut self, path: &Path, mode: u32) -> io::Result<()>;
    /// Opens `path` once without following a link, takes its facts from that open handle and, when it is a regular file of at
    /// most `max` bytes, reads all of it from the same handle. What is checked and what is later written are therefore the
    /// same bytes, whatever happens to the path afterwards. `Ok(None)` when nothing is there; a link is reported as a link
    /// (no bytes); a file over `max` is `InvalidData`.
    fn open_source(&mut self, path: &Path, max: u64) -> io::Result<Option<HeldSource>>;
    /// Writes `bytes` to a new temporary file next to `to`, hashing what it writes, then renames it over `to` (a running copy
    /// is replaced atomically on Unix). Returns the SHA-256 in hex.
    fn write_program(&mut self, bytes: &[u8], to: &Path, mode: u32) -> io::Result<String>;
    /// The SHA-256 of a file in hex.
    fn sha256(&mut self, path: &Path) -> io::Result<String>;
    /// Opens `path`, takes its facts from the open handle (so they describe the file that is read, not a path that may
    /// change) and reads at most `max` bytes as text. `Ok(None)` when nothing is there; a link is reported as a link and not
    /// followed (empty text); a file over `max` is `InvalidData`.
    fn read_checked(&mut self, path: &Path, max: u64) -> io::Result<Option<(FileFacts, String)>>;
    /// Removes a file; a missing file is `NotFound`.
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    /// Removes an empty folder; a missing folder is `NotFound`.
    fn remove_dir(&mut self, path: &Path) -> io::Result<()>;
}

/// A program read through one open handle: its facts and its bytes ([`ProtectedFiles::open_source`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldSource {
    /// What the open handle is.
    pub facts: FileFacts,
    /// The bytes read from that handle (empty unless it is a regular file).
    pub bytes: Vec<u8>,
}

/// Why a path is not a protected place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// It is not there.
    Missing,
    /// It is a link, or not the kind of thing it must be.
    WrongKind(FileKind),
    /// It belongs to a user id other than root's.
    NotRoot(u32),
    /// Its mode lets the group or others change it.
    Writable(u32),
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Violation::Missing => f.write_str("it is not there"),
            Violation::WrongKind(k) => write!(f, "it is a {k:?}, not what the install expects"),
            Violation::NotRoot(uid) => write!(f, "it belongs to user id {uid}, not root"),
            Violation::Writable(m) => {
                write!(f, "its mode {m:o} lets the group or others change it")
            }
        }
    }
}

/// The protected-location rule for one path: the right kind (never a link), owned by root and not writable by the group
/// or others. Where the OS has no owner or mode (Windows) only the kind is checked; the ACL is checked by
/// [`crate::privileged::windows_acl`].
///
/// # Errors
/// The first [`Violation`].
pub fn verify_protected(facts: Option<&FileFacts>, want: FileKind) -> Result<(), Violation> {
    let f = facts.ok_or(Violation::Missing)?;
    if f.kind != want {
        return Err(Violation::WrongKind(f.kind));
    }
    if let Some(owner) = f.owner
        && owner != 0
    {
        return Err(Violation::NotRoot(owner));
    }
    if let Some(mode) = f.mode
        && mode & 0o022 != 0
    {
        return Err(Violation::Writable(mode & 0o7777));
    }
    Ok(())
}

/// The real disk.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFiles;

fn kind_of(t: std::fs::FileType) -> FileKind {
    if t.is_symlink() {
        FileKind::Symlink
    } else if t.is_dir() {
        FileKind::Dir
    } else if t.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    }
}

/// Says in the log that a temporary program file could not be removed and is left behind.
fn warn_if_left(left: bool, path: &Path) {
    if left {
        log::warn!(
            "a temporary program file could not be removed and is left behind: {}",
            path.display()
        );
    }
}

fn temp_name(to: &Path) -> PathBuf {
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    to.with_file_name(format!(".{name}.new{}", std::process::id()))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    hash_stream(bytes, None).unwrap_or_default()
}

fn hash_stream(
    mut input: impl io::Read,
    mut output: Option<&mut dyn io::Write>,
) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(out) = output.as_mut() {
            out.write_all(&buf[..n])?;
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn facts_of(meta: &std::fs::Metadata) -> FileFacts {
    #[cfg(unix)]
    let (owner, mode) = {
        use std::os::unix::fs::MetadataExt;
        (Some(meta.uid()), Some(meta.mode() & 0o7777))
    };
    #[cfg(not(unix))]
    let (owner, mode) = (None, None);
    FileFacts {
        kind: kind_of(meta.file_type()),
        owner,
        mode,
        len: meta.len(),
    }
}

impl ProtectedFiles for RealFiles {
    fn facts(&mut self, path: &Path) -> io::Result<Option<FileFacts>> {
        match std::fs::symlink_metadata(path) {
            Ok(m) => Ok(Some(facts_of(&m))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn read_checked(&mut self, path: &Path, max: u64) -> io::Result<Option<(FileFacts, String)>> {
        use io::Read;
        let link = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        if link.file_type().is_symlink() {
            return Ok(Some((facts_of(&link), String::new())));
        }
        let file = std::fs::File::open(path)?;
        let facts = facts_of(&file.metadata()?);
        if facts.len > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is larger than {max} bytes", path.display()),
            ));
        }
        let mut text = String::new();
        file.take(max).read_to_string(&mut text)?;
        Ok(Some((facts, text)))
    }

    fn create_dir(&mut self, path: &Path, mode: u32) -> io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(mode)
                .create(path)
        }
        #[cfg(not(unix))]
        {
            let _: u32 = mode;
            std::fs::create_dir_all(path)
        }
    }

    fn open_source(&mut self, path: &Path, max: u64) -> io::Result<Option<HeldSource>> {
        use io::Read;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Never follow a link at the last component; never block on a pipe.
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_OPEN_REPARSE_POINT: a link is opened as the link, not followed. FILE_FLAG_BACKUP_SEMANTICS: a folder can be
            // opened at all (and is then reported as a folder, not as an access error).
            options.custom_flags(0x0020_0000 | 0x0200_0000);
        }
        let file = match options.open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                // O_NOFOLLOW on a link fails (ELOOP): report it as a link, not as an unreadable file.
                return match std::fs::symlink_metadata(path) {
                    Ok(m) if m.file_type().is_symlink() => Ok(Some(HeldSource {
                        facts: facts_of(&m),
                        bytes: Vec::new(),
                    })),
                    _ => Err(e),
                };
            }
        };
        let facts = facts_of(&file.metadata()?);
        if facts.kind != FileKind::File {
            return Ok(Some(HeldSource {
                facts,
                bytes: Vec::new(),
            }));
        }
        let mut bytes = Vec::with_capacity(usize::try_from(facts.len.min(max)).unwrap_or(0));
        file.take(max + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the program is larger than {max} bytes"),
            ));
        }
        Ok(Some(HeldSource { facts, bytes }))
    }

    fn write_program(&mut self, bytes: &[u8], to: &Path, mode: u32) -> io::Result<String> {
        let tmp = temp_name(to);
        warn_if_left(paddock_proc::cleanup::remove_file(&tmp), &tmp);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _: u32 = mode;
        let mut out = options.open(&tmp)?;
        let result = hash_stream(bytes, Some(&mut out)).and_then(|hash| {
            out.sync_all()?;
            Ok(hash)
        });
        drop(out);
        match result {
            Ok(hash) => {
                #[cfg(unix)]
                {
                    // The creation mode is cut by the umask; the final mode is exactly the one asked for.
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
                }
                if let Err(e) = std::fs::rename(&tmp, to) {
                    warn_if_left(paddock_proc::cleanup::remove_file(&tmp), &tmp);
                    return Err(e);
                }
                Ok(hash)
            }
            Err(e) => {
                warn_if_left(paddock_proc::cleanup::remove_file(&tmp), &tmp);
                Err(e)
            }
        }
    }

    fn sha256(&mut self, path: &Path) -> io::Result<String> {
        hash_stream(std::fs::File::open(path)?, None)
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir(&mut self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the installer does: one open handle, then the write of those bytes.
    fn copy(files: &mut RealFiles, from: &Path, to: &Path, mode: u32) -> io::Result<String> {
        let held = files
            .open_source(from, 1 << 20)?
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        files.write_program(&held.bytes, to, mode)
    }

    fn facts(kind: FileKind, owner: Option<u32>, mode: Option<u32>) -> FileFacts {
        FileFacts {
            kind,
            owner,
            mode,
            len: 1,
        }
    }

    #[test]
    fn only_a_root_owned_unwritable_thing_of_the_right_kind_is_protected() {
        use FileKind::*;
        assert_eq!(verify_protected(None, File), Err(Violation::Missing));
        assert_eq!(
            verify_protected(Some(&facts(Symlink, Some(0), Some(0o755))), File),
            Err(Violation::WrongKind(Symlink))
        );
        assert_eq!(
            verify_protected(Some(&facts(Dir, Some(0), Some(0o755))), File),
            Err(Violation::WrongKind(Dir))
        );
        assert_eq!(
            verify_protected(Some(&facts(File, Some(501), Some(0o755))), File),
            Err(Violation::NotRoot(501))
        );
        assert_eq!(
            verify_protected(Some(&facts(File, Some(0), Some(0o775))), File),
            Err(Violation::Writable(0o775))
        );
        assert_eq!(
            verify_protected(Some(&facts(File, Some(0), Some(0o757))), File),
            Err(Violation::Writable(0o757))
        );
        assert_eq!(
            verify_protected(Some(&facts(Dir, Some(0), Some(0o1755))), Dir),
            Ok(())
        );
        assert_eq!(
            verify_protected(Some(&facts(File, Some(0), Some(0o755))), File),
            Ok(())
        );
        // Windows reports neither: the kind is all that can be checked here.
        assert_eq!(
            verify_protected(Some(&facts(File, None, None)), File),
            Ok(())
        );
        for v in [
            Violation::Missing,
            Violation::WrongKind(Other),
            Violation::NotRoot(5),
            Violation::Writable(0o777),
        ] {
            assert!(!v.to_string().is_empty());
        }
    }

    #[test]
    fn the_real_disk_copies_hashes_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = RealFiles;
        let src = dir.path().join("src");
        std::fs::write(&src, b"hello").unwrap();
        let folder = dir.path().join("a").join("b");
        files.create_dir(&folder, 0o755).unwrap();
        let dst = folder.join("prog");
        let hash = copy(&mut files, &src, &dst, 0o755).unwrap();
        // SHA-256("hello")
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(files.sha256(&dst).unwrap(), hash);
        let f = files.facts(&dst).unwrap().unwrap();
        assert_eq!((f.kind, f.len), (FileKind::File, 5));
        #[cfg(unix)]
        assert_eq!(f.mode, Some(0o755));
        assert_eq!(files.facts(&folder).unwrap().unwrap().kind, FileKind::Dir);
        // Replacing a copy leaves no temporary file behind.
        std::fs::write(&src, b"hello again").unwrap();
        copy(&mut files, &src, &dst, 0o755).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello again");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        files.remove_file(&dst).unwrap();
        files.remove_dir(&folder).unwrap();
        assert_eq!(files.facts(&folder).unwrap(), None);
        assert_eq!(
            files.remove_file(&dst).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            copy(&mut files, &dir.path().join("none"), &dst, 0o755)
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn a_file_is_read_with_the_facts_of_the_open_handle_and_a_bound() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = RealFiles;
        let path = dir.path().join("policy.toml");
        assert_eq!(files.read_checked(&path, 10).unwrap(), None);
        std::fs::write(&path, "others = 1").unwrap();
        let (facts, text) = files.read_checked(&path, 10).unwrap().unwrap();
        assert_eq!(
            (facts.kind, facts.len, text.as_str()),
            (FileKind::File, 10, "others = 1")
        );
        assert_eq!(
            files.read_checked(&path, 9).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(files.read_checked(&path, 10).is_err());
        #[cfg(unix)]
        {
            let link = dir.path().join("l");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            let (facts, text) = files.read_checked(&link, 10).unwrap().unwrap();
            assert_eq!((facts.kind, text.as_str()), (FileKind::Symlink, ""));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_reported_as_a_link_and_a_failed_copy_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = RealFiles;
        let target = dir.path().join("t");
        std::fs::write(&target, b"x").unwrap();
        let link = dir.path().join("l");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(files.facts(&link).unwrap().unwrap().kind, FileKind::Symlink);
        // The destination folder is missing: the temporary file cannot be made and nothing is left.
        let err = copy(&mut files, &target, &dir.path().join("no/such/prog"), 0o755).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        // A folder where the program should go: the rename fails and the temporary file is removed.
        let blocked = dir.path().join("blocked");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("keep"), b"k").unwrap();
        assert!(copy(&mut files, &target, &blocked, 0o755).is_err());
        assert!(files.facts(&temp_name(&blocked)).unwrap().is_none());
        assert_eq!(files.facts(&dir.path().join("x")).unwrap(), None);
    }

    #[test]
    fn a_source_is_read_once_through_its_handle_and_a_swap_of_the_path_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = RealFiles;
        let src = dir.path().join("src");
        assert_eq!(files.open_source(&src, 100).unwrap(), None);
        std::fs::write(&src, b"signed bytes").unwrap();
        let held = files.open_source(&src, 100).unwrap().unwrap();
        assert_eq!((held.facts.kind, held.facts.len), (FileKind::File, 12));
        // The path now names another file: what was read stays what is written.
        std::fs::remove_file(&src).unwrap();
        std::fs::write(&src, b"replacement!").unwrap();
        let dst = dir.path().join("dst");
        let hash = files.write_program(&held.bytes, &dst, 0o755).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"signed bytes");
        assert_eq!(hash, hash_bytes(b"signed bytes"));
        // A file over the bound is refused, not truncated.
        assert_eq!(
            files.open_source(&src, 5).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        // A folder is reported as a folder, with no bytes.
        let held = files.open_source(dir.path(), 100).unwrap().unwrap();
        assert_eq!((held.facts.kind, held.bytes.len()), (FileKind::Dir, 0));
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_source_is_reported_as_a_link_and_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let mut files = RealFiles;
        let target = dir.path().join("t");
        std::fs::write(&target, b"x").unwrap();
        let link = dir.path().join("l");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let held = files.open_source(&link, 100).unwrap().unwrap();
        assert_eq!((held.facts.kind, held.bytes.len()), (FileKind::Symlink, 0));
        // A file that cannot be opened for another reason is an error.
        use std::os::unix::fs::PermissionsExt;
        let locked = dir.path().join("locked");
        std::fs::write(&locked, b"x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&locked).is_err() {
            assert!(files.open_source(&locked, 100).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_source_that_is_neither_file_nor_folder_is_reported_as_other_with_no_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let held = RealFiles.open_source(&fifo, 100).unwrap().unwrap();
        assert_eq!((held.facts.kind, held.bytes.len()), (FileKind::Other, 0));
    }
}
