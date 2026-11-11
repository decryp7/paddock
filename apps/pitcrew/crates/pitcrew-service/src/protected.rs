//! Places the service may trust with its own token (Windows, where it runs as LocalSystem).
//!
//! Two decisions, both over the shared trust rule of `paddock-service` (`TrustPort`, `verify_chain`; ER-22), so they are tested
//! on a fake machine and run on the real one:
//!
//! - [`check_service_launch`]: a runner that starts with the service's token (session `service`, or `interactive` with
//!   `logged-off = service-account` while nobody is logged on) runs a program and a working folder that a standard user can
//!   change only if they are *protected*: owned by SYSTEM, Administrators or TrustedInstaller, with no write, delete or
//!   take-ownership right for anyone else, on the folder and on every ancestor. Otherwise a user could swap the program and
//!   have SYSTEM run it. **The runner is refused, not copied** into a protected folder: a copy of a runner is hundreds of
//!   megabytes with its own `.runner` registration and work folders, an update would have to repair the copy, and "move it
//!   somewhere only administrators can write" is one step the administrator can take and see (ADR 0004 amendment).
//!   Runners in the user's own session (`interactive`) keep running anywhere: they have the user's rights only.
//! - [`ensure_protected_root`]: the data folder (`%ProgramData%\Pitcrew`, holding the configuration that names the programs the
//!   service starts) is created, or locked, with a protected ACL (SYSTEM and Administrators full control, Users read and
//!   execute, nothing inherited from `C:\ProgramData`, owner Administrators). A folder a user made before the service did, or a
//!   configuration file a user owns, is moved aside (never deleted) and the folder is made again.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use paddock_service::privileged::files::FileKind;
use paddock_service::privileged::install::without_verbatim_prefix;
use paddock_service::privileged::trust::{is_trusted_sid, parent_of, verify_chain, Reason, Security, TrustPort, Untrusted};
use std::path::Path;

fn text(p: &Path) -> String {
    without_verbatim_prefix(&p.to_string_lossy()).to_string()
}

/// Whether the service may start `program` in `dir` with its own token; the error says what is wrong and what to do.
///
/// # Errors
/// The message for the runner's problem line: the first path that a standard user could change, and the next step.
pub fn check_service_launch(port: &mut dyn TrustPort, program: &Path, dir: &Path) -> Result<(), String> {
    let refuse = |what: &str, e: Untrusted| {
        format!(
            "this runner starts with the service's own rights (LocalSystem), and {what} is not in a protected folder: {}. \
             Move the runner into a folder only administrators can write (for example under C:\\Program Files or \
             C:\\ProgramData\\Pitcrew\\runners) and set its folder there, or run it in the user's session",
            e.describe()
        )
    };
    let program = text(program);
    let folder = parent_of(&program, port.style()).unwrap_or_else(|| program.clone());
    verify_chain(port, &program, FileKind::File, Some(&folder), false).map_err(|e| refuse("its program", e))?;
    verify_chain(port, &text(dir), FileKind::Dir, Some(&text(dir)), false).map_err(|e| refuse("its folder", e))?;
    Ok(())
}

/// What [`ensure_protected_root`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootOutcome {
    /// The folder was not there and was created protected.
    Created,
    /// The folder was there and is now locked; what it holds that the service trusts is protected.
    Locked,
    /// What was there could not be trusted (the reason); it was moved to the path and a protected folder made.
    MovedAside(String, String),
}

