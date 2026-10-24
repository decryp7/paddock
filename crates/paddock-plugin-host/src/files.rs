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
        let file = File::open(path).map_err(|e| FileError::Io(format!("cannot open {}: {e}", path.display())))?;
        let meta = file.metadata().map_err(|e| FileError::Io(format!("cannot read {}: {e}", path.display())))?;
        if !meta.is_file() {
            return Err(FileError::Io(format!("{} is not a file", path.display())));
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        lock(&self.open).insert(id, Arc::new(Open { file: Mutex::new(file), len: meta.len() }));
        Ok(HostFileRef { id, len: meta.len() })
    }

    /// Registers a file the user chose (a dialog, a drop, the command line): no grant needed.
    pub fn open_chosen(&self, path: &Path) -> Result<HostFileRef, FileError> {
        self.register_file(path)
    }

    /// Registers a file inside a folder the plugin was granted: the path is resolved (links followed) and must lie inside a
    /// granted, asked-for read folder.
    pub fn open_granted(&self, manifest: &Manifest, grants: &Grants, path: &Path) -> Result<HostFileRef, FileError> {
        let resolved = resolve(path, false).map_err(|e| FileError::Io(format!("cannot open {}: {e}", path.display())))?;
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
        let open = lock(&self.open)
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::Denied(format!("file {id} was not opened for this plugin")))?;
        let want = u64::from(len.min(READ_CHUNK)).min(open.len.saturating_sub(offset));
        let mut out = vec![0u8; usize::try_from(want).unwrap_or(0)];
        if want == 0 {
            return Ok(out);
        }
        let mut file = lock(&open.file);
        file.seek(SeekFrom::Start(offset)).map_err(|e| FileError::Io(format!("cannot read the file: {e}")))?;
        let mut filled = 0;
        while filled < out.len() {
            match file.read(&mut out[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) => return Err(FileError::Io(format!("cannot read the file: {e}"))),
            }
        }
        out.truncate(filled);
        Ok(out)
    }
}
