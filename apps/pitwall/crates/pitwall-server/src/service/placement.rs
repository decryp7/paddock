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
    /// The path has `.` or `..` components or a `\\?\` or `\\.\` prefix: it is not looked at.
    OddPath(String),
    /// This very program is registered already (the installer removes it before it registers again).
    AlreadyInstalled,
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
            Refusal::OddPath(path) => write!(
                f,
                "The Pitwall service was not registered: the program's path {path} is not a plain folder path. Uninstall Pitwall, then install it again into Program Files."
            ),
            Refusal::AlreadyInstalled => f.write_str("Pitwall's service is installed already; uninstall it first"),
            Refusal::OtherCopy { registered, program } => write!(
                f,
                "The Pitwall service is registered for another copy ({registered}), not this one ({program}). Uninstall Pitwall (Settings > Apps), then install it again."
            ),
        }
    }
}

impl std::error::Error for Refusal {}

impl Refusal {
    /// The refusal for the service's log (ER-27): what was refused and why, with a program path only when it lies in Program
    /// Files (a path elsewhere can be a user's profile, which the log never holds).
    pub fn log_line(&self) -> String {
        match self {
            Refusal::NoProgramFiles(why) => format!("install refused: Windows did not name Program Files ({why})"),
            Refusal::OutsideProgramFiles { .. } => "install refused: the program is outside Program Files".into(),
            Refusal::NotProtected { why } => format!("install refused: not protected: {why}"),
            Refusal::OddPath(_) => r"install refused: the program's path has . or .. or a \\?\ prefix".into(),
            Refusal::AlreadyInstalled => "install refused: this program is registered already".into(),
            Refusal::OtherCopy { registered, program } => format!(
                "install refused: the service is registered for another program; registered={} this={program}",
                loggable(registered)
            ),
        }
    }
}

/// `path` when it lies in a Program Files folder, otherwise only that it does not.
fn loggable(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.contains(":\\program files\\") || lower.contains(":\\program files (x86)\\") {
        path.to_string()
    } else {
        "<outside Program Files>".into()
    }
}

/// The accounts a refusal names, in words (the protected-chain rule speaks in SIDs).
fn named_sids(text: &str) -> String {
    [
        ("S-1-5-32-545", "Users"),
        ("S-1-5-11", "Authenticated Users"),
        ("S-1-1-0", "Everyone"),
        ("S-1-5-32-546", "Guests"),
        ("S-1-5-4", "Interactive users"),
    ]
    .iter()
    .fold(text.to_string(), |t, (sid, name)| {
        t.replace(&format!("account {sid}"), &format!("account {name}")).replace(sid, name)
    })
}

/// What `pitwall-server install` does, decided before anything is changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Register the service.
    Create,
    /// Another copy is registered and it is not safe to run as the system (outside Program Files, changeable by others, or
    /// an unquoted path with a space): its registration is removed first, then this one is made. Removing is not migrating:
    /// nothing of the other copy is kept.
    ReplaceUnsafe {
        /// The program the old registration started.
        old: String,
    },
}

/// The plan for this program: what to register (the ImagePath, built with the shared command-line writer) and what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// The checked program.
    pub program: String,
    /// The ImagePath (`"<program>" --service`).
    pub image_path: String,
    /// What to do.
    pub plan: Plan,
}

/// The ImagePath of `program`: paddock-service's Windows command line, with `--service`.
pub fn windows_command_line_for(program: &str) -> String {
    let r = registration_for(program);
    paddock_service::render::windows_command_line(&r.program, &r.args)
}

/// How many entries of the program's folder are checked (the install holds a few dozen files).
pub const MAX_TREE: usize = 4096;

/// Decides the install of `program` given what the service manager holds (`registered_image_path`, `None` when the service
/// does not exist), over the folders Windows names and the security of each path (fakes in tests). Every check on this
/// program comes before anything about the old registration: an odd path, the folder directly under Program Files, the
/// protected chain, and every file already in the folder (a planted library would load as the system).
///
/// # Errors
/// The [`Refusal`] that says what to do.
pub fn plan_install(
    program: &str,
    registered_image_path: Option<&str>,
    folders: &dyn KnownFolders,
    port: &mut dyn TrustPort,
) -> Result<Install, Refusal> {
    if odd_path(program) {
        return Err(Refusal::OddPath(program.to_string()));
    }
    check_program(program, folders, port)?;
    let folder = trust::parent_of(program, Style::Windows).ok_or_else(|| Refusal::OddPath(program.to_string()))?;
    trust::verify_tree(port, &folder, None, MAX_TREE).map_err(|u| Refusal::NotProtected {
        why: named_sids(&u.describe()),
    })?;
    let plan = match registered_image_path.map(read_image_path) {
        None => Plan::Create,
        Some(None) => Plan::ReplaceUnsafe { old: String::new() },
        Some(Some((old, ambiguous))) => {
            if !ambiguous && same_path(&old, program) {
                return Err(Refusal::AlreadyInstalled);
            }
            let safe = !ambiguous && !odd_path(&old) && check_program(&old, folders, port).is_ok();
            if safe {
                return Err(Refusal::OtherCopy {
                    registered: old,
                    program: program.to_string(),
                });
            }
            Plan::ReplaceUnsafe { old }
        }
    };
    Ok(Install {
        program: program.to_string(),
        image_path: windows_command_line_for(program),
        plan,
    })
}

