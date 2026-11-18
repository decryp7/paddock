//! Cargo's `CACHEDIR.TAG` (https://bford.info/cachedir/): the file cargo writes into a target folder it creates itself.
//!
//! The test kit shares one toolchain link folder per target folder, and only in a folder with this tag (so a test program copied
//! to `.../deps/x` in a home folder never writes there). A target folder made by anything but cargo (a CI helper that makes a
//! kept build folder before cargo runs) must carry it too, or every test command links the toolchain into its own folder
//! (ADR 0107, ci.yml run 8246).

use std::io::Write;
use std::path::Path;

/// The file's name.
pub const FILE_NAME: &str = "CACHEDIR.TAG";

/// The file's content, as cargo writes it (`cargo-util`'s `paths::create_dir_all_excluded_from_backups_atomic`): the standard
/// signature line and cargo's two comment lines.
pub const CONTENT: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
# This file is a cache directory tag created by cargo.\n\
# For information about cache directory tags see https://bford.info/cachedir/\n";

/// Writes the tag into the folder `target` when it has none. A tag that is there is never rewritten.
///
/// Returns `true` when it wrote the file.
///
/// # Errors
///
/// When the file cannot be made for any reason but already existing.
pub fn ensure(target: &Path) -> std::io::Result<bool> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target.join(FILE_NAME))
    {
        Ok(mut f) => f.write_all(CONTENT.as_bytes()).map(|()| true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
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
        std::fs::write(d.path().join(FILE_NAME), "mine").unwrap();
        assert!(!ensure(d.path()).unwrap());
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE_NAME)).unwrap(),
            "mine"
        );
    }

    #[test]
    fn it_fails_on_a_folder_that_is_not_there() {
        let d = tempfile::tempdir().unwrap();
        assert!(ensure(&d.path().join("missing")).is_err());
    }
}
