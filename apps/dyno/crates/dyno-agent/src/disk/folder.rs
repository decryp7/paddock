//! Checking a folder the person chose before it is scanned.

use dyno_core::disk::has_dot_segment;
use std::path::{Path, PathBuf};

/// The canonical folder for `path`, or why it cannot be scanned (in words for the person).
pub fn check(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if !p.is_absolute() || has_dot_segment(p) {
        return Err("Give the full path of a folder.".into());
    }
    let real = std::fs::canonicalize(p).map_err(|_| "That folder does not exist or cannot be read.".to_string())?;
    if !real.is_dir() {
        return Err("That is a file; choose a folder.".into());
    }
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_validated() {
        let t = paddock_testkit::private_tempdir().unwrap();
        std::fs::write(t.path().join("b"), b"x").unwrap();
        assert!(check("relative/path").is_err());
        assert!(check(paddock_testkit::path::under(t.path(), "a/../b").to_str().unwrap()).is_err());
        assert!(check(paddock_testkit::path::under(t.path(), "missing").to_str().unwrap()).is_err());
        assert!(check(paddock_testkit::path::under(t.path(), "b").to_str().unwrap())
            .unwrap_err()
            .contains("file"));
        assert!(check(&t.path().display().to_string()).is_ok());
    }
}
