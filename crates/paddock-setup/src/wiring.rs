//! The app's side of the per-machine Windows install (family ADR 0053 section 4), for every app that adopts it: what the
//! updater's hand-off to `$INSTDIR\paddock-setup.exe apply-update` needs to know of this crate. `paddock-updater` cannot depend
//! on `paddock-setup` (this crate depends on the updater for the signature check), so the app wires these in: the
//! [`SetupContract`] (which path `apply-update` accepts, the sentence of each exit code), the installer lock and the install
//! folder. One implementation for every app (rule 55); the app adds only `paddock_updater::tauri`'s elevator and administrator
//! check, which need the Tauri adapter this crate does not link.

use paddock_updater::SetupContract;

/// What `paddock-setup` says about the download and its exit codes.
pub struct SetupFacts;

/// The folders of `dir` above the file, nearest last, as `C:\a`, `C:\a\b`: the ones a junction could be hiding in.
fn folders_above(file: &str) -> Vec<String> {
    crate::checkdir::ancestors(file)
}

/// Refuses a download that has a link or junction anywhere above it (the elevated program refuses it too; saying so before
/// the Windows prompt saves a prompt that can only end in a refusal). `is_reparse` is the platform's look at one folder.
pub fn reparse_above(file: &str, is_reparse: &dyn Fn(&str) -> bool) -> Result<(), String> {
    match folders_above(file).iter().find(|f| is_reparse(f)) {
        Some(_) => Err("a folder above the download is a link or junction".into()),
        None => Ok(()),
    }
}

/// The platform's look at one folder: a reparse point (a junction or symbolic link).
#[cfg(windows)]
fn is_reparse_point(folder: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    std::fs::symlink_metadata(folder)
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(not(windows))]
fn is_reparse_point(folder: &str) -> bool {
    std::fs::symlink_metadata(folder).is_ok_and(|m| m.file_type().is_symlink())
}

