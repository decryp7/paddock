//! Registry reads and deletes (native glue: no decisions).

use super::wide::{from_wide, to_wide};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_SET_VALUE,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW,
};

/// Which root.
#[derive(Clone, Copy)]
pub enum Root {
    /// `HKEY_LOCAL_MACHINE`.
    Machine,
    /// `HKEY_USERS`.
    Users,
}

/// Which view of a 64-bit machine's registry.
#[derive(Clone, Copy)]
pub enum View {
    /// The native 64-bit view.
    Wide,
    /// The 32-bit (WOW6432Node) view.
    Narrow,
}

fn view_flag(v: View) -> u32 {
    match v {
        View::Wide => KEY_WOW64_64KEY,
        View::Narrow => KEY_WOW64_32KEY,
    }
}

/// An open key, closed on drop.
pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from RegOpenKeyExW and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

/// Opens `path` under `root` with `access` (a mask from `windows_sys`).
pub fn open(root: Root, path: &str, view: View, access: u32) -> Option<Key> {
    let root = match root {
        Root::Machine => HKEY_LOCAL_MACHINE,
        Root::Users => HKEY_USERS,
    };
    let wide = to_wide(path);
    let mut out: HKEY = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and `out` is a valid out pointer.
    let rc = unsafe { RegOpenKeyExW(root, wide.as_ptr(), 0, access | view_flag(view), &mut out) };
    (rc == ERROR_SUCCESS).then_some(Key(out))
}

/// Opens for reading.
pub fn open_read(root: Root, path: &str, view: View) -> Option<Key> {
    open(root, path, view, KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS)
}

/// Opens for deleting values.
pub fn open_write(root: Root, path: &str, view: View) -> Option<Key> {
    open(
        root,
        path,
        view,
        KEY_SET_VALUE | KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS,
    )
}

impl Key {
    /// The names of the subkeys (at most 4096).
    pub fn subkeys(&self) -> Vec<String> {
        let mut names = Vec::new();
        for index in 0..4096u32 {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            // SAFETY: `buf` and `len` describe the same buffer.
            let rc = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    buf.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            names.push(from_wide(&buf[..len as usize]));
        }
        names
    }

    /// A string value (`REG_SZ` or `REG_EXPAND_SZ`, not expanded).
    pub fn string(&self, name: &str) -> Option<String> {
        let wide = to_wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: a size query with a null data pointer.
        let rc = unsafe {
            RegQueryValueExW(
                self.0,
                wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if rc != ERROR_SUCCESS || !(kind == REG_SZ || kind == REG_EXPAND_SZ) || size > 64 * 1024 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size2 = (buf.len() * 2) as u32;
        // SAFETY: `buf` holds `size2` bytes.
        let rc = unsafe {
            RegQueryValueExW(
                self.0,
                wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buf.as_mut_ptr().cast(),
                &mut size2,
            )
        };
        (rc == ERROR_SUCCESS).then(|| from_wide(&buf))
    }

    /// Every string value as (name, data), at most 1024.
    pub fn string_values(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for index in 0..1024u32 {
            let mut name = [0u16; 16384];
            let mut name_len = name.len() as u32;
            let mut kind = 0u32;
            let mut data = vec![0u16; 16384];
            let mut data_len = (data.len() * 2) as u32;
            // SAFETY: the buffers and their lengths agree.
            let rc = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut data_len,
                )
            };
            if rc != ERROR_SUCCESS {
                // ERROR_NO_MORE_ITEMS ends; ERROR_MORE_DATA skips the value.
                if rc == windows_sys::Win32::Foundation::ERROR_MORE_DATA {
                    continue;
                }
                break;
            }
            if kind == REG_SZ || kind == REG_EXPAND_SZ {
                let text = from_wide(&data);
                // A REG_EXPAND_SZ command (`%ProgramFiles%\\App\\app.exe`) is compared as Windows would run it.
                let text = if kind == REG_EXPAND_SZ {
                    expand(&text)
                } else {
                    text
                };
                out.push((from_wide(&name[..name_len as usize]), text));
            }
        }
        out
    }

    /// Deletes a value; `true` when it is gone.
    pub fn delete_value(&self, name: &str) -> bool {
        let wide = to_wide(name);
        // SAFETY: NUL-terminated name.
        unsafe { RegDeleteValueW(self.0, wide.as_ptr()) == ERROR_SUCCESS }
    }
}

/// Whether the key exists.
pub fn exists(root: Root, path: &str, view: View) -> bool {
    open(root, path, view, KEY_QUERY_VALUE).is_some()
}

/// `ExpandEnvironmentStringsW` (the text unchanged when it cannot be expanded).
fn expand(text: &str) -> String {
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    let wide = to_wide(text);
    let mut buf = vec![0u16; 4096];
    // SAFETY: NUL-terminated input and a buffer of the stated length.
    let n = unsafe { ExpandEnvironmentStringsW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) }
        as usize;
    if n == 0 || n > buf.len() {
        text.to_string()
    } else {
        from_wide(&buf[..n])
    }
}
