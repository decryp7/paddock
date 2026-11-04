//! UTF-16 helpers for the Win32 calls.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// A NUL-terminated UTF-16 copy of `s`.
pub fn to_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// The text of a UTF-16 buffer up to its first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
}
