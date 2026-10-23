//! SHA-256 of a program's file for "search this file's hash on VirusTotal" (WP-33). The file is read here, in this
//! process, and nothing leaves the machine: the hash is shown first and only the hash goes into the address the browser
//! opens. Results are cached by path, size and modification time, so a repeated click costs nothing.

use dyno_core::weblinks::FileHashInfo;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

/// The largest file hashed (a program bigger than this is not a file anyone sends to VirusTotal).
pub const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Hashes remembered.
const CACHE_ENTRIES: usize = 256;

type Key = (String, u64, i64);

/// The cache and the hasher.
#[derive(Default)]
pub struct FileHasher {
    cache: Mutex<HashMap<Key, String>>,
}

fn millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// The hex SHA-256 of everything `r` yields.
pub fn sha256_of(mut r: impl Read) -> std::io::Result<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

impl FileHasher {
    /// A hasher with an empty cache.
    pub fn new() -> FileHasher {
        FileHasher::default()
    }

    /// The hash of the regular file at `path` (an absolute path; links are resolved to the file they point at, which is
    /// what the program is). The message says what to do when it fails.
    pub fn hash(&self, path: &str) -> Result<FileHashInfo, String> {
        let p = Path::new(path);
        if !p.is_absolute() || dyno_core::cleanup::has_dot_segment(p) {
            return Err("That is not a full path to a file.".into());
        }
        let real = std::fs::canonicalize(p).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => "The file is no longer there.".to_string(),
            _ => format!("The file cannot be read: {e}"),
        })?;
        let meta = std::fs::metadata(&real).map_err(|e| format!("The file cannot be read: {e}"))?;
        if !meta.is_file() {
            return Err("That is not a file.".into());
        }
        if meta.len() > MAX_BYTES {
            return Err("The file is too large to hash (over 2 GB).".into());
        }
        let shown = real.to_string_lossy().into_owned();
        let key: Key = (shown.clone(), meta.len(), meta.modified().map(millis).unwrap_or(0));
        if let Some(h) = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            return Ok(FileHashInfo {
                path: shown,
                bytes: meta.len(),
                sha256: h.clone(),
            });
        }
        let file = std::fs::File::open(&real).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => "Dyno is not allowed to read that file.".to_string(),
            _ => format!("The file cannot be opened: {e}"),
        })?;
        let sha256 = sha256_of(file.take(MAX_BYTES + 1)).map_err(|e| format!("The file could not be read completely: {e}"))?;
        let mut c = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if c.len() >= CACHE_ENTRIES {
            c.clear();
        }
        c.insert(key, sha256.clone());
        Ok(FileHashInfo {
            path: shown,
            bytes: meta.len(),
            sha256,
        })
    }

    /// How many hashes are cached (tests).
    pub fn cached(&self) -> usize {
        self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(sha256_of(&b""[..]).unwrap(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_of(&b"abc"[..]).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_file_is_hashed_once_and_then_cached() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("prog.bin");
        std::fs::write(&f, b"abc").unwrap();
        let h = FileHasher::new();
        let a = h.hash(f.to_str().unwrap()).unwrap();
        assert_eq!(a.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(a.bytes, 3);
        assert_eq!(h.cached(), 1);
        assert_eq!(h.hash(f.to_str().unwrap()).unwrap(), a);
        assert_eq!(h.cached(), 1);
        // A changed file (new size) is hashed again.
        std::fs::write(&f, b"abcd").unwrap();
        assert_ne!(h.hash(f.to_str().unwrap()).unwrap().sha256, a.sha256);
        assert_eq!(h.cached(), 2);
    }

    #[test]
    fn bad_paths_say_what_is_wrong() {
        let t = tempfile::tempdir().unwrap();
        let h = FileHasher::new();
        assert!(h.hash("relative/file").unwrap_err().contains("full path"));
        assert!(h.hash(&format!("{}/../x", t.path().display())).unwrap_err().contains("full path"));
        assert!(h.hash(t.path().join("missing").to_str().unwrap()).unwrap_err().contains("no longer there"));
        assert!(h.hash(t.path().to_str().unwrap()).unwrap_err().contains("not a file"));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("secret");
        std::fs::write(&f, b"x").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root can read anything; only assert the message when the open really fails.
        if std::fs::File::open(&f).is_err() {
            assert!(FileHasher::new().hash(f.to_str().unwrap()).unwrap_err().contains("not allowed"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_hashed_as_the_program_it_points_at() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("real.bin");
        std::fs::write(&f, b"abc").unwrap();
        let l = t.path().join("link");
        std::os::unix::fs::symlink(&f, &l).unwrap();
        let info = FileHasher::new().hash(l.to_str().unwrap()).unwrap();
        assert!(info.path.ends_with("real.bin"));
    }
}
