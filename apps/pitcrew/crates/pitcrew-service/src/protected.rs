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
use paddock_service::privileged::trust::{is_trusted_sid, is_within, parent_of, verify_chain, Reason, Security, TrustPort, Untrusted};
use pitcrew_core::config::{RunnerConfig, SessionMode};
use std::path::Path;
use std::sync::Arc;

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
            "Pitcrew would run this runner's program with the service's own rights (LocalSystem), and {what} is not in a protected folder: {}. \
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

/// Whether nobody but SYSTEM, Administrators and TrustedInstaller can change `dir` or anything that leads to it (the shared trust
/// rule, strict inside `dir`). A folder that is not even there is not protected.
pub fn folder_is_protected(port: &mut dyn TrustPort, dir: &Path) -> bool {
    let d = text(dir);
    verify_chain(port, &d, FileKind::Dir, Some(&d), false).is_ok()
}

/// The `icacls` arguments that give a **new, empty** interactive runner folder its rules: SYSTEM and Administrators full control
/// and interactive users (`S-1-5-4`) modify, nothing inherited from above, all inherited by what the folder will hold. By
/// well-known SID, so the language of the system does not matter. Only for an empty folder: `icacls` applies the rule to what is
/// already in it, and a hard link a user planted there could make that rewrite a file they cannot write.
pub fn interactive_acl_arguments(dir: &Path) -> Vec<String> {
    [
        text(dir).as_str(),
        "/inheritance:r",
        "/grant:r",
        "*S-1-5-18:(OI)(CI)F",
        "/grant:r",
        "*S-1-5-32-544:(OI)(CI)F",
        "/grant:r",
        "*S-1-5-4:(OI)(CI)M",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// Gives a runner's folder **inside the data folder** the rules its session needs (Windows, as LocalSystem; the data folder is
/// protected, but `C:\ProgramData` lets users create folders, so a folder made by an older Pitcrew inherited a rule that lets users
/// add to it):
///
/// - a `service` runner's folder is locked like the data folder (SYSTEM and Administrators full, Users read and execute, by a
///   handle, on this one folder, never propagated: `TrustPort::lock_dir`), so the guard accepts it;
/// - an `interactive` runner's **new** folder is made writable for interactive users (`grant_interactive`), because that runner
///   runs with the user's own token and writes its `work` and cache there; an existing one is left as it is (it already works).
///
/// A folder outside the data folder is the user's own choice and is left alone, as is one that SYSTEM or Administrators do not own
/// (a user made it first; the guard then refuses a service runner there with the reason). Returns a line for the runner's log.
pub fn settle_runner_dir(
    port: &mut dyn TrustPort,
    data_dir: &Path,
    cfg: &RunnerConfig,
    is_new: bool,
    grant_interactive: &mut dyn FnMut(&Path) -> std::io::Result<()>,
) -> Option<String> {
    let dir = text(&cfg.dir);
    if !is_within(&dir, &text(data_dir), port.style()) {
        return None;
    }
    let node = match port.node(&dir) {
        Ok(Some(n)) => n,
        Ok(None) => return None,
        Err(e) => {
            return Some(format!(
                "the runner's folder could not be examined ({e}); its access rules were not changed"
            ))
        }
    };
    if let Security::Windows(d) = &node.security {
        if !is_trusted_sid(&d.owner) {
            return Some(format!(
                "the runner's folder is owned by {}, not by SYSTEM or Administrators; its access rules were left as they are",
                d.owner
            ));
        }
    }
    // The folders between the data folder and the runner's: an older Pitcrew made them with what `C:\\ProgramData` gives users (they
    // may add to them), and a protected folder under one of those is not protected (the guard checks every ancestor).
    let mut ancestors = Vec::new();
    let mut up = parent_of(&dir, port.style());
    while let Some(p) = up {
        if !is_within(&p, &text(data_dir), port.style()) || p.eq_ignore_ascii_case(&text(data_dir)) {
            break;
        }
        up = parent_of(&p, port.style());
        ancestors.push(p);
    }
    for ancestor in ancestors.iter().rev() {
        let owned_by_us = matches!(port.node(ancestor), Ok(Some(n)) if match &n.security {
            Security::Windows(d) => is_trusted_sid(&d.owner),
            Security::Unix { .. } => true,
        });
        if owned_by_us {
            if let Err(e) = port.lock_dir(ancestor) {
                return Some(format!(
                    "{ancestor} could not be locked ({e}); a service runner below it will be refused until it is"
                ));
            }
        }
    }
    match (cfg.session, is_new) {
        (SessionMode::Service, _) => Some(match port.lock_dir(&dir) {
            Ok(()) => {
                "the runner's folder is locked: only administrators change it (a service runner starts with the service's rights)".into()
            }
            Err(e) => format!("the runner's folder could not be locked ({e}); a service runner there will be refused until it is"),
        }),
        (SessionMode::Interactive, true) => Some(match grant_interactive(&cfg.dir) {
            Ok(()) => "the runner's new folder lets interactive users write (the runner runs with the logged-on user's rights)".into(),
            Err(e) => format!(
                "the runner's new folder could not be opened to the user ({e}); the runner may not be able to write its work folder"
            ),
        }),
        (SessionMode::Interactive, false) => None,
    }
}

/// Applies [`interactive_acl_arguments`] with the system's own `icacls` (Windows).
///
/// # Errors
/// `icacls` could not run or refused.
#[cfg(windows)]
pub fn grant_interactive(dir: &Path) -> std::io::Result<()> {
    use crate::process::{quiet_command, run_with_timeout};
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let icacls = std::path::PathBuf::from(root).join("System32").join("icacls.exe");
    let mut cmd = quiet_command(&icacls);
    cmd.args(interactive_acl_arguments(dir));
    match run_with_timeout(cmd, std::time::Duration::from_secs(30)) {
        Ok(out) if out.success => Ok(()),
        Ok(out) => Err(std::io::Error::other(format!("icacls: {}", out.stderr.trim()))),
        Err(e) => Err(std::io::Error::other(e.to_string())),
    }
}

/// What settles a runner's folder: the runner, and whether its folder was made just now; a line for the runner's log.
pub type SettleHook = Arc<dyn Fn(&RunnerConfig, bool) -> Option<String> + Send + Sync>;

/// The settle hook of this OS: on Windows as LocalSystem, [`settle_runner_dir`] over the real machine (the grant by `icacls`);
/// nothing elsewhere.
pub fn os_settle(data_dir: std::path::PathBuf) -> SettleHook {
    #[cfg(windows)]
    {
        Arc::new(move |cfg, is_new| {
            if !crate::platform::win_session::is_local_system() {
                return None;
            }
            let mut machine = paddock_service::privileged::trust_real::RealTrust;
            settle_runner_dir(&mut machine, &data_dir, cfg, is_new, &mut grant_interactive)
        })
    }
    #[cfg(not(windows))]
    {
        drop(data_dir);
        Arc::new(|_, _| None)
    }
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

    fn runner(dir: &str, session: &str) -> RunnerConfig {
        toml::from_str(&format!(
            "name = \"r\"\nkind = \"gitea-runner\"\ndir = '{dir}'\nsession = \"{session}\"\n"
        ))
        .unwrap()
    }

    const DATA: &str = "C:\\ProgramData\\Pitcrew";

    fn grant_never(_: &Path) -> std::io::Result<()> {
        panic!("no grant was expected")
    }

    /// An older Pitcrew made `runners\r` with what `C:\ProgramData` lets users do in it (they may add to it).
    fn old_runner_folder() -> FakePort {
        let mut p = machine();
        p.put(DATA, locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r", inherited_from_program_data());
        p
    }

    #[test]
    fn a_service_runner_folder_from_an_older_pitcrew_is_locked_so_the_guard_accepts_it() {
        let mut p = old_runner_folder();
        let dir = "C:\\ProgramData\\Pitcrew\\runners\\r";
        // Before: users may add to the folder, so a service runner there is refused.
        assert!(!folder_is_protected(&mut p, Path::new(dir)));
        let line = settle_runner_dir(&mut p, Path::new(DATA), &runner(dir, "service"), false, &mut grant_never).unwrap();
        assert!(line.contains("locked"), "{line}");
        assert!(p.calls.iter().any(|c| c == &format!("lock_dir {dir}")), "{:?}", p.calls);
        // The folder between the data folder and the runner's is locked too, and first: every ancestor is judged.
        let calls: Vec<&str> = p.calls.iter().map(String::as_str).collect();
        let runners = calls
            .iter()
            .position(|c| *c == "lock_dir C:\\ProgramData\\Pitcrew\\runners")
            .expect("runners locked");
        assert!(
            runners < calls.iter().position(|c| *c == format!("lock_dir {dir}")).unwrap(),
            "{calls:?}"
        );
        assert!(folder_is_protected(&mut p, Path::new(dir)), "after: the guard accepts the folder");
    }

    #[test]
    fn a_new_interactive_runner_folder_is_opened_to_the_user_and_an_old_one_is_left_alone() {
        let dir = "C:\\ProgramData\\Pitcrew\\runners\\r";
        let mut p = old_runner_folder();
        let mut granted = vec![];
        let line = settle_runner_dir(&mut p, Path::new(DATA), &runner(dir, "interactive"), true, &mut |d: &Path| {
            granted.push(d.display().to_string());
            Ok(())
        })
        .unwrap();
        assert!(line.contains("interactive users write"), "{line}");
        assert_eq!(granted, [dir]);
        assert!(
            !p.calls.iter().any(|c| c == &format!("lock_dir {dir}")),
            "an interactive folder is not locked: {:?}",
            p.calls
        );
        assert_eq!(
            settle_runner_dir(&mut p, Path::new(DATA), &runner(dir, "interactive"), false, &mut grant_never),
            None
        );
    }

    #[test]
    fn a_folder_outside_the_data_folder_or_made_by_a_user_is_left_alone() {
        let mut p = old_runner_folder();
        p.put("C:\\Users\\someone\\runner", user_owned());
        let outside = runner("C:\\Users\\someone\\runner", "service");
        assert_eq!(settle_runner_dir(&mut p, Path::new(DATA), &outside, false, &mut grant_never), None);
        p.put("C:\\ProgramData\\Pitcrew\\runners\\u", user_owned());
        let by_user = runner("C:\\ProgramData\\Pitcrew\\runners\\u", "service");
        let line = settle_runner_dir(&mut p, Path::new(DATA), &by_user, false, &mut grant_never).unwrap();
        assert!(line.contains("left as they are") && line.contains(SID_USER), "{line}");
        assert!(!p.calls.iter().any(|c| c.starts_with("lock_dir")), "{:?}", p.calls);
    }

    #[test]
    fn the_interactive_folder_rule_names_the_accounts_by_sid() {
        let a = interactive_acl_arguments(Path::new("C:\\ProgramData\\Pitcrew\\runners\\r"));
        assert_eq!(a[0], "C:\\ProgramData\\Pitcrew\\runners\\r");
        assert!(a.contains(&"/inheritance:r".to_string()));
        for rule in ["*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F", "*S-1-5-4:(OI)(CI)M"] {
            assert!(a.contains(&rule.to_string()), "{rule}: {a:?}");
        }
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