/// A path with `.` or `..` components, or a device or verbatim prefix (`\\?\`, `\\.\`), is not judged.
fn odd_path(path: &str) -> bool {
    path.starts_with("\\\\?\\") || path.starts_with("\\\\.\\") || path.split(['\\', '/']).any(|c| c == "." || c == "..")
}

/// What the read-back of a new registration means (ADR 0054 §1.1): `held` is the program the service manager now holds for
/// it (`None`: it could not be read). When it is not `program`, `remove` deletes the registration, and the words say whether
/// that worked.
///
/// # Errors
/// What happened, in words for the user.
pub fn read_back(held: Option<&str>, program: &str, remove: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    if held.is_some_and(|h| same_path(h, program)) {
        return Ok(());
    }
    Err(match remove() {
        Ok(()) => "The Pitwall service was not registered for this program as asked, so it was removed again. Uninstall Pitwall, then install it again.".into(),
        Err(why) => format!(
            "The Pitwall service was not registered for this program as asked, and it could not be removed ({why}). Remove the service named Pitwall in an administrator prompt (sc.exe delete Pitwall), then install Pitwall again."
        ),
    })
}

/// Whether two Windows paths name the same file (compared without case).
pub fn same_path(a: &str, b: &str) -> bool {
    trust::is_within(a, b, Style::Windows) && trust::is_within(b, a, Style::Windows)
}

