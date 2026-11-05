//! Dyno's side of the per-machine Windows install (family ADR 0053 section 4): the updater's hand-off to
//! `$INSTDIR\paddock-setup.exe apply-update`. The contract (which path `apply-update` accepts, the sentence of each exit code) and
//! the installer lock come from `paddock-setup`, which `paddock-updater` cannot depend on (it depends on the updater for the
//! signature check), so the app wires them in here (rule 55: one implementation, in `paddock-setup`).

use paddock_updater::SetupContract;

/// What `paddock-setup` says about the download and its exit codes.
pub struct SetupFacts;

impl SetupContract for SetupFacts {
    fn check_download(&self, path: &str) -> Result<(), String> {
        paddock_setup::localpath::LocalPath::parse(path)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn sentence(&self, code: i32) -> Option<String> {
        let exit = paddock_setup::exit::Exit::from_code(code).filter(|e| *e != paddock_setup::exit::Exit::Ok)?;
        Some(as_sentence(exit.sentence()))
    }
}

/// The program's fragment as a sentence: a capital first letter and a full stop.
fn as_sentence(text: &str) -> String {
    let mut chars = text.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    let mut s = format!("{first}{}", chars.as_str());
    if !s.ends_with(['.', '!', '?']) {
        s.push('.');
    }
    s
}

#[cfg(windows)]
pub use windows::{administrator, install_dir, SetupLock};

#[cfg(windows)]
mod windows {
    use paddock_updater::InstallerLock;
    use std::path::PathBuf;
    use std::time::Duration;

    /// The installer lock of this app (`paddock_setup::installer_probe`, `wait_for_installer`).
    pub struct SetupLock {
        identifier: String,
    }

    impl SetupLock {
        /// The lock for the app with this bundle identifier.
        pub fn new(identifier: &str) -> SetupLock {
            SetupLock {
                identifier: identifier.to_string(),
            }
        }
    }

    impl InstallerLock for SetupLock {
        fn installing(&self) -> bool {
            paddock_setup::installer_probe(&self.identifier) == paddock_setup::handoff::SignalProbe::Active
        }

        fn wait(&self, bound: Duration) -> bool {
            paddock_setup::wait_for_installer(&self.identifier, bound) == paddock_setup::handoff::InstallerState::Gone
        }
    }

    /// The folder of the running executable: `$INSTDIR`, where `paddock-setup.exe` is.
    pub fn install_dir() -> Option<PathBuf> {
        std::env::current_exe().ok()?.parent().map(PathBuf::from)
    }

    /// Whether this account is an administrator (also with a filtered token).
    pub fn administrator() -> bool {
        paddock_updater::tauri::is_administrator()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_apply_update_refusal_has_a_sentence_for_the_failed_state() {
        for code in 20..=31 {
            let s = SetupFacts.sentence(code).unwrap_or_else(|| panic!("no sentence for exit {code}"));
            assert!(s.ends_with('.') && s.chars().next().unwrap().is_uppercase(), "{code}: {s}");
        }
        assert_eq!(
            SetupFacts.sentence(26).as_deref(),
            Some("The downloaded update did not pass the signature check. Download it again.")
        );
        assert_eq!(
            SetupFacts.sentence(31).unwrap(),
            paddock_setup::apply::Refusal::InstallerEnded.sentence()
        );
        assert_eq!(SetupFacts.sentence(0), None, "success needs no sentence");
        assert_eq!(SetupFacts.sentence(99), None);
    }

    #[test]
    fn the_tool_codes_read_as_sentences_too() {
        assert_eq!(
            SetupFacts.sentence(2).unwrap(),
            "An older copy installed for one user must be uninstalled first."
        );
        assert_eq!(
            SetupFacts.sentence(11).unwrap(),
            "A program of the app is still running for another user."
        );
    }

    #[test]
    fn the_download_must_be_a_plain_local_drive_letter_path() {
        assert!(SetupFacts
            .check_download(r"C:\Users\Ann\AppData\Local\net.decryptology.dyno\updates\update-1.1.0-x-setup.exe")
            .is_ok());
        for bad in [
            r"\\server\share\x.exe",
            r"\\?\C:\x.exe",
            r"\\.\pipe\x",
            r"C:\a\..\b.exe",
            r"relative\x.exe",
            r"C:\a\b.exe:stream",
        ] {
            assert!(SetupFacts.check_download(bad).is_err(), "{bad}");
        }
    }
}
