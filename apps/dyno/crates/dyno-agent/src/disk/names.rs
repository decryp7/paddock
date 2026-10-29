//! Rebuilding a path from the names an index keeps. The index stores each name as the bytes `OsStr::as_encoded_bytes` gave,
//! so a name that is not valid text (a Unix file name of arbitrary bytes, a Windows name with a lone surrogate) comes back
//! exactly, and the folder it names is the folder that was walked.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The name whose encoded bytes are `bytes`.
fn os_name(bytes: &[u8]) -> OsString {
    // SAFETY: the bytes come from `OsStr::as_encoded_bytes` of a name read on this platform by this program (the index only
    // stores what `DirEntryInfo::name` held, cut at a length that no real name reaches); nothing else writes to the index.
    unsafe { OsString::from_encoded_bytes_unchecked(bytes.to_vec()) }
}

/// `root` joined with the names below the root's own (the first component of an index path is the root's display name).
pub fn join_components(root: &Path, components: &[Vec<u8>]) -> PathBuf {
    components.iter().skip(1).fold(root.to_path_buf(), |p, n| p.join(os_name(n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_name_is_skipped_and_the_rest_is_joined() {
        let p = join_components(Path::new("base"), &[b"Macintosh HD".to_vec(), b"Users".to_vec(), b"alex".to_vec()]);
        assert_eq!(p, Path::new("base").join("Users").join("alex"));
        assert_eq!(join_components(Path::new("base"), &[b"x".to_vec()]), Path::new("base"));
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_comes_back_byte_for_byte() {
        use std::os::unix::ffi::OsStrExt;
        let odd = vec![b'a', 0xff, 0xfe, b'z'];
        let p = join_components(Path::new("base"), &[b"root".to_vec(), odd.clone()]);
        assert_eq!(p.file_name().unwrap().as_bytes(), &odd[..]);
    }
}
