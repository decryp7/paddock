//! Where the helper lives on this computer, from the OS's known folders. (These two functions lived in
//! `dyno_platform::elevate` until the helper left the 1.0 build; the helper is their only user now.)

use dyno_core::helper::HelperHost;
use dyno_core::helper_paths::{HelperPaths, WindowsRoots};
use paddock_service::privileged::OsKnownFolders;

/// The folders Windows names, as the OS reports them (never the environment: see [`dyno_helper_service::windows_roots`]).
pub fn windows_roots() -> WindowsRoots {
    dyno_helper_service::windows_roots(&OsKnownFolders)
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
