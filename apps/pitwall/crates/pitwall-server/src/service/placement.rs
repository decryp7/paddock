//! Where the program of Pitwall's Windows service may be (plan B6, ADR 0053 §1 and §5). The service runs as LocalSystem, so
//! the program it starts must be one no other account can replace: inside a folder directly under Program Files, and that
//! folder and the program protected as `paddock-service`'s protected-chain rule says (no account but SYSTEM, Administrators
//! or TrustedInstaller may write, delete or relink anything on the way). A registration of another copy is refused, not
//! moved (ADR 0053 §5 and "no migration before v1"): the user uninstalls Pitwall and installs it again.
//!
//! The decisions are pure over `paddock-service`'s ports (the folders Windows names, the security of each path component), so
//! they are tested on every OS with fake folders and a fake service manager; `windows.rs` passes the real ones. This stays as
//! defence in depth once the installer itself checks the folder (ADR 0053's include, plan B6b).

use paddock_service::privileged::trust::{self, Style, TrustPort};
use paddock_service::privileged::{FileKind, KnownFolders};
use std::fmt;

/// Why `pitwall-server install` refused, with what the user does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Windows did not name Program Files: nothing is registered from a guessed place.
    NoProgramFiles(String),
    /// The program is not in a folder directly under Program Files.
    OutsideProgramFiles {
        /// The program.
        program: String,
        /// Program Files, as Windows names it.
        program_files: String,
    },
    /// The program or its folder can be changed by an account other than SYSTEM, Administrators or TrustedInstaller.
    NotProtected {
        /// What the protected-chain rule found, in words.
        why: String,
    },
    /// The service is registered for another program (another copy, or a copy moved since).
    OtherCopy {
        /// The program the registration starts.
        registered: String,
        /// This program.
        program: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::NoProgramFiles(why) => write!(
                f,
                "Windows did not say where Program Files is ({why}), so the Pitwall service was not registered. Restart the computer and install Pitwall again."
            ),
            Refusal::OutsideProgramFiles { program, program_files } => write!(
                f,
                "The Pitwall service runs as the system, so Pitwall must be installed in {program_files}; it is in {program}. Uninstall Pitwall, then install it again and keep the suggested folder."
            ),
            Refusal::NotProtected { why } => write!(
                f,
                "The Pitwall service was not registered: {why}. Uninstall Pitwall, then install it again into Program Files."
            ),
            Refusal::OtherCopy { registered, program } => write!(
                f,
                "The Pitwall service is registered for another copy ({registered}), not this one ({program}). Uninstall Pitwall (Settings > Apps), then install it again."
            ),
        }
    }
}

impl std::error::Error for Refusal {}

/// What the service manager registers: the program and its arguments (the service's ImagePath is built from exactly these).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    /// The program, as checked by [`check_program`].
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

/// The registration of `program`: what [`check_program`] accepted, run with `--service`.
pub fn registration_for(program: &str) -> Registration {
    Registration {
        program: program.to_string(),
        args: vec!["--service".to_string()],
    }
}

/// Whether `program` may be what a LocalSystem service starts: in a folder directly under Program Files (as `folders` names it,
/// never the environment), and protected all the way (`trust`'s rule, with the program's folder as the protected root).
///
/// # Errors
/// The [`Refusal`] that says what to do.
pub fn check_program(program: &str, folders: &dyn KnownFolders, port: &mut dyn TrustPort) -> Result<(), Refusal> {
    let program_files = folders
        .program_files()
        .map_err(|e| Refusal::NoProgramFiles(e.to_string()))?
        .to_string_lossy()
        .into_owned();
    let outside = || Refusal::OutsideProgramFiles {
        program: program.to_string(),
        program_files: program_files.clone(),
    };
    // The program's folder must be a direct child of Program Files (`C:\Program Files\Pitwall`).
    let folder = trust::parent_of(program, Style::Windows).ok_or_else(outside)?;
    let parent = trust::parent_of(&folder, Style::Windows).ok_or_else(outside)?;
    if !trust::is_within(&parent, &program_files, Style::Windows) || !trust::is_within(&program_files, &parent, Style::Windows) {
        return Err(outside());
    }
    trust::verify_chain(port, program, FileKind::File, Some(&folder), false).map_err(|u| Refusal::NotProtected { why: u.describe() })
}

