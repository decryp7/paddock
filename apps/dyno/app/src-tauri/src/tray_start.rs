//! How this start began, told once on the first tray line (ER-27): whether the program runs from a per-machine or a per-user
//! folder, whether `--hidden` was given, and how long Explorer has been running. When icons are missing after a login start, the
//! log then shows whether Dyno came up before the shell did. Never the path, never the user name.

use std::path::{Path, PathBuf};

/// Where the running program lives, in words that carry no user name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExeKind {
    /// Under Program Files: an all-users install.
    PerMachine,
    /// Under the user's local application data: a one-user install.
    PerUser,
    /// Anywhere else (a build folder, a copy).
    Other,
}

impl ExeKind {
    /// The word for the log.
    pub fn describe(self) -> &'static str {
        match self {
            ExeKind::PerMachine => "per-machine",
            ExeKind::PerUser => "per-user",
            ExeKind::Other => "other",
        }
    }
}

/// Classifies `exe` against the Program Files folders and the local application data folder (case-insensitive, as Windows is).
pub fn exe_kind(exe: &Path, program_files: &[PathBuf], local_app_data: Option<&Path>) -> ExeKind {
    let under = |dir: &Path| {
        let (e, d) = (exe.to_string_lossy().to_lowercase(), dir.to_string_lossy().to_lowercase());
        let d = d.trim_end_matches(['\\', '/']);
        !d.is_empty() && e.len() > d.len() && e.starts_with(d) && matches!(e.as_bytes()[d.len()], b'\\' | b'/')
    };
    if program_files.iter().any(|p| under(p)) {
        ExeKind::PerMachine
    } else if local_app_data.is_some_and(under) {
        ExeKind::PerUser
    } else {
        ExeKind::Other
    }
}

/// The line: fixed words and numbers only.
pub fn describe(kind: ExeKind, hidden: bool, explorer_uptime_s: Option<u64>) -> String {
    let explorer = match explorer_uptime_s {
        Some(s) => format!("{s} s"),
        None => "not running or unreadable".to_string(),
    };
    format!(
        "start: program {}, --hidden {}, Explorer up {explorer}",
        kind.describe(),
        if hidden { "given" } else { "not given" }
    )
}

/// Logs the start context once per process (the first tray sync); later calls do nothing.
pub fn log_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let hidden = std::env::args().skip(1).any(|a| a == paddock_window::launch::HIDDEN_FLAG);
        let exe = std::env::current_exe().unwrap_or_default();
        let pf: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let kind = exe_kind(&exe, &pf, local.as_deref());
        log::info!("{}", describe(kind, hidden, explorer_uptime_s()));
    });
}

#[cfg(windows)]
mod win {
    //! Toolhelp and process times: plain queries on handles opened and closed here.
    #![allow(unsafe_code)]

    /// Seconds since `explorer.exe` of this session started; `None` when it is not running or cannot be read.
    pub fn explorer_uptime_s() -> Option<u64> {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
        };
        use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
        use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let ticks = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
        // SAFETY: plain Win32 calls on handles opened and closed here; the entry struct is zeroed with its size set as the API requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut oldest: Option<u64> = None;
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                let name_len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..name_len]).eq_ignore_ascii_case("explorer.exe") {
                    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                    if !process.is_null() {
                        let zero = FILETIME {
                            dwLowDateTime: 0,
                            dwHighDateTime: 0,
                        };
                        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
                        if GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0 {
                            let c = ticks(created);
                            oldest = Some(oldest.map_or(c, |o| o.min(c)));
                        }
                        CloseHandle(process);
                    }
                }
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
            let mut now = zero_filetime();
            GetSystemTimeAsFileTime(&mut now);
            // FILETIME counts 100 ns; an unreadable or future creation time is `None`, not a wrapped number.
            ticks(now).checked_sub(oldest?).map(|t| t / 10_000_000)
        }
    }

    fn zero_filetime() -> windows_sys::Win32::Foundation::FILETIME {
        windows_sys::Win32::Foundation::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }
    }
}

#[cfg(windows)]
use win::explorer_uptime_s;

#[cfg(not(windows))]
fn explorer_uptime_s() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pf() -> Vec<PathBuf> {
        vec![PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\Program Files (x86)")]
    }

    #[test]
    fn the_install_kind_comes_from_the_folder_never_from_the_user_name() {
        let local = PathBuf::from(r"C:\Users\someone\AppData\Local");
        let kind = |p: &str| exe_kind(Path::new(p), &pf(), Some(&local));
        assert_eq!(kind(r"C:\Program Files\Dyno\dyno-app.exe"), ExeKind::PerMachine);
        assert_eq!(kind(r"c:\program files (x86)\Dyno\dyno-app.exe"), ExeKind::PerMachine);
        assert_eq!(kind(r"C:\Users\someone\AppData\Local\Programs\Dyno\dyno-app.exe"), ExeKind::PerUser);
        assert_eq!(
            kind(r"C:\Program Files Extra\x.exe"),
            ExeKind::Other,
            "a longer folder name is not Program Files"
        );
        assert_eq!(kind(r"D:\build\dyno-app.exe"), ExeKind::Other);
        assert_eq!(exe_kind(Path::new("x.exe"), &[], None), ExeKind::Other);
    }

    #[test]
    fn the_line_has_fixed_words_and_no_path() {
        let line = describe(ExeKind::PerUser, true, Some(42));
        assert_eq!(line, "start: program per-user, --hidden given, Explorer up 42 s");
        assert!(describe(ExeKind::PerMachine, false, None).contains("--hidden not given, Explorer up not running or unreadable"));
    }
}
