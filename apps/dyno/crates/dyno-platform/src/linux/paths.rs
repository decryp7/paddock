//! Where the Linux adapters read from, so tests can point them at a temporary tree, and small bounded readers.

use crate::parse::linux_proc::MAX_PROC_TEXT;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The roots of the pseudo file systems the adapters read.
#[derive(Debug, Clone)]
pub struct HostPaths {
    /// `/proc`.
    pub proc: PathBuf,
    /// `/sys`.
    pub sys: PathBuf,
}

impl HostPaths {
    /// This machine's `/proc` and `/sys`.
    pub fn host() -> Self {
        HostPaths {
            proc: PathBuf::from("/proc"),
            sys: PathBuf::from("/sys"),
        }
    }

    #[cfg(test)]
    /// A tree under `root` (`root/proc`, `root/sys`), for tests.
    pub fn under(root: &Path) -> Self {
        HostPaths {
            proc: root.join("proc"),
            sys: root.join("sys"),
        }
    }

    /// `/proc/<pid>`.
    pub fn pid(&self, pid: u32) -> PathBuf {
        self.proc.join(pid.to_string())
    }
}

/// At most [`MAX_PROC_TEXT`] bytes of a file as text (invalid UTF-8 replaced); `None` when it can't be read.
pub fn read_text(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_PROC_TEXT as u64)
        .read_to_end(&mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// A small file's text, trimmed; `None` when unreadable or empty.
pub fn read_trim(path: &Path) -> Option<String> {
    read_text(path).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// The home folder from `$HOME`, when set and absolute.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).filter(|h| h.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_readers() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let f = dir.path().join("x");
        std::fs::write(&f, b"  12 \n").unwrap();
        assert_eq!(read_trim(&f).as_deref(), Some("12"));
        std::fs::write(&f, b"\xff\xfeok").unwrap();
        assert!(read_text(&f).unwrap().ends_with("ok"));
        std::fs::write(&f, vec![b'a'; MAX_PROC_TEXT + 10]).unwrap();
        assert_eq!(read_text(&f).unwrap().len(), MAX_PROC_TEXT);
        std::fs::write(&f, b" \n").unwrap();
        assert_eq!(read_trim(&f), None);
        assert_eq!(read_text(&dir.path().join("missing")), None);
        let p = HostPaths::under(dir.path());
        assert_eq!(p.pid(7), dir.path().join("proc/7"));
        assert_eq!(HostPaths::host().sys, Path::new("/sys"));
    }
}
