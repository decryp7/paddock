//! The PowerShell the helper runs as SYSTEM. One builder for every reader (thermal zones, memory modules): the program is the
//! one in the OS's system folder, never a bare name that the search path could resolve elsewhere, and `PSModulePath` is pinned to
//! `$PSHOME\Modules` so no module from a folder the service inherited (or a user can write) loads as SYSTEM (Dyno 1.1.1 WP-6).

use crate::runner::Tool;

/// The Windows PowerShell 5.1 folder below the system folder, where `powershell.exe` and its `Modules` are.
const POWERSHELL_FOLDER: &str = r"WindowsPowerShell\v1.0";

/// A `powershell.exe` run of `script` for `system_dir` (the system folder, `C:\Windows\System32` on a usual machine).
pub fn system_powershell(system_dir: &str, script: &str) -> Tool {
    let home = format!(r"{}\{POWERSHELL_FOLDER}", system_dir.trim_end_matches('\\'));
    Tool {
        program: format!(r"{home}\powershell.exe"),
        args: ["-NoProfile", "-NonInteractive", "-Command", script].map(String::from).to_vec(),
        env: vec![("PSModulePath".to_string(), format!(r"{home}\Modules"))],
    }
}

/// The system folder as the OS says it (`GetSystemDirectoryW`), not from an environment variable. `None` when the call fails.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn windows_system_directory() -> Option<String> {
    use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buffer = [0u16; 260];
    // SAFETY: the buffer is valid for its length; the call writes at most that many units and returns the count written.
    let n = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    (n > 0 && n < buffer.len()).then(|| String::from_utf16_lossy(&buffer[..n]))
}

/// Not Windows: there is no system folder to name (the Windows readers are never used here).
#[cfg(not(windows))]
pub fn windows_system_directory() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_and_the_module_path_come_from_the_system_folder() {
        let t = system_powershell(r"E:\Win\System32\", "Get-Date");
        assert_eq!(t.program, r"E:\Win\System32\WindowsPowerShell\v1.0\powershell.exe");
        assert_eq!(t.args, ["-NoProfile", "-NonInteractive", "-Command", "Get-Date"]);
        assert_eq!(
            t.env,
            [(
                "PSModulePath".to_string(),
                r"E:\Win\System32\WindowsPowerShell\v1.0\Modules".to_string()
            )]
        );
    }
}
