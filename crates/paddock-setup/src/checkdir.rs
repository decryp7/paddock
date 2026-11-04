//! `check-dir`: the install folder rule (ADR 0053 section 2.2) and the `updates\` folder rule (section 2.5), as decisions
//! over facts the native glue reads from open handles.
//!
//! The canonical `$INSTDIR` must be a direct child of `%ProgramFiles%` (or `%ProgramFiles(x86)%`), no component a reparse
//! point, and its owner (when it exists) SYSTEM, Administrators or TrustedInstaller.

use paddock_service::privileged::trust::is_trusted_sid;

/// What the glue found out about a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirInspect {
    /// The canonical (final) path, long names, no `\\?\` prefix needed.
    pub canonical: String,
    /// Whether the folder exists.
    pub exists: bool,
    /// Whether the folder or any component above it is a reparse point.
    pub reparse_component: bool,
    /// The owner's SID when the folder exists.
    pub owner_sid: Option<String>,
}

/// The facts `check` needs.
pub trait DirFacts {
    /// The canonical `%ProgramFiles%` folders accepted for this build.
    fn program_files(&self) -> Vec<String>;
    /// Looks at `dir`; `Err` carries a short reason (no paths).
    fn inspect(&self, dir: &str) -> Result<DirInspect, String>;
    /// The protection rule of `paddock_service::privileged::trust` (rule 55) over the folder, or over its parent when it does
    /// not exist yet: nobody but SYSTEM, Administrators and TrustedInstaller may be able to change it or anything above it.
    fn protection(&self, inspected: &DirInspect) -> Result<(), String>;
}

/// Why the folder is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DirRefusal {
    /// Not a direct child of Program Files.
    #[error("the folder is not directly inside Program Files")]
    NotInProgramFiles,
    /// The folder or a component above it is a link or junction.
    #[error("the folder or a folder above it is a link")]
    ReparsePoint,
    /// Another account owns it.
    #[error(
        "the folder is owned by an account that is not SYSTEM, Administrators or TrustedInstaller"
    )]
    Owner,
    /// It could not be looked at.
    #[error("the folder could not be checked: {0}")]
    Unreadable(String),
    /// Someone other than SYSTEM, Administrators or TrustedInstaller can change it or a folder above it.
    #[error("the folder is not protected: {0}")]
    Protection(String),
}

impl DirRefusal {
    /// The installer's message ("Install `<App>` in Program Files", the folder named).
    pub fn message(&self, app: &str, dir: &str) -> String {
        format!("Install {app} in Program Files. The folder {dir} is not allowed: {self}.")
    }
}

/// Proof that `check` passed for this folder: the only way to open the setup log ([`crate::log::SetupLog::open`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedDir {
    canonical: String,
}

impl CheckedDir {
    /// The canonical folder.
    pub fn path(&self) -> &str {
        &self.canonical
    }
}

/// Applies the rule.
pub fn check(facts: &dyn DirFacts, dir: &str) -> Result<CheckedDir, DirRefusal> {
    let roots = facts.program_files();
    let found = facts.inspect(dir).map_err(DirRefusal::Unreadable)?;
    let parts = components(&found.canonical);
    let direct_child = parts.len() >= 2
        && roots.iter().any(|root| {
            let r = components(root);
            !r.is_empty() && parts.len() == r.len() + 1 && same_components(&parts[..r.len()], &r)
        });
    if !direct_child {
        return Err(DirRefusal::NotInProgramFiles);
    }
    if found.reparse_component {
        return Err(DirRefusal::ReparsePoint);
    }
    if found.exists && !found.owner_sid.as_deref().is_some_and(is_trusted_sid) {
        return Err(DirRefusal::Owner);
    }
    facts.protection(&found).map_err(DirRefusal::Protection)?;
    Ok(CheckedDir {
        canonical: found.canonical,
    })
}

