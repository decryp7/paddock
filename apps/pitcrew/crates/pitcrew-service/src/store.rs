//! Reading and writing the service's files (`pitcrew.toml`, `state.json`) safely: bounded reads, and writes that
//! copy the current file to `<file>.bak`, write a temporary file, flush it and rename it into place, so a crash or
//! power loss leaves either the old or the new file, never half of one (ADR 0001).

use crate::error::Error;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Reads a text file of at most `max` bytes (+1, so the caller's parser sees that it is too large); `None` when
/// it does not exist.
pub fn read_bounded(path: &Path, max: usize) -> Result<Option<String>, Error> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                path: path.display().to_string(),
                source,
            })
        }
    };
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// `<path><suffix>`.
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Writes `text` to `path` atomically, keeping the previous contents as `<path>.bak`.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), Error> {
    let io = |source| Error::Io {
        path: path.display().to_string(),
        source,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    if path.exists() {
        std::fs::copy(path, sibling(path, ".bak")).map_err(io)?;
    }
    let tmp = sibling(path, ".tmp");
    {
        // A temp name that is already there (left by a crash, or planted: a link, a second name for someone's file) is removed, never
        // opened: the new file is made, so nothing written reaches what the old name pointed at.
        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(io)?;
        f.write_all(text.as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, path).map_err(io)
}

/// Moves a damaged file aside as `<path>.damaged-<stamp>` (kept for the user).
pub fn set_aside(path: &Path, stamp: u64) -> Result<PathBuf, Error> {
    let target = sibling(path, &format!(".damaged-{stamp}"));
    std::fs::rename(path, &target).map_err(|source| Error::Io {
        path: path.display().to_string(),
        source,
    })?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn writes_atomically_with_a_backup_and_sets_damage_aside() {
        let d = std::env::temp_dir().join(format!("pitcrew-it-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let f = d.join("pitcrew.toml");
        assert_eq!(read_bounded(&f, 10).unwrap(), None);
        write_atomic(&f, "one").unwrap();
        write_atomic(&f, "two").unwrap();
        assert_eq!(read_bounded(&f, 10).unwrap().as_deref(), Some("two"));
        assert_eq!(read_bounded(&sibling(&f, ".bak"), 10).unwrap().as_deref(), Some("one"));
        assert_eq!(read_bounded(&f, 1).unwrap().map(|s| s.len()), Some(2), "max + 1 bytes are read");
        let moved = set_aside(&f, 7).unwrap();
        assert!(moved.ends_with("pitcrew.toml.damaged-7"));
        assert!(set_aside(&f, 8).is_err());
        assert!(read_bounded(&d, 10).is_err(), "a folder is not a file");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Round 6 P2-8: a temp name that is already there (a second name for someone's file) is removed, not written through.
    #[cfg(unix)]
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn a_planted_temp_name_is_replaced_not_written_through() {
        let d = std::env::temp_dir().join(format!("pitcrew-it-store-tmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("state.json");
        let theirs = d.join("theirs.txt");
        std::fs::write(&theirs, "theirs").unwrap();
        std::fs::hard_link(&theirs, sibling(&f, ".tmp")).unwrap();
        write_atomic(&f, "mine").unwrap();
        assert_eq!(
            std::fs::read_to_string(&theirs).unwrap(),
            "theirs",
            "the other name was written through"
        );
        assert_eq!(read_bounded(&f, 10).unwrap().as_deref(), Some("mine"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
