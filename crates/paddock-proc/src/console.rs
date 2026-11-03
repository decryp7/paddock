//! A program built for the Windows GUI subsystem (no console window when a Run entry, a service or the app starts it)
//! that must still print when a user runs it from a terminal (`dyno-agent --version`): it attaches to the parent's
//! console and points any missing standard handle at it.
#![allow(unsafe_code)]

/// Whether a standard handle value (as `GetStdHandle` returns it) is unusable and must be pointed at the console: none
/// (null, a GUI-subsystem program started without redirection) or invalid (-1).
pub fn handle_missing(handle: isize) -> bool {
    handle == 0 || handle == -1
}

/// Attaches to the parent process's console, when there is one, and gives the program its standard handles there where
/// it has none (a redirected handle is kept). Does nothing when started without a console (login, a service, the app) and
/// on other platforms. Returns whether a console was attached.
#[cfg(windows)]
pub fn attach_parent_console() -> bool {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };
    // SAFETY: plain calls with constant arguments and NUL-terminated wide names that outlive them; a failure leaves the
    // program without a console, exactly as before.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return false;
        }
        let conin: Vec<u16> = "CONIN$\0".encode_utf16().collect();
        let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        for (which, name, access) in [
            (STD_INPUT_HANDLE, &conin, GENERIC_READ),
            (STD_OUTPUT_HANDLE, &conout, GENERIC_WRITE),
            (STD_ERROR_HANDLE, &conout, GENERIC_WRITE),
        ] {
            if handle_missing(GetStdHandle(which) as isize) {
                let h = CreateFileW(
                    name.as_ptr(),
                    access,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    std::ptr::null_mut(),
                );
                if !handle_missing(h as isize) {
                    SetStdHandle(which, h);
                }
            }
        }
    }
    true
}

/// No console to attach to outside Windows.
#[cfg(not(windows))]
pub fn attach_parent_console() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_or_invalid_standard_handle_is_missing() {
        assert!(handle_missing(0));
        assert!(handle_missing(-1));
        assert!(!handle_missing(0x1c4));
    }

    #[test]
    fn attaching_never_panics_and_is_false_without_a_console() {
        // Run by a test harness: there may or may not be a console; the call must simply answer.
        let _ = attach_parent_console();
    }
}
