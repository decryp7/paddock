//! Registering the service with the OS (ADR 0003). One adapter per platform behind the same three functions.

#[cfg(windows)]
pub mod win_session;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{dispatch, install, uninstall};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::{dispatch, install, uninstall};

// Compiled on every Unix so its pure parts are tested on the Mac too; used on Linux.
#[cfg(unix)]
pub mod linux;
#[cfg(target_os = "linux")]
pub use linux::{dispatch, install, uninstall};

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod other {
    use crate::error::Error;

    /// No service manager on this platform.
    pub fn dispatch() -> Result<(), Error> {
        Err(Error::Unsupported("Running as a service"))
    }

    /// No service manager on this platform.
    pub fn install() -> Result<(), Error> {
        Err(Error::Unsupported("Installing the service"))
    }

    /// No service manager on this platform.
    pub fn uninstall() -> Result<(), Error> {
        Err(Error::Unsupported("Removing the service"))
    }
}
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub use other::{dispatch, install, uninstall};
