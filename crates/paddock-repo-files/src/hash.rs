use crate::{RepoFile, RepoFilesError};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of `bytes` as lowercase hex.
pub fn hash_bytes(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

/// SHA-256 of a file, streamed, failing with [`RepoFilesError::TooLarge`] when it has more than `max_bytes`.
pub fn hash_file(path: &Path, max_bytes: u64) -> Result<String, RepoFilesError> {
    let io = |source| RepoFilesError::Io {
        path: path.to_path_buf(),
        source,
    };
    let file = std::fs::File::open(path).map_err(io)?;
    let mut reader = file.take(max_bytes.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
    }
    if total > max_bytes {
        return Err(RepoFilesError::TooLarge {
            path: path.to_path_buf(),
            limit: max_bytes,
        });
    }
    Ok(hex(hasher.finalize()))
}

/// Hashes `files` (relative to `root`) on at most `threads` threads. The result has the order of `files`, whatever the
/// thread count; the first failure (in that order) is returned.
pub fn hash_files(
    root: &Path,
    files: &[RepoFile],
    threads: usize,
    max_bytes: u64,
) -> Result<Vec<String>, RepoFilesError> {
    let threads = threads.clamp(1, files.len().max(1));
    let chunk = files.len().div_ceil(threads).max(1);
    let parts: Vec<Result<Vec<String>, RepoFilesError>> = std::thread::scope(|s| {
        let handles: Vec<_> = files
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .map(|f| hash_file(&root.join(&f.path), max_bytes))
                        .collect::<Result<Vec<_>, _>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("hashing a file does not panic"))
            .collect()
    });
    let mut out = Vec::with_capacity(files.len());
    for p in parts {
        out.extend(p?);
    }
    Ok(out)
}

/// One hash for a listing: SHA-256 over `path NUL hash LF` for every entry, in the order given (sort first for a
/// content address).
pub fn hash_listing(entries: &[(String, String)]) -> String {
    let mut h = Sha256::new();
    for (path, hash) in entries {
        h.update(path.as_bytes());
        h.update([0]);
        h.update(hash.as_bytes());
        h.update(b"\n");
    }
    hex(h.finalize())
}
