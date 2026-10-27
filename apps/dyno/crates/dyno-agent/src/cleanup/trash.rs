//! The OS Recycle Bin / Trash behind a port, so tests never touch the real one.

use std::path::Path;

/// Moves a file or folder to where the user can get it back.
pub trait Trasher: Send + Sync {
    /// Moves `path` to the Recycle Bin / Trash. The message is for the user.
    fn trash(&self, path: &Path) -> Result<(), String>;
}

/// The operating system's Recycle Bin / Trash.
pub struct OsTrash;

impl Trasher for OsTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        trash::delete(path).map_err(|e| format!("The Recycle Bin / Trash refused it: {e}"))
    }
}

/// A "bin" that is just a folder (tests).
#[cfg(test)]
pub struct FolderBin(pub std::path::PathBuf);

#[cfg(test)]
impl Trasher for FolderBin {
    fn trash(&self, path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(&self.0).map_err(|e| e.to_string())?;
        let name = path.file_name().ok_or("no name")?;
        std::fs::rename(path, self.0.join(name)).map_err(|e| e.to_string())
    }
}
