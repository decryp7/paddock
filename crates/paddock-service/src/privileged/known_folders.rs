//! The folders an elevated process may trust, taken from Windows itself and never from the environment.
//!
//! A UAC-elevated process can inherit the user-writable `HKCU\Environment` (the known `windir` bypass): `%SystemRoot%`,
//! `%windir%` and `%ProgramFiles%` are then whatever the user wrote there. A SYSTEM-level install that builds
//! `%SystemRoot%\System32\icacls.exe` or `%ProgramFiles%\Dyno Helper` from them runs the user's program as administrator and
//! creates its "protected" folder where the user chose. So elevated code asks the OS: `GetSystemDirectoryW` for the system
//! folder and `SHGetKnownFolderPath` for Program Files and ProgramData (both read machine-wide registry values), through
//! the [`KnownFolders`] port, so a test shows that a poisoned environment is not used (ER-27: every answer is logged).
#![cfg_attr(windows, allow(unsafe_code))]
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use std::io;
use std::path::PathBuf;

/// The folders Windows names, as the OS reports them.
pub trait KnownFolders: Send + Sync {
    /// `C:\Windows\System32` (`GetSystemDirectoryW`).
    ///
    /// # Errors
    /// The OS could not say; the caller fails closed.
    fn system_directory(&self) -> io::Result<PathBuf>;
    /// `C:\Program Files` (`FOLDERID_ProgramFiles`).
    ///
    /// # Errors
    /// The OS could not say; the caller fails closed.
    fn program_files(&self) -> io::Result<PathBuf>;
    /// `C:\ProgramData` (`FOLDERID_ProgramData`).
    ///
    /// # Errors
    /// The OS could not say; the caller fails closed.
    fn program_data(&self) -> io::Result<PathBuf>;
}

/// The Windows system tools an elevated install runs, by the bare name they are called with.
const SYSTEM_TOOLS: [&str; 3] = ["icacls", "reg", "sc"];

/// A bare name of a Windows system tool as its absolute path under the system folder `folders` names (so a program planted
/// in the working folder or on `PATH`, or reached through a poisoned `%SystemRoot%`, is never the one a SYSTEM process
/// runs). Other programs, and every program when `windows` is false, are returned as given.
///
/// # Errors
/// The system folder could not be determined: the tool is not run.
pub fn system_tool_in(
    folders: &dyn KnownFolders,
    program: &str,
    windows: bool,
) -> io::Result<String> {
    if windows && SYSTEM_TOOLS.iter().any(|t| program.eq_ignore_ascii_case(t)) {
        let dir = folders.system_directory()?;
        let dir = dir.to_string_lossy();
        return Ok(format!(
            "{}\\{}.exe",
            dir.trim_end_matches(['\\', '/']),
            program.to_ascii_lowercase()
        ));
    }
    Ok(program.to_string())
}

/// [`system_tool_in`] over the real OS.
///
/// # Errors
/// The system folder could not be determined.
pub fn system_tool(program: &str) -> io::Result<String> {
    system_tool_in(&OsKnownFolders, program, cfg!(windows))
}

/// The OS itself. Off Windows there are no such folders and the conventional names are returned (nothing there uses them).
#[derive(Debug, Default, Clone, Copy)]
pub struct OsKnownFolders;

#[cfg(not(windows))]
impl KnownFolders for OsKnownFolders {
    fn system_directory(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("C:\\Windows\\System32"))
    }
    fn program_files(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("C:\\Program Files"))
    }
    fn program_data(&self) -> io::Result<PathBuf> {
        Ok(PathBuf::from("C:\\ProgramData"))
    }
}

