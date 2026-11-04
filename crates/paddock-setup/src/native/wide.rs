//! UTF-16 helpers for the Win32 calls.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// A NUL-terminated UTF-16 copy of `s`.
pub fn to_wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// The text of a UTF-16 buffer up to its first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned()
}

/// `CompareStringOrdinal` with `ignoreCase`: the system's own case table decides whether two names are the same.
pub fn compare_ordinal_ignore_case(a: &str, b: &str) -> bool {
    use windows_sys::Win32::Globalization::CompareStringOrdinal;
    const CSTR_EQUAL: i32 = 2;
    let (a16, b16): (Vec<u16>, Vec<u16>) = (
        std::ffi::OsStr::new(a).encode_wide().collect(),
        std::ffi::OsStr::new(b).encode_wide().collect(),
    );
    // SAFETY: the pointers and lengths describe the two live vectors.
    unsafe {
        CompareStringOrdinal(
            a16.as_ptr(),
            a16.len() as i32,
            b16.as_ptr(),
            b16.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}
