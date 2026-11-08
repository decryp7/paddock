//! Dyno's side of the per-machine Windows install (family ADR 0053 section 4): the updater's hand-off to
//! `$INSTDIR\paddock-setup.exe apply-update`. The contract, the installer lock and the install folder are
//! `paddock_setup::wiring`, one implementation for every app that adopts the install (rule 55); only the administrator check,
//! which needs the Tauri adapter of `paddock-updater`, is here.

#[cfg(windows)]
pub use paddock_setup::wiring::{install_dir, SetupLock};
pub use paddock_setup::wiring::{install_dir_ok, reparse_above, SetupFacts};

/// Whether this account is an administrator (also with a filtered token).
#[cfg(windows)]
pub fn administrator() -> bool {
    paddock_updater::tauri::is_administrator()
}
