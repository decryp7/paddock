//! The workspace file system: a port, and the adapter that never follows a link or leaves the workspace.

use std::io::Read;
use std::path::{Path, PathBuf};

use pacenotes_core::RepoPath;

/// What an entry is, from `symlink_metadata` (a link is a link, whatever it points at).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A regular file of this size.
    File(u64),
    /// A folder.
    Dir,
    /// A symbolic link (or a Windows junction or reparse point).
    Link,
    /// A socket, device, pipe or anything else.
    Other,
}

/// One entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// The name, a single path segment.
    pub name: String,
    /// What it is.
    pub kind: EntryKind,
}

/// Why a file-system call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FsError {
    /// There is no such file or folder.
    #[error("not found")]
    NotFound,
    /// The file is larger than the cap.
    #[error("the file is {size} bytes; at most {max} are read")]
    TooLarge {
        /// Its size.
        size: u64,
        /// The cap.
        max: u64,
    },
    /// The path is a link, or resolves outside the workspace.
    #[error("refused: {0}")]
    Refused(String),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
    /// Any other I/O failure.
    #[error("i/o: {0}")]
    Io(String),
}

/// Read-only access to a workspace. `None` for a folder means the workspace root.
pub trait WorkspaceFs: Send + Sync {
    /// The entries of a folder, in no particular order.
    fn list(&self, dir: Option<&RepoPath>) -> Result<Vec<DirEntry>, FsError>;

    /// The content of a regular file, at most `max_bytes`.
    fn read(&self, path: &RepoPath, max_bytes: u64) -> Result<Vec<u8>, FsError>;
}

/// The real file system under one root.
#[derive(Debug, Clone)]
pub struct DiskFs {
    root: PathBuf,
}

fn io(e: std::io::Error) -> FsError {
    if e.kind() == std::io::ErrorKind::NotFound {
        FsError::NotFound
    } else {
        FsError::Io(e.to_string())
    }
}

impl DiskFs {
    /// A file system rooted at `root`, canonicalised once.
    pub fn new(root: &Path) -> Result<DiskFs, FsError> {
        Ok(DiskFs {
            root: root.canonicalize().map_err(io)?,
        })
    }

    /// The canonical root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn join(&self, path: Option<&RepoPath>) -> PathBuf {
        match path {
            Some(p) => p.as_str().split('/').fold(self.root.clone(), |acc, part| acc.join(part)),
            None => self.root.clone(),
        }
    }
}

impl WorkspaceFs for DiskFs {
    fn list(&self, dir: Option<&RepoPath>) -> Result<Vec<DirEntry>, FsError> {
        let at = self.join(dir);
        if !std::fs::symlink_metadata(&at).map_err(io)?.is_dir() {
            return Err(FsError::Refused(format!("{} is not a folder", at.display())));
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&at).map_err(io)? {
            let entry = entry.map_err(io)?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                out.push(DirEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    kind: EntryKind::Other,
                });
                continue;
            };
            let meta = std::fs::symlink_metadata(entry.path()).map_err(io)?;
            let ty = meta.file_type();
            let kind = if ty.is_symlink() {
                EntryKind::Link
            } else if ty.is_dir() {
                EntryKind::Dir
            } else if ty.is_file() {
                EntryKind::File(meta.len())
            } else {
                EntryKind::Other
            };
            out.push(DirEntry { name, kind });
        }
        Ok(out)
    }

    fn read(&self, path: &RepoPath, max_bytes: u64) -> Result<Vec<u8>, FsError> {
        let at = self.join(Some(path));
        let meta = std::fs::symlink_metadata(&at).map_err(io)?;
        if meta.file_type().is_symlink() {
            return Err(FsError::Refused(format!("{path} is a link")));
        }
        if !meta.is_file() {
            return Err(FsError::Refused(format!("{path} is not a regular file")));
        }
        // a link in a folder on the way resolves outside: judge the real location
        let real = at.canonicalize().map_err(io)?;
        if !real.starts_with(&self.root) {
            return Err(FsError::Refused(format!("{path} resolves outside the workspace")));
        }
        if meta.len() > max_bytes {
            return Err(FsError::TooLarge {
                size: meta.len(),
                max: max_bytes,
            });
        }
        let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
        std::fs::File::open(&real)
            .map_err(io)?
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(io)?;
        if bytes.len() as u64 > max_bytes {
            return Err(FsError::TooLarge {
                size: bytes.len() as u64,
                max: max_bytes,
            });
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::Scratch;

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    #[test]
    fn lists_and_reads_regular_files_and_reports_links_as_links() {
        let s = Scratch::new();
        s.write("ws/a.txt", "hello").unwrap();
        s.write("ws/dir/b.txt", "x").unwrap();
        let fs = DiskFs::new(&s.path("ws")).unwrap();
        let mut root: Vec<_> = fs.list(None).unwrap().into_iter().map(|e| (e.name, e.kind)).collect();
        root.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(root, [("a.txt".to_owned(), EntryKind::File(5)), ("dir".to_owned(), EntryKind::Dir)]);
        assert_eq!(fs.list(Some(&p("dir"))).unwrap().len(), 1);
        assert_eq!(fs.read(&p("a.txt"), 10).unwrap(), b"hello");
        assert_eq!(fs.read(&p("a.txt"), 4), Err(FsError::TooLarge { size: 5, max: 4 }));
        assert_eq!(fs.read(&p("missing"), 10), Err(FsError::NotFound));
        assert!(matches!(fs.read(&p("dir"), 10), Err(FsError::Refused(_))));
        assert!(matches!(fs.list(Some(&p("a.txt"))), Err(FsError::Refused(_))));
        assert_eq!(DiskFs::new(&s.path("nope")).unwrap_err(), FsError::NotFound);
        assert_eq!(fs.root(), s.path("ws").canonicalize().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_listed_as_a_link_and_never_read_through() {
        let s = Scratch::new();
        s.write("ws/a.txt", "inside").unwrap();
        s.write("outside/secret.txt", "outside").unwrap();
        std::os::unix::fs::symlink(s.path("outside/secret.txt"), s.path("ws/file-link")).unwrap();
        std::os::unix::fs::symlink(s.path("outside"), s.path("ws/dir-link")).unwrap();
        let fs = DiskFs::new(&s.path("ws")).unwrap();
        let links = fs.list(None).unwrap().into_iter().filter(|e| e.kind == EntryKind::Link).count();
        assert_eq!(links, 2);
        assert!(matches!(fs.read(&p("file-link"), 100), Err(FsError::Refused(_))));
        assert!(
            matches!(fs.read(&p("dir-link/secret.txt"), 100), Err(FsError::Refused(_))),
            "a link on the way resolves outside"
        );
        assert!(matches!(fs.list(Some(&p("dir-link"))), Err(FsError::Refused(_))));
    }
}
