//! A program built for the Windows GUI subsystem (no console window when a Run entry, a service or the app starts it)
//! that must still print when a user runs it from a terminal (`dyno-agent --version`): it attaches to the parent's
//! console and points any missing standard handle at it.
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]
#![allow(unsafe_code)]

/// Whether a standard handle value (as `GetStdHandle` returns it) is unusable and must be pointed at the console: none
/// (null, a GUI-subsystem program started without redirection) or invalid (-1).
pub fn handle_missing(handle: isize) -> bool {
    handle == 0 || handle == -1
}

/// Which of the three standard handles (input, output, error), given as `GetStdHandle` returns them, need pointing at the
/// console.
pub fn handles_to_reopen(handles: [isize; 3]) -> [bool; 3] {
    handles.map(handle_missing)
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
        let kinds = [
            (STD_INPUT_HANDLE, &conin, GENERIC_READ),
            (STD_OUTPUT_HANDLE, &conout, GENERIC_WRITE),
            (STD_ERROR_HANDLE, &conout, GENERIC_WRITE),
        ];
        let reopen = handles_to_reopen(kinds.map(|(which, _, _)| GetStdHandle(which) as isize));
        for ((which, name, access), reopen) in kinds.into_iter().zip(reopen) {
            if reopen {
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
    fn only_the_missing_standard_handles_are_reopened() {
        // A redirected output (a valid handle) is kept; the other two have none (a GUI program started from a terminal).
        assert_eq!(handles_to_reopen([0, 0x1c4, -1]), [true, false, true]);
        assert_eq!(handles_to_reopen([0x10, 0x20, 0x30]), [false, false, false]);
    }

    #[cfg(not(windows))]
    #[test]
    fn there_is_no_console_to_attach_to_outside_windows() {
        assert!(!attach_parent_console());
    }
}