/// Splits a Windows path into components, dropping empty ones and a `\\?\` prefix.
pub fn components(path: &str) -> Vec<String> {
    let p = path.strip_prefix(r"\\?\").unwrap_or(path);
    p.split('\\')
        .filter(|c| !c.is_empty())
        .map(String::from)
        .collect()
}

/// `true` when `path` is `root` or below it, by components without case (never a string prefix:
/// `C:\Program Files\Dyno Helper` is not inside `C:\Program Files\Dyno`).
pub fn is_within(path: &str, root: &str) -> bool {
    let (p, r) = (components(path), components(root));
    !r.is_empty() && p.len() >= r.len() && same_components(&p[..r.len()], &r)
}

/// The descriptor of `updates\` (section 2.5): the family's protected-folder descriptor, one definition (rule 55): owner
/// Administrators, protected (no inheritance from `$INSTDIR`), SYSTEM and Administrators full, Users read and execute (the
/// log is readable by users), inheritable by what the folder holds.
pub use paddock_service::privileged::windows_acl::PROTECTED_FOLDER_SDDL as UPDATES_SDDL;

/// The descriptor of a run folder and its `tmp\`: Administrators and SYSTEM only. Users never read an update that has not
/// passed its signature check (the staged copy inherits this, see [`STAGED_FILE_SDDL`]).
pub const RUN_FOLDER_SDDL: &str = "O:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// The descriptor of the staged copy itself: Administrators and SYSTEM only, no inheritance.
pub const STAGED_FILE_SDDL: &str = "O:BAD:PAI(A;;FA;;;SY)(A;;FA;;;BA)";

/// What the glue found at `$INSTDIR\updates`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatesInspect {
    /// Something is there.
    pub exists: bool,
    /// It is a directory.
    pub is_dir: bool,
    /// It is a reparse point.
    pub reparse: bool,
    /// The owner's SID.
    pub owner_sid: Option<String>,
}

/// Every folder from the drive root down to `full_path` itself (`C:\`, `C:\Program Files`, `C:\Program Files\Dyno`): the
/// glue opens each one without following it and refuses a reparse point, so a link in the middle of the path is found by its
/// attribute and not by comparing names (an 8.3 name such as `C:\PROGRA~1` is not a link).
pub fn prefixes(full_path: &str) -> Vec<String> {
    let parts = components(full_path);
    let Some((drive, rest)) = parts.split_first() else {
        return Vec::new();
    };
    let mut out = vec![format!("{drive}\\")];
    let mut current = drive.clone();
    for p in rest {
        current = format!("{current}\\{p}");
        out.push(current.clone());
    }
    out
}

/// The folders above a file: every prefix of its path except the file itself. The glue refuses a reparse point among them
/// before the file is opened (a directory symlink or junction in the middle can point at a share).
pub fn ancestors(file_path: &str) -> Vec<String> {
    let mut all = prefixes(file_path);
    all.pop();
    all
}

/// What to do about `$INSTDIR\updates`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatesAction {
    /// Use it as it is.
    Use,
    /// Make it (with the explicit descriptor).
    Create,
    /// Rename it aside (a wrong owner, a reparse point, not a folder) and make a new one.
    RenameAsideAndCreate,
}

/// Decides (section 2.5: same owner rule, no reparse point, else renamed aside and made again).
pub fn plan_updates_dir(found: &UpdatesInspect) -> UpdatesAction {
    if !found.exists {
        return UpdatesAction::Create;
    }
    let owner_ok = found.owner_sid.as_deref().is_some_and(is_trusted_sid);
    if found.is_dir && !found.reparse && owner_ok {
        UpdatesAction::Use
    } else {
        UpdatesAction::RenameAsideAndCreate
    }
}

/// A name for the renamed-aside folder that is not in `taken`.
pub fn aside_name(taken: &[String]) -> String {
    (1u32..)
        .map(|n| format!("updates.refused-{n}"))
        .find(|c| !taken.iter().any(|t| t.eq_ignore_ascii_case(c)))
        .unwrap_or_else(|| "updates.refused".to_string())
}

