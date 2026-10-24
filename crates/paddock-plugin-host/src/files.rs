//! Host files: the files the app opened for a plugin. The plugin reads them by id and range
//! ([`paddock_plugin_api::protocol::HostCall::ReadFile`]) and never learns the path. A file is opened when it is
//! registered (the handle is kept, so a replaced file is not followed), and only a path the user chose or a granted folder
//! contains can be registered.

use crate::grants::{Grants, resolve};
use paddock_plugin_api::host_file::HostFileRef;
use paddock_plugin_api::protocol::READ_CHUNK;
use paddock_plugin_package::Manifest;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::Io(format!("cannot read the file: {e}"))
    }
}

/// Why a file could not be registered or read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// The plugin is not allowed this file (not registered for it, or outside its grants).
    Denied(String),
    /// The file cannot be opened or read.
    Io(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Denied(m) | FileError::Io(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for FileError {}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct Open {
    file: Mutex<File>,
    len: u64,
}

/// The files opened for one plugin. Cheap to clone; clones share the registry.
#[derive(Clone, Default)]
pub struct HostFiles {
    open: Arc<Mutex<HashMap<u64, Arc<Open>>>>,
    next: Arc<AtomicU64>,
}

impl HostFiles {
    /// An empty registry.
    pub fn new() -> HostFiles {
        HostFiles::default()
    }

    fn register_file(&self, path: &Path) -> Result<HostFileRef, FileError> {
        let file = File::open(path)
            .map_err(|e| FileError::Io(format!("cannot open {}: {e}", path.display())))?;
        let meta = file
            .metadata()
            .map_err(|e| FileError::Io(format!("cannot read {}: {e}", path.display())))?;
        if !meta.is_file() {
            return Err(FileError::Io(format!("{} is not a file", path.display())));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        lock(&self.open).insert(
            id,
            Arc::new(Open {
                file: Mutex::new(file),
                len: meta.len(),
            }),
        );
        Ok(HostFileRef {
            id,
            len: meta.len(),
        })
    }

    /// Registers a file the user chose (a dialog, a drop, the command line): no grant needed.
    pub fn open_chosen(&self, path: &Path) -> Result<HostFileRef, FileError> {
        self.register_file(path)
    }

    /// Registers a file inside a folder the plugin was granted: the path is resolved (links followed) and must lie inside a
    /// granted, asked-for read folder.
    pub fn open_granted(
        &self,
        manifest: &Manifest,
        grants: &Grants,
        path: &Path,
    ) -> Result<HostFileRef, FileError> {
        let resolved = resolve(path, false)
            .map_err(|e| FileError::Io(format!("cannot open {}: {e}", path.display())))?;
        let inside = grants
            .readable(manifest)
            .iter()
            .filter_map(|r| std::fs::canonicalize(r).ok())
            .any(|r| resolved.starts_with(&r));
        if !inside {
            return Err(FileError::Denied(format!(
                "{} is not inside a folder you allowed plugin {} to read",
                path.display(),
                manifest.id
            )));
        }
        self.register_file(&resolved)
    }

    /// Forgets a file (its handle is closed).
    pub fn close(&self, id: u64) {
        lock(&self.open).remove(&id);
    }

    /// Forgets every file (the plugin's process ended).
    pub fn close_all(&self) {
        lock(&self.open).clear();
    }

    /// Up to `len` bytes (at most [`READ_CHUNK`]) of file `id` at `offset`. An id that was not registered for this plugin is
    /// [`FileError::Denied`].
    pub fn read(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, FileError> {
        let open = lock(&self.open).get(&id).cloned().ok_or_else(|| {
            FileError::Denied(format!("file {id} was not opened for this plugin"))
        })?;
        let want = u64::from(len.min(READ_CHUNK)).min(open.len.saturating_sub(offset));
        let mut out = Vec::new();
        let mut file = lock(&open.file);
        file.seek(SeekFrom::Start(offset))?;
        // A file shortened since it was opened gives fewer bytes, not an error.
        file.by_ref().take(want).read_to_end(&mut out)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::Grants;

    fn manifest() -> Manifest {
        Manifest::parse(
            "format = 1\n[plugin]\nid = \"net.example.p\"\nname = \"P\"\nversion = \"1.0.0\"\npublisher = \"E\"\nprotocol = \"2.0\"\nbinary = \"p\"\ntargets = [\"x86_64-unknown-linux-gnu\"]\n[[permissions]]\npermission = \"readFolders\"\ngrants = [\"docs\"]\nreason = \"r\"\n",
        )
        .unwrap()
    }

    #[test]
    fn a_registered_file_is_read_by_range_and_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "0123456789").unwrap();
        let files = HostFiles::new();
        let r = files.open_chosen(&path).unwrap();
        assert_eq!(r.len, 10);
        assert_eq!(files.read(r.id, 2, 3).unwrap(), b"234");
        assert_eq!(files.read(r.id, 8, 100).unwrap(), b"89");
        assert_eq!(files.read(r.id, 10, 5).unwrap(), b"");
        assert_eq!(files.read(r.id, 99, 5).unwrap(), b"");
        // The handle is kept: the file replaced by another does not change what the plugin reads.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(files.read(r.id, 0, 4).unwrap(), b"0123");
        let other = files.open_chosen(dir.path()).unwrap_err();
        assert!(
            other.to_string().contains("not a file") || other.to_string().contains("cannot"),
            "{other}"
        );
        files.close(r.id);
        assert!(matches!(files.read(r.id, 0, 1), Err(FileError::Denied(_))));
        assert!(
            files
                .open_chosen(&dir.path().join("missing"))
                .unwrap_err()
                .to_string()
                .contains("cannot open")
        );
    }

    #[test]
    fn an_os_error_while_reading_is_an_io_error_with_the_reason() {
        let e = FileError::from(std::io::Error::other("the disk is gone"));
        assert_eq!(
            e,
            FileError::Io("cannot read the file: the disk is gone".into())
        );
    }

    #[test]
    fn an_id_never_registered_is_denied_and_close_all_forgets_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, "x").unwrap();
        let files = HostFiles::new();
        let a = files.open_chosen(&path).unwrap();
        let b = files.open_chosen(&path).unwrap();
        assert_ne!(a.id, b.id);
        let clone = files.clone();
        files.close_all();
        for id in [a.id, b.id, 42] {
            assert_eq!(
                clone.read(id, 0, 1),
                Err(FileError::Denied(format!(
                    "file {id} was not opened for this plugin"
                )))
            );
        }
        assert_eq!(FileError::Io("i".into()).to_string(), "i");
    }

    #[test]
    fn granted_files_must_lie_inside_a_granted_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (docs, other) = (dir.path().join("docs"), dir.path().join("other"));
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(docs.join("a"), "a").unwrap();
        std::fs::write(other.join("b"), "b").unwrap();
        let m = manifest();
        let g = Grants::none()
            .allow(m.permissions[0].permission.clone())
            .folder("docs", &docs);
        let files = HostFiles::new();
        let r = files.open_granted(&m, &g, &docs.join("a")).unwrap();
        assert_eq!(files.read(r.id, 0, 1).unwrap(), b"a");
        let denied = files.open_granted(&m, &g, &other.join("b")).unwrap_err();
        assert!(
            matches!(&denied, FileError::Denied(m) if m.contains("not inside a folder you allowed plugin net.example.p to read"))
        );
        assert!(matches!(
            files.open_granted(&m, &g, &docs.join("missing")),
            Err(FileError::Io(_))
        ));
        // Nothing granted: nothing readable.
        assert!(matches!(
            files.open_granted(&m, &Grants::none(), &docs.join("a")),
            Err(FileError::Denied(_))
        ));
        // The directory itself is not a file.
        assert!(matches!(
            files.open_granted(&m, &g, &docs),
            Err(FileError::Io(_))
        ));
    }
}
