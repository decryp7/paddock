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
        let mut f = std::fs::File::create(&tmp).map_err(io)?;
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
}
