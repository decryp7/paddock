//! The folder behind `staging::clean` (native glue).

use crate::staging::{Entry, RemoveError, StagingFs};
use std::path::PathBuf;

/// `$INSTDIR\updates` on disk. `skip` is a run folder that is never touched (the one in use by this run).
pub struct UpdatesFolder {
    pub dir: PathBuf,
    pub skip: Option<String>,
}

/// `ERROR_SHARING_VIOLATION` and `ERROR_ACCESS_DENIED` (Windows reports a mapped, running image that way) mean "in use".
fn in_use(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32) | Some(33))
        || e.kind() == std::io::ErrorKind::PermissionDenied
}

impl StagingFs for UpdatesFolder {
    fn list(&mut self) -> Result<Vec<Entry>, String> {
        let read = std::fs::read_dir(&self.dir).map_err(|e| format!("{e}"))?;
        Ok(read
            .filter_map(|e| e.ok())
            .map(|e| Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
            })
            .filter(|e| self.skip.as_deref() != Some(e.name.as_str()))
            .collect())
    }

    fn trash(&mut self, entry: &Entry) -> Result<Entry, RemoveError> {
        // Renaming a folder fails while anything inside it is open (a running installer's image), which finds it without
        // deleting any part of it.
        let renamed = Entry {
            name: format!("{}{}", crate::staging::TRASH_PREFIX, entry.name),
            is_dir: true,
        };
        std::fs::rename(self.dir.join(&entry.name), self.dir.join(&renamed.name))
            .map(|_| renamed)
            .map_err(|e| {
                if in_use(&e) {
                    RemoveError::InUse
                } else {
                    RemoveError::Other(format!("{e}"))
                }
            })
    }

    fn remove(&mut self, entry: &Entry) -> Result<(), RemoveError> {
        let path = self.dir.join(&entry.name);
        // A link is removed as a link: `remove_dir_all` does not follow a junction or symlink inside, and a top-level link
        // is removed with `remove_dir`/`remove_file` by its type.
        let result = if entry.is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        result.map_err(|e| {
            if in_use(&e) {
                RemoveError::InUse
            } else {
                RemoveError::Other(format!("{e}"))
            }
        })
    }
}
