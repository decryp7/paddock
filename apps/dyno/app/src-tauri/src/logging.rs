//! A small rotating log in the data folder (`logs/app.log`, one older generation). It records what the shell does
//! (connecting, starting the agent, window and tray events), never file contents or process command lines.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A log file is rotated once it grows past this size.
pub const MAX_BYTES: u64 = 256 * 1024;

/// The log file and its size.
pub struct Log {
    path: PathBuf,
}

impl Log {
    /// A log at `dir/app.log`.
    pub fn new(dir: &Path) -> Log {
        Log { path: dir.join("app.log") }
    }

    fn older(&self) -> PathBuf {
        self.path.with_extension("log.1")
    }

    /// Appends a line (rotating first when the file is large). Failures are ignored: logging never breaks the app.
    pub fn write(&self, line: &str) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::metadata(&self.path).is_ok_and(|m| needs_rotation(m.len(), MAX_BYTES)) {
            let _ = std::fs::rename(&self.path, self.older());
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// Whether a file of `len` bytes should be rotated before the next line.
pub fn needs_rotation(len: u64, max: u64) -> bool {
    len >= max
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Starts logging to `dir`.
pub fn init(dir: &Path) {
    let mut g = LOG.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some(Log::new(dir));
}

/// Logs a line (to standard error too).
pub fn info(message: &str) {
    eprintln!("dyno: {message}");
    let line = format!("{} {message}", stamp());
    if let Some(log) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        log.write(&line);
    }
}

fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("[{secs}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_append_and_a_full_file_rotates_once() {
        let dir = tempfile::tempdir().unwrap();
        let log = Log::new(dir.path());
        log.write("one");
        log.write("two");
        assert_eq!(std::fs::read_to_string(dir.path().join("app.log")).unwrap(), "one\ntwo\n");
        std::fs::write(dir.path().join("app.log"), vec![b'x'; MAX_BYTES as usize]).unwrap();
        log.write("fresh");
        assert_eq!(std::fs::read_to_string(dir.path().join("app.log")).unwrap(), "fresh\n");
        assert_eq!(std::fs::metadata(dir.path().join("app.log.1")).unwrap().len(), MAX_BYTES);
    }

    #[test]
    fn rotation_threshold() {
        assert!(!needs_rotation(9, 10));
        assert!(needs_rotation(10, 10));
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_is_not_an_error() {
        Log::new(Path::new("/dev/null/nope")).write("ignored");
    }
}