/// The program an ImagePath (`"C:\\Program Files\\Pitwall\\pitwall-server.exe" --service`, as the service manager holds it) starts.
pub fn program_of_image_path(image_path: &str) -> Option<String> {
    // The shared reader of Windows command lines (quotes, arguments), the one `sc qc` and the `Run` values use.
    paddock_service::render::windows_program_of(image_path)
}

/// Whether a service already registered for `registered` (the program its ImagePath starts; `None`: no registration) may be
/// left to this `program`. The same program passes (a reinstall in place); another one is refused.
///
/// # Errors
/// [`Refusal::OtherCopy`].
pub fn check_existing(registered: Option<&str>, program: &str) -> Result<(), Refusal> {
    match registered {
        Some(r) if !trust::is_within(r, program, Style::Windows) || !trust::is_within(program, r, Style::Windows) => {
            Err(Refusal::OtherCopy {
                registered: r.to_string(),
                program: program.to_string(),
            })
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::privileged::trust_fake::{file, from_sddl, stock_drive_root, FakePort, SDDL_PROGRAM_FILES};
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;

    const PF: &str = "C:\\Program Files";
    const DIR: &str = "C:\\Program Files\\Pitwall";
    const EXE: &str = "C:\\Program Files\\Pitwall\\pitwall-server.exe";
    /// The folder the elevated installer makes: owner Administrators, SYSTEM and Administrators full, Users read and run.
    const SDDL_INSTALLED: &str = "O:BAG:S-1-5-21-1-2-513D:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIID;0x1200a9;;;BU)";
    /// A folder Users may write (Users modify, as a folder made by a standard user or opened up later).
    const SDDL_USER_WRITABLE: &str = "O:BAG:S-1-5-21-1-2-513D:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIID;0x1301bf;;;BU)";

    /// The folders Windows names, as a test says.
    struct Folders(Option<&'static str>);

    impl KnownFolders for Folders {
        fn system_directory(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("C:\\Windows\\System32"))
        }
        fn program_files(&self) -> io::Result<PathBuf> {
            self.0
                .map(PathBuf::from)
                .ok_or_else(|| io::Error::other("FOLDERID_ProgramFiles failed"))
        }
        fn program_data(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("C:\\ProgramData"))
        }
    }

    /// A machine with stock `C:\` and Program Files, and `extra` folders and files with their security.
    fn machine(extra: &[(&str, &str, bool)]) -> FakePort {
        let mut p = FakePort {
            style: Style::Windows,
            nodes: HashMap::new(),
            links: HashMap::new(),
            calls: vec![],
        };
        p.put("C:\\", stock_drive_root())
            .put(PF, from_sddl(FileKind::Dir, SDDL_PROGRAM_FILES));
        for (path, sddl, is_file) in extra {
            let node = from_sddl(FileKind::Dir, sddl);
            p.put(path, if *is_file { file(node) } else { node });
        }
        p
    }

    fn installed() -> FakePort {
        machine(&[(DIR, SDDL_INSTALLED, false), (EXE, SDDL_INSTALLED, true)])
    }

    #[test]
    fn a_program_in_its_protected_folder_under_program_files_is_accepted() {
        assert_eq!(check_program(EXE, &Folders(Some(PF)), &mut installed()), Ok(()));
        // Windows paths compare without case.
        assert_eq!(check_program(&EXE.to_uppercase(), &Folders(Some(PF)), &mut installed()), Ok(()));
    }

    #[test]
    fn a_program_outside_program_files_is_refused_with_what_to_do() {
        let profile = "C:\\Users\\sam\\AppData\\Local\\Pitwall\\pitwall-server.exe";
        let mut port = machine(&[
            ("C:\\Users", SDDL_INSTALLED, false),
            ("C:\\Users\\sam", SDDL_USER_WRITABLE, false),
            ("C:\\Users\\sam\\AppData", SDDL_USER_WRITABLE, false),
            ("C:\\Users\\sam\\AppData\\Local", SDDL_USER_WRITABLE, false),
            ("C:\\Users\\sam\\AppData\\Local\\Pitwall", SDDL_USER_WRITABLE, false),
            (profile, SDDL_USER_WRITABLE, true),
        ]);
        let e = check_program(profile, &Folders(Some(PF)), &mut port).unwrap_err();
        assert!(matches!(e, Refusal::OutsideProgramFiles { .. }), "{e:?}");
        let shown = e.to_string();
        assert!(
            shown.contains("must be installed in C:\\Program Files") && shown.contains("Uninstall Pitwall, then install it again"),
            "{shown}"
        );
        // Program Files itself, a folder deeper down, and a name that only starts like it are not "directly under".
        for not_direct in [
            "C:\\Program Files\\pitwall-server.exe",
            "C:\\Program Files\\Vendor\\Pitwall\\pitwall-server.exe",
            "C:\\Program Files Evil\\Pitwall\\pitwall-server.exe",
            "pitwall-server.exe",
        ] {
            let e = check_program(not_direct, &Folders(Some(PF)), &mut installed()).unwrap_err();
            assert!(matches!(e, Refusal::OutsideProgramFiles { .. }), "{not_direct}: {e:?}");
        }
    }

    #[test]
    fn a_folder_or_program_users_can_write_is_refused_even_under_program_files() {
        for (dir_sddl, exe_sddl) in [(SDDL_USER_WRITABLE, SDDL_INSTALLED), (SDDL_INSTALLED, SDDL_USER_WRITABLE)] {
            let mut port = machine(&[(DIR, dir_sddl, false), (EXE, exe_sddl, true)]);
            let e = check_program(EXE, &Folders(Some(PF)), &mut port).unwrap_err();
            assert!(matches!(e, Refusal::NotProtected { .. }), "{e:?}");
            let shown = e.to_string();
            assert!(
                shown.contains("S-1-5-32-545") && shown.contains("install it again into Program Files"),
                "{shown}"
            );
        }
        // A program that is not there, and a folder that is a link, are refused too (nothing is assumed).
        let mut port = machine(&[(DIR, SDDL_INSTALLED, false)]);
        assert!(matches!(
            check_program(EXE, &Folders(Some(PF)), &mut port),
            Err(Refusal::NotProtected { .. })
        ));
    }

    #[test]
    fn without_program_files_from_windows_nothing_is_registered() {
        let e = check_program(EXE, &Folders(None), &mut installed()).unwrap_err();
        assert!(matches!(e, Refusal::NoProgramFiles(_)), "{e:?}");
        assert!(e.to_string().contains("FOLDERID_ProgramFiles failed"));
    }

    #[test]
    fn the_registration_starts_exactly_the_checked_program() {
        let r = registration_for(EXE);
        assert_eq!(
            r,
            Registration {
                program: EXE.to_string(),
                args: vec!["--service".to_string()]
            }
        );
    }

    #[test]
    fn an_image_path_with_quotes_and_arguments_is_read_as_its_program() {
        assert_eq!(program_of_image_path(&format!("\"{EXE}\" --service")).as_deref(), Some(EXE));
        assert_eq!(
            program_of_image_path("D:\\Tools\\pitwall-server.exe --service").as_deref(),
            Some("D:\\Tools\\pitwall-server.exe")
        );
        // What the service manager holds for this program passes the check against it.
        let held = program_of_image_path(&format!("\"{EXE}\" --service"));
        assert_eq!(check_existing(held.as_deref(), EXE), Ok(()));
    }

    #[test]
    fn a_registration_of_another_copy_is_refused_and_the_same_copy_passes() {
        assert_eq!(check_existing(None, EXE), Ok(()));
        assert_eq!(check_existing(Some(EXE), EXE), Ok(()));
        assert_eq!(
            check_existing(Some(&EXE.to_lowercase()), EXE),
            Ok(()),
            "Windows paths compare without case"
        );
        let old = "D:\\Tools\\Pitwall\\pitwall-server.exe";
        let e = check_existing(Some(old), EXE).unwrap_err();
        assert_eq!(
            e,
            Refusal::OtherCopy {
                registered: old.into(),
                program: EXE.into()
            }
        );
        let shown = e.to_string();
        assert!(
            shown.contains(old) && shown.contains("Uninstall Pitwall (Settings > Apps), then install it again"),
            "{shown}"
        );
    }
}
