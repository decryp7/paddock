//! Cargo's `CACHEDIR.TAG` (https://bford.info/cachedir/): the file cargo writes into a target folder it creates itself.
//!
//! The test kit shares one toolchain link folder per target folder, and only in a folder with this tag (so a test program copied
//! to `.../deps/x` in a home folder never writes there). A target folder made by anything but cargo (a CI helper that makes a
//! kept build folder before cargo runs) must carry it too, or every test command links the toolchain into its own folder
//! (ADR 0107, testcommand-ab.yml run 8246).

use std::io::Read;
use std::path::Path;

/// The file's name.
pub const FILE_NAME: &str = "CACHEDIR.TAG";

/// The file's content, as cargo writes it (`cargo-util`'s `paths::create_dir_all_excluded_from_backups_atomic`): the standard
/// signature line and cargo's two comment lines.
pub const CONTENT: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by cargo.\n\
# For information about cache directory tags see https://bford.info/cachedir/\n";

/// The signature line every tag starts with (the cachedir.tag specification's first 43 bytes).
pub const SIGNATURE: &str = "Signature: 8a477f597d28d172789f06886806bc55";

/// Whether the folder `dir` holds a tag: a file that starts with [`SIGNATURE`]. An empty or partial file (a write that stopped
/// halfway) is not a tag.
#[must_use]
pub fn is_tag(dir: &Path) -> bool {
    let Ok(file) = std::fs::File::open(dir.join(FILE_NAME)) else {
        return false;
    };
    let mut head = Vec::with_capacity(SIGNATURE.len());
    file.take(SIGNATURE.len() as u64)
        .read_to_end(&mut head)
        .is_ok()
        && head == SIGNATURE.as_bytes()
}

/// Makes sure the folder `target` holds a tag. One that is there is never rewritten; a missing, empty or partial one is
/// written to a temporary file beside it and renamed over it, so a reader never sees half a tag.
///
/// Returns `true` when it wrote the file.
///
/// # Errors
///
/// When `target` is not a folder, or the file cannot be written.
pub fn ensure(target: &Path) -> std::io::Result<bool> {
    if !target.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} is not a folder", target.display()),
        ));
    }
    if is_tag(target) {
        return Ok(false);
    }
    crate::write_atomic(&target.join(FILE_NAME), CONTENT.as_bytes(), false)
        .map(|()| true)
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_writes_the_tag_once_and_never_overwrites_one() {
        let d = tempfile::tempdir().unwrap();
        assert!(ensure(d.path()).unwrap());
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE_NAME)).unwrap(),
            CONTENT
        );
        std::fs::write(d.path().join(FILE_NAME), format!("{SIGNATURE}\nmine")).unwrap();
        assert!(!ensure(d.path()).unwrap());
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE_NAME)).unwrap(),
            format!("{SIGNATURE}\nmine")
        );
    }

    #[test]
    fn a_partial_or_empty_file_is_not_a_tag_and_is_repaired() {
        let d = tempfile::tempdir().unwrap();
        for bad in ["", "Signature: 8a477f597d28d172", "not a tag\n"] {
            std::fs::write(d.path().join(FILE_NAME), bad).unwrap();
            assert!(!is_tag(d.path()), "{bad:?}");
            assert!(ensure(d.path()).unwrap(), "{bad:?}");
            assert!(is_tag(d.path()));
            assert_eq!(
                std::fs::read_to_string(d.path().join(FILE_NAME)).unwrap(),
                CONTENT
            );
        }
        // Cargo's own tag (the signature plus its comments) is a tag; so is the bare signature line.
        std::fs::write(d.path().join(FILE_NAME), format!("{SIGNATURE}\n")).unwrap();
        assert!(is_tag(d.path()) && !ensure(d.path()).unwrap());
    }

    #[test]
    fn it_fails_on_a_folder_that_is_not_there() {
        let d = tempfile::tempdir().unwrap();
        assert!(ensure(&d.path().join("missing")).is_err());
    }
}
