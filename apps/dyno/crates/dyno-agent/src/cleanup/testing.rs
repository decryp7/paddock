//! Helpers for the temp-folder tests of this module (compiled only for tests).

use dyno_core::time::Timestamp;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Now as a [`Timestamp`].
pub fn now() -> Timestamp {
    let ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    Timestamp(i64::try_from(ms).unwrap_or(i64::MAX))
}

/// Sets a file's or folder's modification time `days` into the past.
pub fn backdate(path: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    #[cfg(unix)]
    let f = std::fs::File::open(path);
    // Windows opens a folder only with backup semantics, and setting times needs the attribute-write right.
    #[cfg(windows)]
    let f = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::File::options().access_mode(0x100).custom_flags(0x0200_0000).open(path)
    };
    #[cfg(not(any(unix, windows)))]
    let f = std::fs::File::options().write(true).open(path);
    if let Ok(f) = f {
        let _ = f.set_modified(t);
    }
}