/// Makes `dir` a protected folder, and checks what the service trusts in it: `trusted_files` (read as instructions) must be
/// files protected like the folder, `trusted_dirs` (written to) must be real folders, never a link, owned by SYSTEM or
/// Administrators. (Not the whole tree: a folder the service made before this check inherited rights from `C:\ProgramData`
/// that cost nothing while only administrators could create files here.)
///
/// # Errors
/// What could not be done (the folder could not be created, locked or moved aside): the service must not start.
pub fn ensure_protected_root(
    port: &mut dyn TrustPort,
    dir: &Path,
    trusted_files: &[&str],
    trusted_dirs: &[&str],
) -> Result<RootOutcome, String> {
    let path = text(dir);
    let io = |what: &str, e: std::io::Error| format!("{what} {path}: {e}");
    let Some(node) = port.node(&path).map_err(|e| io("Examining", e))? else {
        port.create_dir(&path).map_err(|e| io("Creating the protected folder", e))?;
        return Ok(RootOutcome::Created);
    };
    let is_folder = node.kind == FileKind::Dir;
    let mut verdict: Result<(), String> = if is_folder { Ok(()) } else { Err("it is not a folder".into()) };
    if is_folder {
        port.lock_dir(&path).map_err(|e| io("Locking", e))?;
        verdict = verify_chain(port, &path, FileKind::Dir, Some(&path), false).map_err(|e| e.describe());
        for name in trusted_files {
            let file = format!("{}\\{name}", path.trim_end_matches('\\'));
            if verdict.is_ok() && port.node(&file).map_err(|e| io("Examining", e))?.is_some() {
                verdict = verify_chain(port, &file, FileKind::File, Some(&path), false).map_err(|e| e.describe());
            }
        }
    }
    if is_folder {
        for name in trusted_dirs {
            let child = format!("{}\\{name}", path.trim_end_matches('\\'));
            let Some(node) = port.node(&child).map_err(|e| io("Examining", e))? else {
                continue;
            };
            let owner_ok = match &node.security {
                Security::Windows(d) => is_trusted_sid(&d.owner),
                Security::Unix { .. } => true,
            };
            if verdict.is_ok() && (node.kind != FileKind::Dir || !owner_ok) {
                let reason = if node.kind == FileKind::Symlink {
                    Reason::Link
                } else {
                    Reason::UntrustedOwner("not SYSTEM or Administrators".into())
                };
                verdict = Err(Untrusted { path: child, reason }.describe());
            }
        }
    }
    match verdict {
        Ok(()) => Ok(RootOutcome::Locked),
        Err(reason) => {
            let aside = port.rename_aside(&path).map_err(|e| io("Moving aside", e))?;
            port.create_dir(&path).map_err(|e| io("Creating the protected folder", e))?;
            Ok(RootOutcome::MovedAside(aside, reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_service::privileged::trust::{Ace, Style, SID_ADMINISTRATORS, SID_SYSTEM, SID_USERS};
    use paddock_service::privileged::trust_fake::{
        file, from_sddl, inherited_from_program_data, locked, stock_drive_root, stock_program_data, win, FakePort,
    };
    use std::collections::HashMap;

    const SID_USER: &str = "S-1-5-21-1-2-3-1001";
    const FULL: u32 = 0x1f01ff;

    fn machine() -> FakePort {
        let mut p = FakePort {
            style: Style::Windows,
            nodes: HashMap::new(),
            links: HashMap::new(),
            calls: vec![],
        };
        p.put("C:\\", stock_drive_root()).put("C:\\ProgramData", stock_program_data());
        p
    }

    fn user_owned() -> paddock_service::privileged::trust::Node {
        win(SID_USER, vec![Ace::allow(SID_USER, FULL), Ace::allow(SID_SYSTEM, FULL)])
    }

    #[test]
    fn a_runner_in_a_folder_users_can_change_is_refused_for_the_service_token() {
        let mut p = machine();
        // `C:\runner` made by an administrator inherits what `C:\` gives the users it lets create things: they may modify it.
        p.put(
            "C:\\runner",
            from_sddl(
                FileKind::Dir,
                "O:BAG:SYD:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIID;0x1301bf;;;AU)",
            ),
        )
        .put(
            "C:\\runner\\run.exe",
            file(from_sddl(
                FileKind::Dir,
                "O:BAG:SYD:AI(A;ID;FA;;;SY)(A;ID;FA;;;BA)(A;ID;0x1301bf;;;AU)",
            )),
        );
        let e = check_service_launch(&mut p, Path::new("C:\\runner\\run.exe"), Path::new("C:\\runner")).unwrap_err();
        assert!(
            e.contains("not in a protected folder") && e.contains("C:\\runner is not") && e.contains("Program Files"),
            "{e}"
        );
    }

    #[test]
    fn a_runner_in_a_protected_folder_is_accepted() {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\a", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\a\\run.exe", file(locked()));
        check_service_launch(
            &mut p,
            Path::new("C:\\ProgramData\\Pitcrew\\runners\\a\\run.exe"),
            Path::new("C:\\ProgramData\\Pitcrew\\runners\\a"),
        )
        .unwrap();
    }

    #[test]
    fn a_program_in_a_protected_folder_is_refused_when_the_working_folder_is_not() {
        let mut p = machine();
        p.put("C:\\tools", locked())
            .put("C:\\tools\\run.exe", file(locked()))
            .put("C:\\work", user_owned());
        let e = check_service_launch(&mut p, Path::new("C:\\tools\\run.exe"), Path::new("C:\\work")).unwrap_err();
        assert!(e.contains("its folder"), "{e}");
    }

    #[test]
    fn a_missing_data_folder_is_created_protected() {
        let mut p = machine();
        let out = ensure_protected_root(&mut p, Path::new("C:\\ProgramData\\Pitcrew"), &["pitcrew.toml"], &["logs"]).unwrap();
        assert_eq!(out, RootOutcome::Created);
        assert_eq!(p.calls, ["create_dir C:\\ProgramData\\Pitcrew"]);
    }

    #[test]
    fn an_existing_folder_the_service_made_is_locked_and_kept() {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\pitcrew.toml", file(locked()));
        let out = ensure_protected_root(&mut p, Path::new("C:\\ProgramData\\Pitcrew"), &["pitcrew.toml"], &["logs"]).unwrap();
        assert_eq!(out, RootOutcome::Locked);
        assert!(p.calls.iter().any(|c| c.starts_with("lock_dir")), "{:?}", p.calls);
    }

    #[test]
    fn a_link_where_the_logs_folder_should_be_is_not_trusted() {
        let mut p = machine();
        let mut link = locked();
        link.kind = FileKind::Symlink;
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\logs", link);
        let out = ensure_protected_root(&mut p, Path::new("C:\\ProgramData\\Pitcrew"), &["pitcrew.toml"], &["logs"]).unwrap();
        assert!(matches!(&out, RootOutcome::MovedAside(_, r) if r.contains("logs")), "{out:?}");
    }

    #[test]
    fn a_folder_or_configuration_a_user_made_first_is_moved_aside_not_trusted() {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\pitcrew.toml", file(user_owned()));
        let out = ensure_protected_root(&mut p, Path::new("C:\\ProgramData\\Pitcrew"), &["pitcrew.toml"], &["logs"]).unwrap();
        let RootOutcome::MovedAside(aside, reason) = out else {
            panic!("{out:?}");
        };
        assert!(aside.contains("untrusted") && reason.contains("pitcrew.toml"), "{aside} {reason}");
        assert!(p.calls.iter().any(|c| c.starts_with("create_dir")), "{:?}", p.calls);
        let _ = (SID_ADMINISTRATORS, SID_USERS);
    }
}
