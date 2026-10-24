//! The two ports of a hand-off, so tests never touch the user's machine (rule 7): what exists on disk and starting a
//! program.

use std::ffi::OsString;
use std::path::Path;

/// Whether a path is an existing regular file.
pub trait FileProbe: Send + Sync {
    /// True for an existing file.
    fn is_file(&self, path: &Path) -> bool;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskProbe;

impl FileProbe for DiskProbe {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// Starts a program without waiting for it (the app it opens keeps running).
pub trait Spawner: Send + Sync {
    /// Starts `program` with `args`, detached, with no console window, and returns at once.
    fn launch(&self, program: &Path, args: &[OsString]) -> Result<(), String>;
}
