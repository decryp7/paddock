//! Where Pitwall keeps its files, and writing them safely (atomic, with a backup; loading tolerates damage).

use std::path::{Path, PathBuf};

/// The folders under the data directory.
#[derive(Debug, Clone)]
pub struct Dirs {
    /// The data directory (`PITWALL_HOME` overrides the per-OS default; tests use a temp folder).
    pub root: PathBuf,
}

impl Dirs {
    /// The per-user data directory of this OS.
    pub fn for_user() -> Dirs {
        if let Some(h) = std::env::var_os("PITWALL_HOME").filter(|h| !h.is_empty()) {
            return Dirs { root: PathBuf::from(h) };
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let root = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(home).join("Pitwall")
        } else if cfg!(target_os = "macos") {
            home.join("Library/Application Support/Pitwall")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".local/share"))
                .join("pitwall")
        };
        Dirs { root }
    }
    /// Dirs in a given folder.
    pub fn at(root: impl Into<PathBuf>) -> Dirs {
        Dirs { root: root.into() }
    }
    /// `settings.toml`.
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.toml")
    }
    /// `apps.json` (keys are stored hashed).
    pub fn apps(&self) -> PathBuf {
        self.root.join("apps.json")
    }
    /// The admin key file the local management client reads (owner-only).
    pub fn admin_key(&self) -> PathBuf {
        self.root.join("admin.key")
    }
    /// The model library.
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }
    /// Installed engine builds.
    pub fn engines(&self) -> PathBuf {
        self.root.join("engines")
    }
    /// Partial downloads.
    pub fn partial(&self) -> PathBuf {
        self.root.join("partial")
    }
    /// The discovery file (ADR 0009).
    pub fn discovery(&self) -> PathBuf {
        self.root.join(pitwall_core::discovery::FILE_NAME)
    }
    /// Metrics history.
    pub fn metrics(&self) -> PathBuf {
        self.root.join("metrics.json")
    }
}

/// Write `data` to `path` atomically, keeping the previous file as `<path>.bak`.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    if path.exists() {
        let _ = std::fs::copy(path, backup_of(path));
    }
    std::fs::rename(&tmp, path)
}

/// `<path>.bak`.
pub fn backup_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

/// Read the primary file and its backup, bounded to `max` bytes (a missing or oversized file reads as `None`).
pub fn read_with_backup(path: &Path, max: usize) -> (Option<String>, Option<String>) {
    let read = |p: &Path| {
        std::fs::metadata(p)
            .ok()
            .filter(|m| m.len() as usize <= max)
            .and_then(|_| std::fs::read_to_string(p).ok())
    };
    (read(path), read(&backup_of(path)))
}

/// Make a file owner-only on Unix (a no-op on Windows, where the per-user folder is already private).
pub fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Random bytes from the OS (a failure is impossible in practice; the fallback is time-seeded and still unique).
pub fn random32() -> [u8; 32] {
    let mut b = [0u8; 32];
    if getrandom::fill(&mut b).is_err() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        for (i, x) in b.iter_mut().enumerate() {
            *x = (n >> (i % 16)) as u8 ^ (i as u8).wrapping_mul(31);
        }
    }
    b
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_keeps_a_backup_and_reads_back() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a/settings.toml");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        let (a, b) = read_with_backup(&p, 100);
        assert_eq!((a.as_deref(), b.as_deref()), (Some("two"), Some("one")));
        assert_eq!(read_with_backup(&p, 2).0, None);
    }

    #[test]
    fn layout_and_random() {
        let d = Dirs::at("/x");
        assert_eq!(d.models(), PathBuf::from("/x/models"));
        assert_ne!(random32(), random32());
        assert!(now_ms() > 1_700_000_000_000);
    }
}