/// The program an ImagePath starts and whether it is ambiguous: an unquoted path with a space, which the service manager
/// tries piece by piece (`C:\\Program.exe` first). The program of an unquoted path is read up to its `.exe`.
fn read_image_path(image_path: &str) -> Option<(String, bool)> {
    let s = image_path.trim_start();
    if s.starts_with('"') {
        return program_of_image_path(s).map(|p| (p, false));
    }
    let lower = s.to_ascii_lowercase();
    let end = lower.find(".exe").map_or_else(|| s.find([' ', '\t']).unwrap_or(s.len()), |i| i + 4);
    let program = s[..end].to_string();
    (!program.is_empty()).then(|| (program.clone(), program.contains(' ')))
}

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
    trust::verify_chain(port, program, FileKind::File, Some(&folder), false).map_err(|u| Refusal::NotProtected {
        why: named_sids(&u.describe()),
    })
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
                shown.contains("account Users") && shown.contains("install it again into Program Files"),
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

    /// Review M1: Users may add files to the folder (FILE_ADD_FILE without delete): a planted DLL would load as SYSTEM.
    #[test]
    fn a_folder_users_may_add_files_to_is_refused() {
        let add_file = "O:BAG:S-1-5-21-1-2-513D:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIID;0x1200af;;;BU)";
        let mut port = machine(&[(DIR, add_file, false), (EXE, SDDL_INSTALLED, true)]);
        assert!(matches!(
            check_program(EXE, &Folders(Some(PF)), &mut port),
            Err(Refusal::NotProtected { .. })
        ));
    }

    /// Review M3: a registration of a folder that holds this program, or of a path this one starts with, is another program.
    #[test]
    fn a_registration_that_is_only_a_prefix_or_a_parent_is_another_copy() {
        assert!(check_existing(Some(DIR), EXE).is_err());
        assert!(check_existing(Some(&format!("{EXE}.old")), EXE).is_err());
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

    /// A second protected install, in another folder under Program Files.
    const OTHER: &str = "C:\\Program Files\\Pitwall Old\\pitwall-server.exe";

    fn two_installs() -> FakePort {
        machine(&[
            (DIR, SDDL_INSTALLED, false),
            (EXE, SDDL_INSTALLED, true),
            ("C:\\Program Files\\Pitwall Old", SDDL_INSTALLED, false),
            (OTHER, SDDL_INSTALLED, true),
        ])
    }

    fn plan(registered: Option<&str>, port: &mut FakePort) -> Result<Install, Refusal> {
        plan_install(EXE, registered, &Folders(Some(PF)), port)
    }

    /// Review P2-2 and P2-5: the whole decision is one pure plan over a fake service manager's ImagePath.
    #[test]
    fn the_install_plan_creates_refuses_or_replaces_an_unsafe_copy() {
        let image = windows_command_line_for(EXE);
        // Nothing registered: create, with the shared command line, which reads back as this program.
        let p = plan(None, &mut two_installs()).unwrap();
        assert_eq!(
            (p.program.as_str(), p.image_path.as_str(), &p.plan),
            (EXE, image.as_str(), &Plan::Create)
        );
        assert_eq!(program_of_image_path(&p.image_path).as_deref(), Some(EXE), "the round trip");
        // This copy already: installed already (the installer removes it first).
        assert_eq!(plan(Some(&image), &mut two_installs()), Err(Refusal::AlreadyInstalled));
        // Another protected copy: refused, never moved.
        let other = windows_command_line_for(OTHER);
        assert!(matches!(plan(Some(&other), &mut two_installs()), Err(Refusal::OtherCopy { .. })));
        // Another copy that is not safe (outside Program Files, or writable): its registration is removed and this one made.
        let unsafe_copy = "\"D:\\Tools\\Pitwall\\pitwall-server.exe\" --service";
        let p = plan(Some(unsafe_copy), &mut two_installs()).unwrap();
        assert_eq!(
            p.plan,
            Plan::ReplaceUnsafe {
                old: "D:\\Tools\\Pitwall\\pitwall-server.exe".into()
            }
        );
        // An unquoted ImagePath with a space is ambiguous to the service manager (it may start C:\Program.exe): unsafe.
        let unquoted = "C:\\Program Files\\Pitwall Old\\pitwall-server.exe --service";
        let p = plan(Some(unquoted), &mut two_installs()).unwrap();
        assert!(matches!(p.plan, Plan::ReplaceUnsafe { .. }), "{p:?}");
        // The checks on this program come first: nothing is replaced for a program that may not be registered.
        let mut port = machine(&[(DIR, SDDL_USER_WRITABLE, false), (EXE, SDDL_INSTALLED, true)]);
        assert!(matches!(plan(Some(unsafe_copy), &mut port), Err(Refusal::NotProtected { .. })));
    }

    /// Review P2-6: `.`, `..` and `\\?\` paths are refused before anything is looked at.
    #[test]
    fn odd_paths_are_refused_up_front() {
        for odd in [
            "C:\\Program Files\\Pitwall\\.\\pitwall-server.exe",
            "C:\\Program Files\\Other\\..\\Pitwall\\pitwall-server.exe",
            "\\\\?\\C:\\Program Files\\Pitwall\\pitwall-server.exe",
            "\\\\.\\C:\\Program Files\\Pitwall\\pitwall-server.exe",
        ] {
            let e = plan_install(odd, None, &Folders(Some(PF)), &mut two_installs()).unwrap_err();
            assert!(matches!(e, Refusal::OddPath(_)), "{odd}: {e:?}");
        }
    }

    /// Review P2-3: a file in the folder that Users may change (a DLL Windows would load next to the program) is refused.
    #[test]
    fn a_writable_file_already_in_the_folder_is_refused() {
        let mut port = machine(&[
            (DIR, SDDL_INSTALLED, false),
            (EXE, SDDL_INSTALLED, true),
            ("C:\\Program Files\\Pitwall\\version.dll", SDDL_USER_WRITABLE, true),
        ]);
        assert!(matches!(plan(None, &mut port), Err(Refusal::NotProtected { .. })));
    }

    /// Review P2-7: the words name the account ("Users"), and the log line never holds a profile path.
    #[test]
    fn refusals_name_accounts_and_log_no_profile_path() {
        let mut port = machine(&[(DIR, SDDL_USER_WRITABLE, false), (EXE, SDDL_INSTALLED, true)]);
        let e = plan(None, &mut port).unwrap_err();
        assert!(e.to_string().contains("Users") && !e.to_string().contains("S-1-5-32-545"), "{e}");
        let profile = "C:\\Users\\sam\\AppData\\Local\\Pitwall\\pitwall-server.exe";
        let e = Refusal::OutsideProgramFiles {
            program: profile.into(),
            program_files: PF.into(),
        };
        assert!(
            !e.log_line().contains("sam") && e.log_line().contains("outside Program Files"),
            "{}",
            e.log_line()
        );
        let e = Refusal::OtherCopy {
            registered: profile.into(),
            program: EXE.into(),
        };
        assert!(!e.log_line().contains("sam"), "{}", e.log_line());
        assert!(Refusal::OtherCopy {
            registered: OTHER.into(),
            program: EXE.into()
        }
        .log_line()
        .contains("Pitwall Old"));
    }

    /// B6 re-review P2-1: the read-back compares paths as Windows does, and the words say what the clean-up really did.
    #[test]
    fn a_registration_read_back_wrong_says_whether_it_was_removed() {
        assert_eq!(
            read_back(Some(&EXE.to_uppercase()), EXE, || unreachable!("nothing to remove")),
            Ok(())
        );
        let removed = read_back(Some(OTHER), EXE, || Ok(())).unwrap_err();
        assert!(removed.contains("so it was removed again"), "{removed}");
        let kept = read_back(None, EXE, || Err("access denied".into())).unwrap_err();
        assert!(
            kept.contains("could not be removed (access denied)") && !kept.contains("removed again"),
            "{kept}"
        );
    }
}
