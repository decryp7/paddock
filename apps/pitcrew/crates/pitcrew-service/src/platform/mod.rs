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

#[cfg(not(any(windows, target_os = "macos")))]
mod other {
    use crate::error::Error;

    /// Linux support is planned (systemd user or system unit).
    pub fn dispatch() -> Result<(), Error> {
        Err(Error::Unsupported("Running as a service"))
    }

    /// Linux support is planned.
    pub fn install() -> Result<(), Error> {
        Err(Error::Unsupported("Installing the service"))
    }

    /// Linux support is planned.
    pub fn uninstall() -> Result<(), Error> {
        Err(Error::Unsupported("Removing the service"))
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
pub use other::{dispatch, install, uninstall};
