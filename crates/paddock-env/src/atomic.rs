//! Atomic file replacement: the new content is written to a temporary file beside the target, flushed to disk, and
//! renamed over it, so a reader (or a crash, or power loss) sees the old file or the new one, never half of one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{Result, io};

static WRITER: Mutex<()> = Mutex::new(());
static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

/// Points in a write where a test can make it fail (a stand-in for a kill or a full disk).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The temporary file exists and is empty.
    Created,
    /// Half of the bytes are in the temporary file.
    PartlyWritten,
    /// All bytes are flushed; the rename has not happened.
    Written,
}

/// Writes `bytes` to `path` atomically, creating its folder; with `private` the file is owner-only (0600) on Unix. One
/// write at a time per process. On failure the old file is untouched and the temporary file is removed.
pub fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> Result<()> {
    write_hooked(path, bytes, &mut |tmp| create(tmp, private), &mut |_, _| {
        Ok(())
    })
}

/// [`write_atomic`] where the caller creates the temporary file: `create` gets its path (beside the target, not existing
/// yet) and must create it new and open it for writing. `paddock-secrets` uses it on Windows to create the file with an
/// owner-only security descriptor and no sharing, so no other principal can open it at any point. A failed creation fails
/// the write like any other step: nothing is replaced and no temporary file is left.
pub fn write_atomic_created(
    path: &Path,
    bytes: &[u8],
    create: &mut dyn FnMut(&Path) -> std::io::Result<std::fs::File>,
) -> Result<()> {
    write_hooked(path, bytes, create, &mut |_, _| Ok(()))
}

pub(crate) fn write_hooked(
    path: &Path,
    bytes: &[u8],
    create: &mut dyn FnMut(&Path) -> std::io::Result<std::fs::File>,
    hook: &mut dyn FnMut(Stage, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let _one = WRITER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io("create folder", dir))?;
    let tmp = temp_name(dir, path);
    let result = write_and_rename(&tmp, path, bytes, create, hook);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn temp_name(dir: &Path, path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ))
}

fn write_and_rename(
    tmp: &Path,
    path: &Path,
    bytes: &[u8],
    create: &mut dyn FnMut(&Path) -> std::io::Result<std::fs::File>,
    hook: &mut dyn FnMut(Stage, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let mut file = create(tmp).map_err(io("create", tmp))?;
    hook(Stage::Created, tmp).map_err(io("prepare", tmp))?;
    let (first, rest) = bytes.split_at(bytes.len() / 2);
    file.write_all(first).map_err(io("write", tmp))?;
    hook(Stage::PartlyWritten, tmp).map_err(io("write", tmp))?;
    file.write_all(rest).map_err(io("write", tmp))?;
    file.sync_all().map_err(io("flush", tmp))?;
    drop(file);
    hook(Stage::Written, tmp).map_err(io("write", tmp))?;
    std::fs::rename(tmp, path).map_err(io("replace", path))?;
    sync_dir(path.parent().unwrap_or(Path::new(".")));
    Ok(())
}

fn create(path: &Path, private: bool) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private; // Windows has no mode; the per-user folder is already private.
    options.open(path)
}

/// Makes the rename durable where the platform needs it (best effort: the data itself is already flushed).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .filter(|n| n.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn writes_and_replaces_creating_the_folder() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a/b/state.json");
        write_atomic(&f, b"one", false).unwrap();
        write_atomic(&f, b"two", false).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"two");
        assert!(leftovers(f.parent().unwrap()).is_empty());
    }

    #[test]
    fn a_failure_before_the_rename_never_shows_a_partial_file() {
        for stage in [Stage::PartlyWritten, Stage::Written] {
            let t = tempfile::tempdir().unwrap();
            let f = t.path().join("state.json");
            write_atomic(&f, b"old content", false).unwrap();
            let mut seen_old = true;
            let r = write_hooked(
                &f,
                b"new content that is longer",
                &mut |t| create(t, false),
                &mut |s, _| {
                    // At the failure point the target is still the complete old file.
                    seen_old &= std::fs::read(&f).unwrap() == b"old content";
                    if s == stage {
                        Err(std::io::Error::other("killed"))
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(r.is_err(), "{stage:?}");
            assert!(seen_old);
            assert_eq!(std::fs::read(&f).unwrap(), b"old content");
            assert!(
                leftovers(t.path()).is_empty(),
                "the temporary file is removed"
            );
        }
    }

    #[test]
    fn a_failure_on_a_new_file_leaves_no_file() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("state.json");
        let r = write_hooked(&f, b"x", &mut |t| create(t, false), &mut |_, _| {
            Err(std::io::Error::other("full"))
        });
        assert!(r.is_err());
        assert!(!f.exists());
    }

    #[test]
    fn a_folder_in_the_target_place_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("state.json");
        std::fs::create_dir(&f).unwrap();
        let e = write_atomic(&f, b"x", false).unwrap_err();
        assert!(e.to_string().contains("state.json"));
        assert!(leftovers(t.path()).is_empty());
    }

    #[test]
    fn a_file_where_the_folder_should_be_is_an_error() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("dir"), "x").unwrap();
        assert!(write_atomic(&t.path().join("dir/state.json"), b"x", false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("secret.json");
        write_atomic(&f, b"x", true).unwrap();
        assert_eq!(
            std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn concurrent_writers_leave_one_complete_file() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("state.json");
        std::thread::scope(|s| {
            for i in 0..8u8 {
                let f = f.clone();
                s.spawn(move || write_atomic(&f, &[b'a' + i; 4096], false).unwrap());
            }
        });
        let got = std::fs::read(&f).unwrap();
        assert_eq!(got.len(), 4096);
        assert!(got.iter().all(|b| *b == got[0]));
    }

    /// The caller creates the temporary file (`paddock-secrets` creates it with an owner-only security descriptor and no
    /// sharing on Windows, so no other principal can open it, not even before the value is written).
    #[test]
    fn the_caller_creates_the_temporary_file_beside_the_target() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("admin.key");
        let mut seen = Vec::new();
        write_atomic_created(&f, b"secret", &mut |tmp| {
            seen.push(tmp.to_path_buf());
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(tmp)
        })
        .unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"secret");
        assert_eq!(seen.len(), 1, "once per write");
        assert_eq!(
            seen[0].parent(),
            Some(t.path()),
            "the temporary file is beside the target"
        );
        assert_ne!(seen[0], f);
        // A failed creation replaces nothing and leaves no temporary file.
        let e = write_atomic_created(&f, b"other", &mut |_| Err(std::io::Error::other("no acl")))
            .unwrap_err();
        assert!(e.to_string().contains("no acl"), "{e}");
        assert_eq!(std::fs::read(&f).unwrap(), b"secret");
        assert!(leftovers(t.path()).is_empty());
    }
}
