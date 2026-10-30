//! Discovery files as hints (rule 53). A file that says how to reach a running instance (an endpoint, a token, a pid) is
//! only a hint: it may be left by a process that is gone, and a pid in it may since belong to another process. Only a
//! held lock ([`crate::instance`]) or a live connection proves a process. These helpers keep a hint from being mistaken
//! for proof and from removing someone else's file.

use std::io::Write;
use std::path::Path;

/// What a discovery file says about its writer.
pub trait Hint {
    /// The writer's process id.
    fn pid(&self) -> u32;
    /// When the writer started, in milliseconds since the epoch.
    fn started_ms(&self) -> i64;
}

/// The most a hint file may hold; a larger file is not one.
pub const MAX_HINT_BYTES: u64 = 16 * 1024;

/// The bytes of a hint file, `None` when missing, unreadable, not a regular file or larger than [`MAX_HINT_BYTES`]. The
/// file is opened once and read through that handle, so what was checked is what is read.
pub fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_HINT_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_HINT_BYTES).then_some(bytes)
}

/// Whether the hint was written after a wait began at `since_ms`: a file from before that is an old instance's.
pub fn written_since(hint: &impl Hint, since_ms: i64) -> bool {
    hint.started_ms() >= since_ms
}

/// Whether a hint names a live instance: written after the wait began, or `answers` (a connection was made). A file from
/// before the wait that nothing answers is not the one waited for.
pub fn names_live(hint: &impl Hint, since_ms: i64, answers: impl FnOnce() -> bool) -> bool {
    written_since(hint, since_ms) || answers()
}

/// Removes the file at `path` only if `read` still finds the one that was seen (same pid and same start): a newer
/// instance may have written its own meanwhile, and that one is not removed. Best effort: the check and the removal are
/// two steps, so an instance that writes its file between them loses it; the instance lock
/// ([`crate::instance`]), not this file, is what proves a process, and a lost hint is written again.
pub fn remove_if_same<H: Hint>(path: &Path, seen: &H, read: impl Fn(&Path) -> Option<H>) {
    if read(path).is_some_and(|d| d.pid() == seen.pid() && d.started_ms() == seen.started_ms()) {
        let _ = std::fs::remove_file(path);
    }
}

/// Removes the file if it still names this process.
pub fn remove_if_mine<H: Hint>(path: &Path, read: impl Fn(&Path) -> Option<H>) {
    if read(path).is_some_and(|d| d.pid() == std::process::id()) {
        let _ = std::fs::remove_file(path);
    }
}

/// Writes `body` to `path` atomically: a temporary file beside it with a name no other writer uses, created new (never
/// following a link someone left at that name) and readable by the owner only on Unix, then renamed over `path`. The
/// temporary file is removed when anything fails.
pub fn write_private_atomic(path: &Path, body: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = std::path::PathBuf::from(tmp_name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options
        .open(&tmp)
        .and_then(|mut f| f.write_all(body))
        .and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct H(u32, i64);
    impl Hint for H {
        fn pid(&self) -> u32 {
            self.0
        }
        fn started_ms(&self) -> i64 {
            self.1
        }
    }
    fn read(path: &Path) -> Option<H> {
        let t = String::from_utf8(read_bounded(path)?).ok()?;
        let (p, s) = t.split_once(',')?;
        Some(H(p.parse().ok()?, s.parse().ok()?))
    }

    #[test]
    fn a_hint_is_live_only_when_new_or_answered() {
        let old = H(1, 100);
        assert!(written_since(&old, 100));
        assert!(!written_since(&old, 101));
        assert!(names_live(&old, 50, || panic!("not asked: it is new")));
        assert!(!names_live(&old, 200, || false));
        assert!(names_live(&old, 200, || true));
    }

    #[test]
    fn only_the_file_that_was_seen_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agent.json");
        write_private_atomic(&path, b"7,5").unwrap();
        remove_if_same(&path, &H(7, 6), read);
        assert!(path.exists(), "a newer start is not removed");
        remove_if_same(&path, &H(8, 5), read);
        assert!(path.exists(), "another pid is not removed");
        remove_if_same(&path, &H(7, 5), read);
        assert!(!path.exists());
        remove_if_same(&path, &H(7, 5), read);
    }

    #[test]
    fn only_a_file_naming_this_process_is_removed_as_mine() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agent.json");
        write_private_atomic(&path, format!("{},1", std::process::id() + 1).as_bytes()).unwrap();
        remove_if_mine(&path, read);
        assert!(path.exists());
        write_private_atomic(&path, format!("{},1", std::process::id()).as_bytes()).unwrap();
        remove_if_mine(&path, read);
        assert!(!path.exists());
    }

    #[test]
    fn writes_are_atomic_private_and_reads_are_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sub").join("f.json");
        write_private_atomic(&path, b"abc").unwrap();
        assert_eq!(read_bounded(&path).unwrap(), b"abc");
        let leftovers = std::fs::read_dir(tmp.path().join("sub"))
            .unwrap()
            .flatten()
            .count();
        assert_eq!(leftovers, 1, "no temporary file is left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(
            &path,
            vec![0u8; usize::try_from(MAX_HINT_BYTES).unwrap() + 1],
        )
        .unwrap();
        assert!(read_bounded(&path).is_none());
        assert!(read_bounded(&tmp.path().join("missing")).is_none());
        let under_a_file = path.join("x");
        assert!(
            write_private_atomic(&under_a_file, b"x").is_err(),
            "the folder cannot be made under a file"
        );
        // A path that is a folder: the temporary file is written, the rename fails, and the temporary file is removed.
        let folder = tmp.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_private_atomic(&folder, b"x").is_err());
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().flatten().count(),
            2,
            "only `sub` and `folder` remain"
        );
        assert!(read_bounded(&folder).is_none(), "a folder is not a hint");
    }
}
