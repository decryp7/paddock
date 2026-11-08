//! The file behind `setup.log` (native glue).

use crate::log::{Clock, LogFs};
use std::io::Write;
use std::path::PathBuf;

/// `setup.log` in `$INSTDIR\updates\`.
pub struct FileLog {
    path: PathBuf,
}

impl FileLog {
    /// The log at `path`.
    pub fn new(path: PathBuf) -> FileLog {
        FileLog { path }
    }
}

impl LogFs for FileLog {
    fn len(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }
    fn append(&mut self, text: &str) -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(text.as_bytes())
    }
    fn rotate(&mut self) -> std::io::Result<()> {
        let mut rotated = self.path.clone().into_os_string();
        rotated.push(".1");
        std::fs::rename(&self.path, rotated)
    }
}

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}