/// Whether `dir` is a folder the elevated program may be run from, over the facts of the machine: the same rule as the installer's
/// `check-dir` (a direct child of Program Files, no link above it, owned and protected by administrators).
pub fn install_dir_ok(facts: &dyn crate::checkdir::DirFacts, dir: &str) -> Result<(), String> {
    if crate::localpath::LocalPath::parse(dir).is_err() {
        return Err("the install folder is not an absolute drive-letter path".into());
    }
    crate::checkdir::check(facts, dir)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

impl SetupContract for SetupFacts {
    #[cfg(windows)]
    fn check_install_dir(&self, dir: &str) -> Result<(), String> {
        install_dir_ok(&crate::native::RealDirs, dir)
    }

    #[cfg(not(windows))]
    fn check_install_dir(&self, _dir: &str) -> Result<(), String> {
        Err("per-machine installs exist on Windows only".into())
    }

    fn check_download(&self, path: &str) -> Result<(), String> {
        crate::localpath::LocalPath::parse(path).map_err(|e| e.to_string())?;
        reparse_above(path, &is_reparse_point)
    }

    /// Only `apply-update`'s own codes: usage (4), failed (5) and its refusals (20 to 31). Other codes belong to other commands
    /// of the same program or to the installer's error levels (2 to 5 overlap), so an `apply-update` that returns one of them
    /// has an unknown outcome and the next start settles it.
    fn sentence(&self, code: i32) -> Option<String> {
        use crate::exit::Exit;
        let exit = match code {
            4 => Exit::Usage,
            5 => Exit::Failed,
            c => Exit::Refused(crate::apply::Refusal::from_code(c)?),
        };
        Some(as_sentence(exit.sentence()))
    }
}

/// The program's fragment as a sentence: a capital first letter and a full stop.
fn as_sentence(text: &str) -> String {
    let mut chars = text.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    let mut s = format!("{first}{}", chars.as_str());
    if !s.ends_with(['.', '!', '?']) {
        s.push('.');
    }
    s
}

#[cfg(windows)]
pub use windows::{SetupLock, install_dir};

#[cfg(windows)]
mod windows {
    use paddock_updater::InstallerLock;
    use std::path::PathBuf;
    use std::time::Duration;

    /// The installer lock of this app (`crate::installer_probe`, `wait_for_installer`).
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
            crate::installer_probe(&self.identifier) == crate::handoff::SignalProbe::Active
        }

        fn wait(&self, bound: Duration) -> bool {
            crate::wait_for_installer(&self.identifier, bound)
                == crate::handoff::InstallerState::Gone
        }
    }

    /// The folder of the running executable: `$INSTDIR`, where `paddock-setup.exe` is.
    pub fn install_dir() -> Option<PathBuf> {
        std::env::current_exe().ok()?.parent().map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_apply_update_refusal_has_a_sentence_for_the_failed_state() {
        for code in 20..=31 {
            let s = SetupFacts
                .sentence(code)
                .unwrap_or_else(|| panic!("no sentence for exit {code}"));
            assert!(
                s.ends_with('.') && s.chars().next().unwrap().is_uppercase(),
                "{code}: {s}"
            );
        }
        assert_eq!(
            SetupFacts.sentence(26).as_deref(),
            Some("The downloaded update did not pass the signature check. Download it again.")
        );
        assert_eq!(
            SetupFacts.sentence(31).unwrap(),
            crate::apply::Refusal::InstallerEnded.sentence()
        );
        assert_eq!(SetupFacts.sentence(0), None, "success needs no sentence");
        assert_eq!(SetupFacts.sentence(99), None);
    }

    #[test]
    fn only_apply_updates_own_codes_have_sentences_the_others_are_unknown() {
        assert_eq!(
            SetupFacts.sentence(4).unwrap(),
            "The setup program was started with wrong arguments."
        );
        assert_eq!(
            SetupFacts.sentence(5).unwrap(),
            "The setup program failed; the reason is in setup.log."
        );
        // 2, 3, 11 and 12 belong to guard, check-dir, stop-all and check-version, and 2 to 5 are also the installer's error levels.
        for code in [1, 2, 3, 11, 12, 32, -1] {
            assert_eq!(SetupFacts.sentence(code), None, "exit {code}");
        }
    }

    struct Facts {
        roots: Vec<String>,
        canonical: String,
        reparse: bool,
        protection: Result<(), String>,
    }

    impl crate::checkdir::DirFacts for Facts {
        fn program_files(&self) -> Vec<String> {
            self.roots.clone()
        }
        fn inspect(&self, _dir: &str) -> Result<crate::checkdir::DirInspect, String> {
            Ok(crate::checkdir::DirInspect {
                canonical: self.canonical.clone(),
                exists: true,
                reparse_component: self.reparse,
                owner_sid: Some("S-1-5-32-544".into()),
            })
        }
        fn protection(&self, _i: &crate::checkdir::DirInspect) -> Result<(), String> {
            self.protection.clone()
        }
    }

    fn facts(canonical: &str) -> Facts {
        Facts {
            roots: vec![r"C:\Program Files".into()],
            canonical: canonical.into(),
            reparse: false,
            protection: Ok(()),
        }
    }

    #[test]
    fn the_install_folder_must_pass_the_program_files_rule_before_any_prompt() {
        assert!(install_dir_ok(&facts(r"C:\Program Files\App"), r"C:\Program Files\App").is_ok());
        // No folder, a relative name (what `unwrap_or_default` used to make of "no install dir"), or another place.
        for dir in ["", "App", r"relative\App", r"\\server\share\App"] {
            assert!(
                install_dir_ok(&facts(r"C:\Program Files\App"), dir).is_err(),
                "`{dir}`"
            );
        }
        assert!(install_dir_ok(&facts(r"C:\Users\Ann\App"), r"C:\Users\Ann\App").is_err());
        let mut linked = facts(r"C:\Program Files\App");
        linked.reparse = true;
        assert!(install_dir_ok(&linked, r"C:\Program Files\App").is_err());
        let mut loose = facts(r"C:\Program Files\App");
        loose.protection = Err("Users can change it".into());
        assert!(
            install_dir_ok(&loose, r"C:\Program Files\App")
                .unwrap_err()
                .contains("Users can change it")
        );
    }

    #[test]
    fn a_junction_anywhere_above_the_download_is_refused_before_the_prompt() {
        let file = r"C:\Users\Ann\AppData\Local\net.test.app\updates\update-1.1.0-x-setup.exe";
        assert!(reparse_above(file, &|_| false).is_ok());
        let e = reparse_above(file, &|f| f == r"C:\Users\Ann\AppData").unwrap_err();
        assert!(e.contains("link or junction"), "{e}");
        assert!(
            reparse_above(file, &|f| f == r"C:\Users").is_err(),
            "also high up"
        );
    }

    #[test]
    fn the_download_must_be_a_plain_local_drive_letter_path() {
        assert!(SetupFacts
            .check_download(r"C:\Users\Ann\AppData\Local\net.decryptology.app\updates\update-1.1.0-x-setup.exe")
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