fn same_components(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.chars()
                .flat_map(char::to_uppercase)
                .eq(y.chars().flat_map(char::to_uppercase))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADMINS: &str = "S-1-5-32-544";
    const SYSTEM: &str = "S-1-5-18";
    const USER: &str = "S-1-5-21-1-2-3-1001";

    struct Fake {
        roots: Vec<&'static str>,
        found: Result<DirInspect, String>,
        protection: Option<String>,
    }
    impl DirFacts for Fake {
        fn program_files(&self) -> Vec<String> {
            self.roots.iter().map(|s| s.to_string()).collect()
        }
        fn inspect(&self, _dir: &str) -> Result<DirInspect, String> {
            self.found.clone()
        }
        fn protection(&self, _inspected: &DirInspect) -> Result<(), String> {
            self.protection.clone().map_or(Ok(()), Err)
        }
    }
    fn fake(canonical: &str, exists: bool, reparse: bool, owner: Option<&str>) -> Fake {
        Fake {
            protection: None,
            roots: vec![r"C:\Program Files"],
            found: Ok(DirInspect {
                canonical: canonical.into(),
                exists,
                reparse_component: reparse,
                owner_sid: owner.map(String::from),
            }),
        }
    }

    #[test]
    fn accepts_a_direct_child_owned_by_admins() {
        let ok = check(
            &fake(r"C:\Program Files\Dyno", true, false, Some(ADMINS)),
            r"C:\Program Files\Dyno",
        )
        .unwrap();
        assert_eq!(ok.path(), r"C:\Program Files\Dyno");
        assert!(
            check(
                &fake(r"c:\PROGRAM FILES\Dyno", true, false, Some(SYSTEM)),
                "x"
            )
            .is_ok()
        );
    }

    #[test]
    fn a_missing_folder_has_no_owner_to_check() {
        assert!(check(&fake(r"C:\Program Files\Dyno", false, false, None), "x").is_ok());
    }

    #[test]
    fn x86_root_is_accepted_when_listed() {
        let mut f = fake(r"C:\Program Files (x86)\Dyno", true, false, Some(ADMINS));
        f.roots = vec![r"C:\Program Files", r"C:\Program Files (x86)"];
        assert!(check(&f, "x").is_ok());
    }

    #[test]
    fn refuses_other_places() {
        for p in [
            r"C:\Dyno",
            r"C:\Program Files",
            r"C:\Program Files\Vendor\Dyno",
            r"D:\Program Files\Dyno",
            r"C:\Users\x\AppData\Local\Dyno",
            r"C:\Program Files Extra\Dyno",
        ] {
            assert_eq!(
                check(&fake(p, true, false, Some(ADMINS)), p).unwrap_err(),
                DirRefusal::NotInProgramFiles,
                "{p}"
            );
        }
    }

    #[test]
    fn refuses_reparse_points_and_other_owners() {
        assert_eq!(
            check(
                &fake(r"C:\Program Files\Dyno", true, true, Some(ADMINS)),
                "x"
            )
            .unwrap_err(),
            DirRefusal::ReparsePoint
        );
        assert_eq!(
            check(
                &fake(r"C:\Program Files\Dyno", true, false, Some(USER)),
                "x"
            )
            .unwrap_err(),
            DirRefusal::Owner
        );
        assert_eq!(
            check(&fake(r"C:\Program Files\Dyno", true, false, None), "x").unwrap_err(),
            DirRefusal::Owner
        );
    }

    #[test]
    fn a_folder_someone_else_can_change_is_refused_with_the_reason() {
        let mut f = fake(r"C:\Program Files\Dyno", true, false, Some(ADMINS));
        f.protection = Some("Users hold WRITE_DAC".into());
        assert_eq!(
            check(&f, "x").unwrap_err(),
            DirRefusal::Protection("Users hold WRITE_DAC".into())
        );
        assert!(
            DirRefusal::Protection("x".into())
                .to_string()
                .contains("not protected")
        );
    }

    #[test]
    fn prefixes_walk_from_the_drive_root_down() {
        assert_eq!(
            prefixes(r"C:\Program Files\Dyno"),
            [r"C:\", r"C:\Program Files", r"C:\Program Files\Dyno"]
        );
        assert_eq!(
            prefixes(r"\\?\C:\PROGRA~1\Dyno\"),
            [r"C:\", r"C:\PROGRA~1", r"C:\PROGRA~1\Dyno"]
        );
        assert_eq!(prefixes(r"C:\"), [r"C:\"]);
        assert!(prefixes("").is_empty());
    }

    #[test]
    fn ancestors_leave_out_the_file_itself() {
        assert_eq!(ancestors(r"C:\a\b\f.exe"), [r"C:\", r"C:\a", r"C:\a\b"]);
        assert_eq!(ancestors(r"C:\f.exe"), [r"C:\"]);
        assert!(ancestors("").is_empty());
    }

    #[test]
    fn unreadable_is_refused_with_the_reason() {
        let f = Fake {
            protection: None,
            roots: vec![r"C:\Program Files"],
            found: Err("access denied".into()),
        };
        assert_eq!(
            check(&f, "x").unwrap_err(),
            DirRefusal::Unreadable("access denied".into())
        );
    }

    #[test]
    fn message_names_the_app_and_folder() {
        let m = DirRefusal::ReparsePoint.message("Dyno", r"C:\X");
        assert!(m.starts_with("Install Dyno in Program Files"));
        assert!(m.contains(r"C:\X"));
    }

    #[test]
    fn within_is_by_component_never_by_prefix() {
        assert!(is_within(
            r"C:\Program Files\Dyno",
            r"C:\Program Files\Dyno"
        ));
        assert!(is_within(
            r"c:\program files\dyno\sub\a.exe",
            r"C:\Program Files\Dyno\"
        ));
        assert!(is_within(
            r"\\?\C:\Program Files\Dyno\a.exe",
            r"C:\Program Files\Dyno"
        ));
        assert!(!is_within(
            r"C:\Program Files\Dyno Helper\a.exe",
            r"C:\Program Files\Dyno"
        ));
        assert!(!is_within(r"C:\Program Files", r"C:\Program Files\Dyno"));
        assert!(!is_within("", r"C:\Program Files"));
    }

    #[test]
    fn components_drop_empties_and_prefix() {
        assert_eq!(components(r"\\?\C:\A\\B\"), vec!["C:", "A", "B"]);
        assert_eq!(components(""), Vec::<String>::new());
    }

    fn up(exists: bool, is_dir: bool, reparse: bool, owner: Option<&str>) -> UpdatesInspect {
        UpdatesInspect {
            exists,
            is_dir,
            reparse,
            owner_sid: owner.map(String::from),
        }
    }

    #[test]
    fn updates_dir_plan() {
        assert_eq!(
            plan_updates_dir(&up(false, false, false, None)),
            UpdatesAction::Create
        );
        assert_eq!(
            plan_updates_dir(&up(true, true, false, Some(ADMINS))),
            UpdatesAction::Use
        );
        assert_eq!(
            plan_updates_dir(&up(true, true, false, Some(SYSTEM))),
            UpdatesAction::Use
        );
        assert_eq!(
            plan_updates_dir(&up(true, true, false, Some(USER))),
            UpdatesAction::RenameAsideAndCreate
        );
        assert_eq!(
            plan_updates_dir(&up(true, true, false, None)),
            UpdatesAction::RenameAsideAndCreate
        );
        assert_eq!(
            plan_updates_dir(&up(true, true, true, Some(ADMINS))),
            UpdatesAction::RenameAsideAndCreate
        );
        assert_eq!(
            plan_updates_dir(&up(true, false, false, Some(ADMINS))),
            UpdatesAction::RenameAsideAndCreate
        );
    }

    #[test]
    fn updates_descriptor_is_the_familys_protected_folder_descriptor() {
        assert_eq!(
            UPDATES_SDDL,
            paddock_service::privileged::windows_acl::PROTECTED_FOLDER_SDDL
        );
        assert!(
            UPDATES_SDDL.starts_with("O:BAD:P"),
            "owner Administrators, protected"
        );
    }

    #[test]
    fn run_folder_and_staged_file_are_administrators_and_system_only() {
        for sddl in [RUN_FOLDER_SDDL, STAGED_FILE_SDDL] {
            assert!(
                sddl.starts_with("O:BAD:PAI"),
                "owner Administrators, protected: {sddl}"
            );
            let who: Vec<&str> = sddl
                .split('(')
                .skip(1)
                .map(|a| a.trim_end_matches(')').rsplit(';').next().unwrap())
                .collect();
            assert_eq!(who, ["SY", "BA"], "{sddl}");
            assert!(sddl.contains(";FA;"), "full control for those two");
        }
        assert!(
            !STAGED_FILE_SDDL.contains("OICI"),
            "the file itself, not inherited further"
        );
    }

    #[test]
    fn aside_names_skip_taken_ones() {
        assert_eq!(aside_name(&[]), "updates.refused-1");
        assert_eq!(
            aside_name(&["updates.refused-1".into(), "UPDATES.REFUSED-2".into()]),
            "updates.refused-3"
        );
    }
}
