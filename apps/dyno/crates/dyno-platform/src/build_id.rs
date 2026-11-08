//! A program's build id: the start of the SHA-256 of its file. Two builds of one version differ here, so the agent, the app and
//! the helper compare builds, not version strings (Dyno lesson 2026-11-07). The app, the agent and the helper share this one
//! implementation (ER-22).

use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

/// Longest file hashed (a bound on an unbounded input; the programs are a few tens of MiB).
const MAX_HASHED: u64 = 1 << 30;
/// Hex characters kept from the digest.
pub const ID_LEN: usize = 16;

/// The build id of the program file at `path`, or `None` when it cannot be read (or is over the bound).
pub fn file_build_id(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_HASHED {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_HASHED {
            return None;
        }
        hasher.update(&buf[..n]);
    }
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Some(hex[..ID_LEN].to_string())
}

/// The build id of the running program's own file, read once (empty when it cannot be read).
pub fn own_build_id() -> String {
    static OWN: OnceLock<String> = OnceLock::new();
    OWN.get_or_init(|| std::env::current_exe().ok().and_then(|p| file_build_id(&p)).unwrap_or_default())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_follows_the_content_and_an_unreadable_file_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::write(&a, b"build one").unwrap();
        std::fs::write(&b, b"build two!").unwrap();
        let (ia, ib) = (file_build_id(&a).unwrap(), file_build_id(&b).unwrap());
        assert_ne!(ia, ib);
        assert_eq!(ia.len(), ID_LEN);
        assert_eq!(file_build_id(&a).unwrap(), ia, "the same bytes, the same id");
        assert_eq!(file_build_id(&dir.path().join("none")), None);
    }

    /// Measures the hash of real program files: `DYNO_HASH_TIMING=<path>:<path> cargo test -p dyno-platform -- --ignored --nocapture hash_timing`.
    #[test]
    #[ignore = "measures files named by DYNO_HASH_TIMING; run by hand"]
    fn hash_timing() {
        let Some(list) = std::env::var_os("DYNO_HASH_TIMING") else { return };
        for path in std::env::split_paths(&list) {
            let began = std::time::Instant::now();
            let id = file_build_id(&path);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!(
                "hash {} size {size} bytes id {id:?} took {} ms",
                path.display(),
                began.elapsed().as_millis()
            );
        }
    }
}