#[cfg(windows)]
mod os {
    //! Native glue (crates/README.md): covered by the Windows runner and the Windows test VM.
    use std::io;
    use std::path::PathBuf;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows::Win32::UI::Shell::{
        FOLDERID_ProgramData, FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    use windows::core::GUID;

    fn log_and_return(what: &str, result: io::Result<PathBuf>) -> io::Result<PathBuf> {
        match &result {
            Ok(p) => log::info!(folder = what; "{}: {}", what, p.display()),
            Err(e) => log::error!(folder = what, error:% = e; "the OS did not name the folder"),
        }
        result
    }

    pub fn system_directory() -> io::Result<PathBuf> {
        let mut buf = [0u16; 260];
        // SAFETY: the buffer is valid for its length; the call writes at most that many units.
        let n = unsafe { GetSystemDirectoryW(Some(&mut buf)) } as usize;
        let result = if n == 0 || n >= buf.len() {
            Err(io::Error::other("GetSystemDirectoryW failed"))
        } else {
            Ok(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
        };
        log_and_return("system directory", result)
    }

    fn known(id: &GUID, what: &str) -> io::Result<PathBuf> {
        // SAFETY: the returned block is read as a NUL-terminated wide string and freed once with CoTaskMemFree.
        let result = unsafe {
            SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)
                .map_err(|e| io::Error::other(format!("SHGetKnownFolderPath: {e}")))
                .and_then(|p| {
                    let text = p.to_string().map_err(io::Error::other);
                    CoTaskMemFree(Some(p.as_ptr().cast()));
                    text.map(PathBuf::from)
                })
        };
        log_and_return(what, result)
    }

    pub fn program_files() -> io::Result<PathBuf> {
        known(&FOLDERID_ProgramFiles, "Program Files")
    }

    pub fn program_data() -> io::Result<PathBuf> {
        known(&FOLDERID_ProgramData, "ProgramData")
    }
}

#[cfg(windows)]
impl KnownFolders for OsKnownFolders {
    fn system_directory(&self) -> io::Result<PathBuf> {
        os::system_directory()
    }
    fn program_files(&self) -> io::Result<PathBuf> {
        os::program_files()
    }
    fn program_data(&self) -> io::Result<PathBuf> {
        os::program_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        system: &'static str,
    }
    impl KnownFolders for Fake {
        fn system_directory(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from(self.system))
        }
        fn program_files(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("D:\\Real Program Files"))
        }
        fn program_data(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("D:\\RealData"))
        }
    }

    #[test]
    fn a_system_tool_comes_from_the_folder_the_os_names_never_from_the_environment() {
        let real = Fake {
            system: "D:\\RealWin\\System32",
        };
        for tool in ["icacls", "ICACLS", "reg", "sc"] {
            let p = system_tool_in(&real, tool, true).unwrap();
            assert_eq!(
                p,
                format!("D:\\RealWin\\System32\\{}.exe", tool.to_ascii_lowercase())
            );
        }
        // A trailing separator is not doubled.
        let slash = Fake {
            system: "D:\\RealWin\\System32\\",
        };
        assert_eq!(
            system_tool_in(&slash, "sc", true).unwrap(),
            "D:\\RealWin\\System32\\sc.exe"
        );
    }

    #[test]
    fn other_programs_and_other_systems_are_left_as_given() {
        let real = Fake { system: "X:\\W" };
        assert_eq!(system_tool_in(&real, "other", true).unwrap(), "other");
        assert_eq!(system_tool_in(&real, "icacls", false).unwrap(), "icacls");
        assert_eq!(system_tool("other").unwrap(), "other");
    }

    #[test]
    fn a_system_folder_the_os_cannot_name_stops_the_tool() {
        struct Broken;
        impl KnownFolders for Broken {
            fn system_directory(&self) -> io::Result<PathBuf> {
                Err(io::Error::other("no answer"))
            }
            fn program_files(&self) -> io::Result<PathBuf> {
                Err(io::Error::other("no answer"))
            }
            fn program_data(&self) -> io::Result<PathBuf> {
                Err(io::Error::other("no answer"))
            }
        }
        assert!(system_tool_in(&Broken, "icacls", true).is_err());
        assert_eq!(system_tool_in(&Broken, "x", true).unwrap(), "x");
    }
}
