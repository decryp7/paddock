//! The fixed `FileVersion` of a program, from its version resource (`major.minor.patch`). One reader for `apply-update`'s
//! "installed version" and for `check-version` (rule 55).

use super::wide::to_wide;
use semver::Version;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
};

/// The version of the program at `path`, or why it could not be read.
pub fn file_version(path: &Path) -> Result<Version, String> {
    let wide = to_wide(&path.to_string_lossy());
    let mut dummy = 0u32;
    // SAFETY: NUL-terminated path.
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut dummy) };
    if size == 0 || size > 1 << 20 {
        return Err(super::handle::last_error().to_string());
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: `data` holds `size` bytes.
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return Err(super::handle::last_error().to_string());
    }
    let root = to_wide("\\");
    let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut len = 0u32;
    // SAFETY: `data` is a version block; `ptr` points into it afterwards.
    if unsafe { VerQueryValueW(data.as_ptr().cast(), root.as_ptr(), &mut ptr, &mut len) } == 0
        || ptr.is_null()
        || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
    {
        return Err("no fixed version info".into());
    }
    // SAFETY: checked length; read unaligned.
    let fixed = unsafe { std::ptr::read_unaligned(ptr.cast::<VS_FIXEDFILEINFO>()) };
    Ok(Version::new(
        u64::from(fixed.dwFileVersionMS >> 16),
        u64::from(fixed.dwFileVersionMS & 0xFFFF),
        u64::from(fixed.dwFileVersionLS >> 16),
    ))
}
