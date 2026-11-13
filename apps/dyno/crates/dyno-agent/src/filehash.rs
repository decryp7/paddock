//! SHA-256 of a program's file for "search this file's hash on VirusTotal" (WP-33). The file is read here, in this
//! process, and nothing leaves the machine: the hash is shown first and only the hash goes into the address the browser
//! opens. Results are cached by path, size and modification time, so a repeated click costs nothing.

use dyno_core::weblinks::FileHashInfo;
use paddock_jobs::{copy_hashed, CancelReason, HashingWriter, JobContext, Phase};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// The largest file hashed (a program bigger than this is not a file anyone sends to VirusTotal).
pub const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Hashes remembered.
const CACHE_ENTRIES: usize = 256;

type Key = (String, u64, i64);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Why a hash job ended without a hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashError {
    /// Stopped on request.
    Cancelled(CancelReason),
    /// It could not be done; the text says what to do.
    Failed(String),
}

/// The cache and the hasher.
#[derive(Default)]
pub struct FileHasher {
    cache: Mutex<HashMap<Key, String>>,
}

fn millis(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl FileHasher {
    /// A hasher with an empty cache.
    pub fn new() -> FileHasher {
        FileHasher::default()
    }

    /// The hash of the regular file at `path` (an absolute path; links are resolved to the file they point at, which is
    /// what the program is), as the job `ctx`: one phase counting the bytes read, Cancel checked per chunk. A file hashed
    /// before (same path, size and modification time) answers from the cache without reading. The cache is filled only
    /// when the whole file was read, so a cancelled hash leaves nothing behind.
    pub fn hash_job(&self, ctx: &JobContext, path: &str) -> Result<FileHashInfo, HashError> {
        let p = Path::new(path);
        if !p.is_absolute() || dyno_core::disk::has_dot_segment(p) {
            return Err(HashError::Failed("That is not a full path to a file.".into()));
        }
        let real = std::fs::canonicalize(p).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => HashError::Failed("The file is no longer there.".into()),
            _ => HashError::Failed(format!("The file cannot be read: {e}")),
        })?;
        let meta = std::fs::metadata(&real).map_err(|e| HashError::Failed(format!("The file cannot be read: {e}")))?;
        if !meta.is_file() {
            return Err(HashError::Failed("That is not a file.".into()));
        }
        if meta.len() > MAX_BYTES {
            return Err(HashError::Failed("The file is too large to hash (over 2 GB).".into()));
        }
        let shown = real.to_string_lossy().into_owned();
        let key: Key = (shown.clone(), meta.len(), meta.modified().map(millis).unwrap_or(0));
        if let Some(h) = lock(&self.cache).get(&key) {
            return Ok(FileHashInfo {
                path: shown,
                bytes: meta.len(),
                sha256: h.clone(),
            });
        }
        let mut file = std::fs::File::open(&real).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => HashError::Failed("Dyno is not allowed to read that file.".into()),
            _ => HashError::Failed(format!("The file cannot be opened: {e}")),
        })?;
        ctx.phase(Phase::bytes("hash", "Reading the file", Some(meta.len())));
        let mut limited = (&mut file).take(MAX_BYTES + 1);
        let (digest, _) =
            copy_hashed(&mut limited, std::io::sink(), ctx.cancel_token(), ctx).map_err(
                |e| match HashingWriter::<std::io::Sink>::cancelled(&e) {
                    Some(c) => HashError::Cancelled(c.0),
                    None => HashError::Failed(format!("The file could not be read completely: {e}")),
                },
            )?;
        let sha256 = paddock_jobs::hex(&digest);
        let mut c = lock(&self.cache);
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
        lock(&self.cache).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::testing::Run;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn hash(h: &FileHasher, path: &str) -> Result<FileHashInfo, String> {
        Run::new().body(|ctx| h.hash_job(ctx, path)).map_err(|e| match e {
            HashError::Failed(m) => m,
            HashError::Cancelled(_) => "cancelled".into(),
        })
    }

    #[test]
    fn a_file_is_hashed_once_and_then_cached() {
        let t = paddock_testkit::private_tempdir().unwrap();
        let f = t.path().join("prog.bin");
        std::fs::write(&f, b"abc").unwrap();
        let h = FileHasher::new();
        let a = hash(&h, f.to_str().unwrap()).unwrap();
        assert_eq!(a.sha256, ABC);
        assert_eq!(a.bytes, 3);
        assert_eq!(h.cached(), 1);
        assert_eq!(hash(&h, f.to_str().unwrap()).unwrap(), a);
        assert_eq!(h.cached(), 1);
        // A changed file (new size) is hashed again.
        std::fs::write(&f, b"abcd").unwrap();
        assert_ne!(hash(&h, f.to_str().unwrap()).unwrap().sha256, a.sha256);
        assert_eq!(h.cached(), 2);
    }

    #[test]
    fn bad_paths_say_what_is_wrong() {
        let t = paddock_testkit::private_tempdir().unwrap();
        let h = FileHasher::new();
        assert!(hash(&h, "relative/file").unwrap_err().contains("full path"));
        assert!(hash(&h, &format!("{}/../x", t.path().display())).unwrap_err().contains("full path"));
        assert!(hash(&h, t.path().join("missing").to_str().unwrap())
            .unwrap_err()
            .contains("no longer there"));
        assert!(hash(&h, t.path().to_str().unwrap()).unwrap_err().contains("not a file"));
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let t = paddock_testkit::private_tempdir().unwrap();
        let f = t.path().join("secret");
        std::fs::write(&f, b"x").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Running as root can read anything; only assert the message when the open really fails.
        if std::fs::File::open(&f).is_err() {
            assert!(hash(&FileHasher::new(), f.to_str().unwrap()).unwrap_err().contains("not allowed"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_hashed_as_the_program_it_points_at() {
        let t = paddock_testkit::private_tempdir().unwrap();
        let f = t.path().join("real.bin");
        std::fs::write(&f, b"abc").unwrap();
        let l = t.path().join("link");
        std::os::unix::fs::symlink(&f, &l).unwrap();
        let info = hash(&FileHasher::new(), l.to_str().unwrap()).unwrap();
        assert!(info.path.ends_with("real.bin"));
    }

    #[test]
    fn the_hash_reports_the_bytes_read_and_never_goes_backwards() {
        let t = paddock_testkit::private_tempdir().unwrap();
        let f = t.path().join("big.bin");
        std::fs::write(&f, vec![7u8; 300_000]).unwrap();
        let run = Run::new();
        let info = run.body(|ctx| FileHasher::new().hash_job(ctx, f.to_str().unwrap())).unwrap();
        assert_eq!(info.bytes, 300_000);
        let phases = run.phases();
        assert_eq!(phases.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["hash"]);
        assert_eq!(phases[0].total, Some(300_000));
        assert!(phases[0].counts.windows(2).all(|w| w[0] <= w[1]), "{:?}", phases[0].counts);
        assert_eq!(phases[0].counts.last(), Some(&300_000));
    }

    #[test]
    fn a_cancelled_hash_stops_within_a_chunk_and_caches_nothing() {
        let t = paddock_testkit::private_tempdir().unwrap();
        let f = t.path().join("big.bin");
        std::fs::write(&f, vec![7u8; 4 * 1024 * 1024]).unwrap();
        let h = FileHasher::new();
        let run = Run::new().cancel_when("hash", 64 * 1024);
        let out = run.body(|ctx| h.hash_job(ctx, f.to_str().unwrap()));
        assert!(matches!(out, Err(HashError::Cancelled(_))), "{out:?}");
        assert_eq!(h.cached(), 0, "a cancelled hash fills no cache");
        let read = run.phases()[0].counts.last().copied().unwrap_or(0);
        assert!(read <= 2 * paddock_jobs::CHUNK as u64, "stopped promptly, read {read}");
    }
}
