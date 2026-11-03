//! Where the helper lives on this computer, read from the environment. (These two functions lived in
//! `dyno_platform::elevate` until the helper left the 1.0 build; the helper is their only user now.)

use dyno_core::helper::HelperHost;
use dyno_core::helper_paths::{HelperPaths, WindowsRoots};
use std::path::PathBuf;

/// The folders Windows names, from the environment (the usual defaults when a variable is missing).
pub fn windows_roots() -> WindowsRoots {
    let var = |name: &str, default: &str| std::env::var_os(name).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default));
    WindowsRoots {
        program_files: var("ProgramFiles", r"C:\Program Files"),
        program_data: var("ProgramData", r"C:\ProgramData"),
        system_root: var("SystemRoot", r"C:\Windows"),
    }
}

/// The system the helper runs on.
pub fn this_host() -> HelperHost {
    if cfg!(windows) {
        HelperHost::Windows
    } else if cfg!(target_os = "macos") {
        HelperHost::Macos
    } else {
        HelperHost::Linux
    }
}

/// Where the helper lives on this computer.
pub fn helper_paths() -> HelperPaths {
    HelperPaths::for_host(this_host(), &windows_roots())
}
