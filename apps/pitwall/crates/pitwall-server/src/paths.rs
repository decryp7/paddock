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
    /// The server's log folder when the data directory was moved (`--data-dir`, `PITWALL_HOME`): the logs stay with it.
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
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
    /// Benchmark results.
    pub fn benchmarks(&self) -> PathBuf {
        self.root.join("benchmarks.json")
    }
    /// Per-app usage over time.
    pub fn usage(&self) -> PathBuf {
        self.root.join("usage.json")
    }
    /// Engine version pins and the builds used before (ADR 0018).
    pub fn engine_pins(&self) -> PathBuf {
        self.root.join("engine-pins.json")
    }

    /// Model presets and aliases (ADR 0014).
    pub fn presets(&self) -> PathBuf {
        self.root.join("presets.json")
    }
    /// Model update notices and their dismissals (ADR 0017).
    pub fn model_updates(&self) -> PathBuf {
        self.root.join("model-updates.json")
    }
    /// The cache database (ADR 0019).
    pub fn cache_db(&self) -> PathBuf {
        self.root.join("cache.sqlite")
    }

    /// Saved prompt prefixes (llama-server's `--slot-save-path`).
    pub fn slots(&self) -> PathBuf {
        self.root.join("slots")
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

/// Write `data` to `path` atomically and owner-only, through the shared writer of `paddock-secrets` (ADR 0092 WP-0): a
/// new temporary file, created exclusively (`O_EXCL`, no link followed) with mode 0600 on Unix and an
/// owner-only DACL without sharing on Windows, flushed and renamed over the target. The folder is made private or refused
/// (on Windows when others may write in it). No backup copy is kept: use it for files that must not outlive a revocation.
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    paddock_secrets::write_owner_only(path, data).map_err(std::io::Error::other)
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

/// [`restrict`] for a folder: owner-only access on Unix.
pub fn restrict_dir(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// The OS random source failed, so no key can be made (W5: a time-seeded stand-in would be guessable).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the operating system's random source failed ({0}), so no key can be made; restart Pitwall, and if it keeps failing check the system's entropy source"
)]
pub struct RandomError(pub String);

/// 32 random bytes from the OS. Fails closed: when the OS source fails there is no stand-in, because every secret here
/// (admin key, app keys, pairing codes, engine keys) is made from these bytes.
pub fn random32() -> Result<[u8; 32], RandomError> {
    random32_with(|b| getrandom::fill(b).map_err(|e| e.to_string()))
}

/// [`random32`] over a given source (tests inject a failing one).
pub fn random32_with(fill: impl FnOnce(&mut [u8]) -> Result<(), String>) -> Result<[u8; 32], RandomError> {
    let mut b = [0u8; 32];
    match fill(&mut b) {
        Ok(()) => Ok(b),
        // Whatever the source wrote before failing is discarded with the buffer.
        Err(e) => Err(RandomError(e)),
    }
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

    /// ADR 0092 test 5 (W5): with a failing OS source there is an error and no bytes, never a time-seeded stand-in.
    #[test]
    fn a_failing_random_source_gives_an_error_and_no_key() {
        let e = random32_with(|_| Err("no entropy".into())).unwrap_err();
        assert!(
            e.to_string().contains("no entropy") && e.to_string().contains("no key can be made"),
            "{e}"
        );
        // A source that half-filled the buffer before failing gives nothing either.
        assert!(random32_with(|b| {
            b[0] = 1;
            Err("partial".into())
        })
        .is_err());
    }

    #[test]
    fn layout_and_random() {
        let d = Dirs::at("/x");
        assert_eq!(d.models(), PathBuf::from("/x/models"));
        assert_ne!(random32().unwrap(), random32().unwrap());
        assert!(now_ms() > 1_700_000_000_000);
    }
}
