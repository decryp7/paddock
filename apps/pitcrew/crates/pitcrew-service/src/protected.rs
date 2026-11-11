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

/// Why the service may not touch a runner's folder, or `Ok` when only SYSTEM, Administrators and TrustedInstaller can change it.
///
/// This is the one rule of ADR 0004's second amendment (the model of the GitHub Actions runner service): **the SYSTEM service
/// works only in folders only administrators can change.** A runner in a folder a user can change is run by the user, in the
/// user's session; the service never reads, writes, deletes or runs anything in it, so a link, a junction or a swapped file
/// a user planted there can never be followed with SYSTEM's rights.
///
/// Checked: the chain from the drive root to `dir` and `dir` itself (the shared strict rule: owner, no write, add, delete or
/// take-ownership right for anyone else), and every **direct child** of `dir` the same way (a folder an older Pitcrew made inside
/// it keeps the rights `C:\ProgramData` gave it, and a user may have planted a link or a file there). Deeper entries are made
/// by the runner as SYSTEM inside a folder that forbids users to add to it, so they inherit that; checking every one of
/// millions of work files on every request would make the runner unusable. `list` names the children (the real folder, or a
/// fake in tests).
///
/// # Errors
/// The first path that a user could change, with the reason and what to do.
pub fn check_runner_folder(
    port: &mut dyn TrustPort,
    list: &dyn Fn(&str) -> std::io::Result<Vec<String>>,
    dir: &Path,
) -> Result<(), String> {
    let d = text(dir);
    let refuse = |e: Untrusted| {
        format!(
            "{} is not a folder only administrators can change: {}. The Pitcrew service manages only service runners in such folders \
             (like the GitHub Actions runner service); move the runner under C:\\Program Files or C:\\ProgramData\\Pitcrew\\runners, \
             or run it in your own session with the Pitcrew app",
            d,
            e.describe()
        )
    };
    verify_chain(port, &d, FileKind::Dir, Some(&d), false).map_err(refuse)?;
    let names = list(&d).map_err(|e| format!("{d} could not be listed ({e}); the service did not touch it"))?;
    for name in names {
        let child = format!("{}\\{name}", d.trim_end_matches('\\'));
        let Some(node) = port
            .node(&child)
            .map_err(|e| format!("{child} could not be examined ({e}); the service did not touch {d}"))?
        else {
            continue;
        };
        let want = if node.kind == FileKind::Dir {
            FileKind::Dir
        } else {
            FileKind::File
        };
        verify_chain(port, &child, want, Some(&d), false).map_err(refuse)?;
    }
    Ok(())
}

/// The name of the file in the data folder that lists the runner folders the service made itself (or adopted).
pub const ADOPTED_FILE: &str = "adopted-folders.json";

/// The runner folders the service **made** (empty, protected at creation: [`prepare_runner_dir`]) or adopted (the shared
/// adopt-a-tree step, not built yet). **Only these are accepted**: a folder that merely looks admin-only (one an older Pitcrew made,
/// or an administrator made by hand) may still hold, deeper than the service looks, folders a user can write in, so it stays
/// blocked until it is adopted. The list lives in the data folder (administrator-only, and checked at start like the
/// configuration), never in a runner's folder; text that cannot be read is the empty list (fail closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Adopted(std::collections::BTreeSet<String>);

fn adopted_key(p: &Path) -> String {
    text(p).replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

impl Adopted {
    /// Reads the file's text; anything that is not the expected JSON is no folders at all.
    pub fn parse(text: &str) -> Self {
        #[derive(serde::Deserialize)]
        struct File {
            folders: Vec<String>,
        }
        serde_json::from_str::<File>(text).map_or_else(
            |_| Adopted::default(),
            |f| Adopted(f.folders.iter().map(|d| adopted_key(Path::new(d))).collect()),
        )
    }

    /// The file's text.
    pub fn to_json(&self) -> String {
        serde_json::json!({ "folders": self.0.iter().collect::<Vec<_>>() }).to_string()
    }

    /// The file's text as a list: an error (to be logged) when it is not the expected JSON, never silently no folders. The caller
    /// decides what a missing file means (see [`read_adopted`]).
    ///
    /// # Errors
    /// The text is not the list's JSON.
    pub fn try_parse(text: &str) -> Result<Self, String> {
        #[derive(serde::Deserialize)]
        struct File {
            folders: Vec<String>,
        }
        serde_json::from_str::<File>(text)
            .map(|f| Adopted(f.folders.iter().map(|d| adopted_key(Path::new(d))).collect()))
            .map_err(|e| format!("{ADOPTED_FILE} is not the list of folders the service made ({e})"))
    }

    /// Adds a folder (paths compare without case or trailing separators, as on Windows).
    pub fn insert(&mut self, dir: &Path) {
        self.0.insert(adopted_key(dir));
    }

    /// Whether the service made (or adopted) the folder.
    pub fn contains(&self, dir: &Path) -> bool {
        self.0.contains(&adopted_key(dir))
    }
}

/// [`check_runner_folder`] for a folder that is also in `adopted`: the service works only in a folder it made itself or adopted.
///
/// # Errors
/// The folder is not one the service made, or it is not admin-only; the message says what to do.
pub fn check_adopted_runner_folder(
    port: &mut dyn TrustPort,
    list: &dyn Fn(&str) -> std::io::Result<Vec<String>>,
    adopted: &Adopted,
    dir: &Path,
) -> Result<(), String> {
    if !adopted.contains(dir) {
        return Err(format!(
            "{} is not a folder this service made: the service accepts only a runner folder it made itself (through Add runner, under \
             C:\\ProgramData\\Pitcrew\\runners) or adopted; an older or hand-made folder stays blocked until it is adopted, \
             because it may hold, deeper than the service looks, folders a user can still write in",
            text(dir)
        ));
    }
    check_runner_folder(port, list, dir)
}

/// Makes the folder `dir` and any folders above it that do not exist yet, each with the protected rules **at creation**
/// (`TrustPort::create_dir`: no moment in which a user could add to it), but only when the nearest folder that does exist is itself
/// admin-only: a runner under a folder a user can change is refused, not made. A folder that exists is left to the guard.
/// Without this a new runner's default folder (`data\runners\<name>`) would be "missing" to the guard for good and `Register`
/// refused. Returns a line for the log when something was made.
///
/// # Errors
/// The nearest existing folder is one a user can change, or a folder could not be made.
pub fn prepare_runner_dir(
    port: &mut dyn TrustPort,
    dir: &Path,
    record: &mut dyn FnMut(&Path) -> Result<(), String>,
) -> Result<Option<String>, String> {
    let d = text(dir);
    if port.node(&d).map_err(|e| format!("{d} could not be examined ({e})"))?.is_some() {
        return Ok(None);
    }
    let mut missing = vec![d.clone()];
    let mut above = parent_of(&d, port.style()).ok_or_else(|| format!("{d} has no folder above it"))?;
    while port
        .node(&above)
        .map_err(|e| format!("{above} could not be examined ({e})"))?
        .is_none()
    {
        missing.push(above.clone());
        above = parent_of(&above, port.style()).ok_or_else(|| format!("{d} has no existing folder above it"))?;
    }
    verify_chain(port, &above, FileKind::Dir, Some(&above), false).map_err(|e| {
        format!(
            "{d} does not exist yet and the folder it would be made in is not one only administrators can change ({}); the service makes \
             runner folders only inside such a folder (for example under C:\\ProgramData\\Pitcrew\\runners)",
            e.describe()
        )
    })?;
    // Recorded first, in the service's own (administrator-only) folder, never in the runner's: a folder that could not be recorded is
    // not made, so no protected folder is left that nobody accepts. A recorded folder that was never made is only a name the guard
    // finds missing.
    record(dir).map_err(|e| format!("{d} could not be recorded as the service's own ({e}); nothing was made"))?;
    for p in missing.iter().rev() {
        port.create_dir(p).map_err(|e| format!("{p} could not be made ({e})"))?;
    }
    Ok(Some(format!("made the runner folder {d} (administrators and SYSTEM only)")))
}

/// Reads the list of folders the service made. A missing file is the empty list (nothing was made yet); a file that cannot be read or
/// is not the list is an error naming the cause (ER-27), never silently no folders.
///
/// # Errors
/// The file exists and cannot be read, or is not the list.
pub fn read_adopted(file: &Path) -> Result<Adopted, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Adopted::try_parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Adopted::default()),
        Err(e) => Err(format!("{} could not be read ({e})", file.display())),
    }
}

/// What makes a runner's missing folder.
pub type PrepareHook = Arc<dyn Fn(&Path) -> Result<Option<String>, String> + Send + Sync>;

/// The prepare hook of this OS: on Windows as LocalSystem, [`prepare_runner_dir`] over the real machine; nothing elsewhere (the
/// service is the user's own there, and `register` makes the folder).
pub fn os_prepare(data_dir: std::path::PathBuf) -> PrepareHook {
    #[cfg(windows)]
    {
        Arc::new(move |dir| {
            if !crate::platform::win_session::is_local_system() {
                return Ok(None);
            }
            let mut record = |made: &Path| -> Result<(), String> {
                let file = data_dir.join(ADOPTED_FILE);
                let mut list = read_adopted(&file)?;
                list.insert(made);
                crate::store::write_atomic(&file, &list.to_json()).map_err(|e| e.to_string())
            };
            prepare_runner_dir(&mut paddock_service::privileged::trust_real::RealTrust, dir, &mut record)
        })
    }
    #[cfg(not(windows))]
    {
        drop(data_dir);
        Arc::new(|_| Ok(None))
    }
}

/// What decides whether the service may touch a runner's folder.
pub type FolderGuard = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// The guard of this OS: on Windows as LocalSystem, [`check_runner_folder`] over the real machine; everywhere else (the service
/// is the user's own, or a developer's console) nothing.
pub fn os_folder_guard(data_dir: std::path::PathBuf) -> FolderGuard {
    #[cfg(windows)]
    {
        Arc::new(move |dir| {
            if !crate::platform::win_session::is_local_system() {
                return Ok(());
            }
            let list = |d: &str| -> std::io::Result<Vec<String>> {
                std::fs::read_dir(d)?
                    .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect()
            };
            let adopted = read_adopted(&data_dir.join(ADOPTED_FILE))?;
            check_adopted_runner_folder(&mut paddock_service::privileged::trust_real::RealTrust, &list, &adopted, dir)
        })
    }
    #[cfg(not(windows))]
    {
        drop(data_dir);
        Arc::new(|_| Ok(()))
    }
}

/// Locks a **service** runner's folder inside the data folder, the folders above it up to the data folder and the folders directly
/// inside it (Windows, as LocalSystem). `C:\ProgramData` lets users create folders, so a folder an older Pitcrew made there kept what it inherited, and a
/// folder under one of those is not admin-only (the guard checks every ancestor). Each is locked like the data folder, one
/// folder at a time, by a handle, never propagated (`TrustPort::lock_dir`). A folder outside the data folder is the
/// administrator's own choice and is left alone, as is one that SYSTEM or Administrators do not own (a user made it first: the
/// guard then refuses it with the reason). An `interactive` runner's folder is not touched: the service does not manage one.
/// Returns a line for the runner's log.
pub fn settle_runner_dir(
    port: &mut dyn TrustPort,
    list: &dyn Fn(&str) -> std::io::Result<Vec<String>>,
    data_dir: &Path,
    cfg: &RunnerConfig,
) -> Option<String> {
    if cfg.session != SessionMode::Service {
        return None;
    }
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
    if let Err(e) = port.lock_dir(&dir) {
        return Some(format!(
            "the runner's folder could not be locked ({e}); it will be refused until it is"
        ));
    }
    // The folders directly inside it (`work`, `cache`, `bin`) the same way: an older Pitcrew made them with what the folder gave
    // them then, and the guard judges every direct child. Only SYSTEM's or Administrators' own folders, never a link.
    let mut locked = 0;
    for name in list(&dir).unwrap_or_default() {
        let child = format!("{}\\{name}", dir.trim_end_matches('\\'));
        let ours = matches!(port.node(&child), Ok(Some(n)) if n.kind == FileKind::Dir && match &n.security {
            Security::Windows(d) => is_trusted_sid(&d.owner),
            Security::Unix { .. } => true,
        });
        if ours && port.lock_dir(&child).is_ok() {
            locked += 1;
        }
    }
    Some(format!(
        "the runner's folder and its {locked} folder(s) inside are locked: only administrators change them"
    ))
}

/// What settles a runner's folder; a line for the runner's log.
pub type SettleHook = Arc<dyn Fn(&RunnerConfig) -> Option<String> + Send + Sync>;

/// The settle hook of this OS: on Windows as LocalSystem, [`settle_runner_dir`] over the real machine; nothing elsewhere.
pub fn os_settle(data_dir: std::path::PathBuf) -> SettleHook {
    #[cfg(windows)]
    {
        Arc::new(move |cfg| {
            if !crate::platform::win_session::is_local_system() {
                return None;
            }
            let list = |d: &str| -> std::io::Result<Vec<String>> {
                std::fs::read_dir(d)?
                    .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
                    .collect()
            };
            settle_runner_dir(&mut paddock_service::privileged::trust_real::RealTrust, &list, &data_dir, cfg)
        })
    }
    #[cfg(not(windows))]
    {
        drop(data_dir);
        Arc::new(|_| None)
    }
}

/// What the sweep of a trusted folder's children did: every entry moved aside (and why), and every move that failed.
/// A failed move never stops the service: the entry stays where it is and is reported, and the service does not use it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Swept {
    /// `(entry, where it went, why)`.
    pub moved: Vec<(String, String, String)>,
    /// `(entry, the OS's error)`.
    pub failed: Vec<(String, String)>,
}

impl Swept {
    /// One line per entry for the service log.
    pub fn lines(&self) -> Vec<String> {
        let moved = self
            .moved
            .iter()
            .map(|(entry, aside, why)| format!("{entry} was {why}: moved to {aside}. Nothing was deleted: copy back only what you trust"));
        let failed = self.failed.iter().map(|(entry, e)| {
            format!("{entry} could not be moved aside ({e}); the service leaves it alone and does not use it. Close what holds it open and restart the service")
        });
        moved.chain(failed).collect()
    }
}

/// Whether `name` is an entry this sweep already moved aside (`<name>.untrusted-<secs>-<n>`): moving it again would only grow its name.
fn is_moved_aside(name: &str) -> bool {
    name.contains(".untrusted-")
}

/// What [`ensure_protected_root`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootOutcome {
    /// The folder was not there and was created protected.
    Created,
    /// The folder was there and is now locked; what it holds that the service trusts is protected. Carries what the children
    /// sweep moved or could not move (ER-27: each is logged with its reason).
    Locked(Swept),
    /// What was there could not be trusted (the reason); it was moved to the path and a protected folder made.
    MovedAside(String, String),
}

/// The files of the data folder that the service reads as instructions (checked like the folder at every start).
pub const TRUSTED_FILES: &[&str] = &[crate::paths::CONFIG_FILE, crate::paths::CONFIG_BACKUP, "state.json", ADOPTED_FILE];
/// The folders of the data folder the service writes to: owner-checked and locked at every start, their links and strangers' entries
/// moved aside (`runners` is here as well as `logs`: an older Pitcrew's `runners` inherited what `C:\ProgramData` gives users).
pub const TRUSTED_DIRS: &[&str] = &["logs", "runners"];

/// Makes `dir` a protected folder, and checks what the service trusts in it: `trusted_files` (read as instructions) must be
/// files protected like the folder, `trusted_dirs` (written to) must be real folders, never a link, owned by SYSTEM or
/// Administrators; each is locked by handle (never propagated) and **every child of it that is a link, or not owned by SYSTEM or
/// Administrators, is moved aside** (a user could have made one while the folder still inherited what `C:\ProgramData` gives users).
/// (Not the whole tree: a folder the service made before this check inherited rights from `C:\ProgramData` that cost nothing while
/// only administrators could create files here.)
///
/// # Errors
/// What could not be done (the folder could not be created, locked or moved aside): the service must not start.
pub fn ensure_protected_root(
    port: &mut dyn TrustPort,
    list: &dyn Fn(&str) -> std::io::Result<Vec<String>>,
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
    let mut swept = Swept::default();
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
            } else if verdict.is_ok() {
                // A folder an older Pitcrew made inside the data folder (`logs`) inherited what `C:\ProgramData` gives users, who
                // could then make links inside it: it is locked like the data folder, by handle, never propagated.
                port.lock_dir(&child).map_err(|e| io("Locking", e))?;
                let names = list(&child).map_err(|e| io("Listing", e))?;
                for name in names.into_iter().filter(|n| !is_moved_aside(n)) {
                    let entry = format!("{}\\{name}", child.trim_end_matches('\\'));
                    let Some(n) = port.node(&entry).map_err(|e| io("Examining", e))? else {
                        continue;
                    };
                    let ours = match &n.security {
                        Security::Windows(d) => is_trusted_sid(&d.owner),
                        Security::Unix { .. } => true,
                    };
                    if n.kind == FileKind::Symlink || !ours {
                        let why = if n.kind == FileKind::Symlink {
                            "a link"
                        } else {
                            "not owned by SYSTEM or Administrators"
                        };
                        match port.rename_aside(&entry) {
                            Ok(aside) => swept.moved.push((entry, aside, why.to_string())),
                            Err(e) => swept.failed.push((entry, e.to_string())),
                        }
                    }
                }
            }
        }
    }
    match verdict {
        Ok(()) => Ok(RootOutcome::Locked(swept)),
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
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs"]).unwrap();
        assert_eq!(out, RootOutcome::Created);
        assert_eq!(p.calls, ["create_dir C:\\ProgramData\\Pitcrew"]);
    }

    #[test]
    fn an_existing_folder_the_service_made_is_locked_and_kept() {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\pitcrew.toml", file(locked()));
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs"]).unwrap();
        assert!(matches!(out, RootOutcome::Locked(_)), "{out:?}");
        assert!(p.calls.iter().any(|c| c.starts_with("lock_dir")), "{:?}", p.calls);
    }

    fn runner(dir: &str, session: &str) -> RunnerConfig {
        toml::from_str(&format!(
            "name = \"r\"\nkind = \"gitea-runner\"\ndir = '{dir}'\nsession = \"{session}\"\n"
        ))
        .unwrap()
    }

    const DATA: &str = "C:\\ProgramData\\Pitcrew";

    /// An older Pitcrew made `runners\r` with what `C:\ProgramData` lets users do in it (they may add to it).
    fn old_runner_folder() -> FakePort {
        let mut p = machine();
        p.put(DATA, locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r\\work", inherited_from_program_data());
        p
    }

    /// The children of `dir` in the fake machine (what `read_dir` gives on the real one).
    fn list_of(p: &FakePort) -> impl Fn(&str) -> std::io::Result<Vec<String>> {
        let nodes: Vec<String> = p.nodes.keys().cloned().collect();
        move |dir: &str| {
            let prefix = format!("{}\\", dir.to_lowercase().trim_end_matches('\\'));
            Ok(nodes
                .iter()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('\\'))
                .map(String::from)
                .collect())
        }
    }

    fn ensure(p: &mut FakePort, files: &[&str], dirs: &[&str]) -> Result<RootOutcome, String> {
        let list = list_of(p);
        ensure_protected_root(p, &list, Path::new("C:\\ProgramData\\Pitcrew"), files, dirs)
    }

    fn settle(p: &mut FakePort, cfg: &RunnerConfig) -> Option<String> {
        let list = list_of(p);
        settle_runner_dir(p, &list, Path::new(DATA), cfg)
    }

    #[test]
    fn a_service_runner_folder_from_an_older_pitcrew_is_locked_so_the_guard_accepts_it() {
        let mut p = old_runner_folder();
        let dir = "C:\\ProgramData\\Pitcrew\\runners\\r";
        // Before: users may add to the folder, so the service refuses it.
        let list = list_of(&p);
        assert!(check_runner_folder(&mut p, &list, Path::new(dir)).is_err());
        let line = settle(&mut p, &runner(dir, "service")).unwrap();
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
        let list = list_of(&p);
        check_runner_folder(&mut p, &list, Path::new(dir)).expect("after: the guard accepts the folder");
    }

    #[test]
    fn an_interactive_runner_or_a_folder_outside_the_data_folder_or_made_by_a_user_is_left_alone() {
        let mut p = old_runner_folder();
        let dir = "C:\\ProgramData\\Pitcrew\\runners\\r";
        assert_eq!(settle(&mut p, &runner(dir, "interactive")), None, "the service does not manage it");
        p.put("C:\\Users\\someone\\runner", user_owned());
        let outside = runner("C:\\Users\\someone\\runner", "service");
        assert_eq!(settle(&mut p, &outside), None);
        p.put("C:\\ProgramData\\Pitcrew\\runners\\u", user_owned());
        let by_user = runner("C:\\ProgramData\\Pitcrew\\runners\\u", "service");
        let line = settle(&mut p, &by_user).unwrap();
        assert!(line.contains("left as they are") && line.contains(SID_USER), "{line}");
        assert!(p.calls.is_empty(), "nothing was changed: {:?}", p.calls);
    }

    /// B and C in their new form: a runner folder a user can change is refused whole, whatever is in it.
    #[test]
    fn a_folder_a_user_can_change_is_refused_and_so_is_a_user_made_child_in_an_admin_folder() {
        let mut p = machine();
        p.put(
            "C:\\runner",
            from_sddl(
                FileKind::Dir,
                "O:BAG:SYD:AI(A;OICIID;FA;;;SY)(A;OICIID;FA;;;BA)(A;OICIID;0x1301bf;;;AU)",
            ),
        );
        let list = list_of(&p);
        let e = check_runner_folder(&mut p, &list, Path::new("C:\\runner")).unwrap_err();
        assert!(
            e.contains("not a folder only administrators can change") && e.contains("GitHub Actions"),
            "{e}"
        );

        // An admin-only folder holding a child a user made (a `work` folder they own, or a link).
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r\\bin", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r\\work", user_owned());
        let list = list_of(&p);
        let e = check_runner_folder(&mut p, &list, Path::new("C:\\ProgramData\\Pitcrew\\runners\\r")).unwrap_err();
        assert!(e.contains("work"), "{e}");
        let mut link = locked();
        link.kind = FileKind::Symlink;
        p.put("C:\\ProgramData\\Pitcrew\\runners\\r\\work", link);
        let list = list_of(&p);
        let e = check_runner_folder(&mut p, &list, Path::new("C:\\ProgramData\\Pitcrew\\runners\\r")).unwrap_err();
        assert!(e.contains("work"), "{e}");
        // Without the user's child the same folder passes.
        p.nodes.remove(&p.key("C:\\ProgramData\\Pitcrew\\runners\\r\\work"));
        let list = list_of(&p);
        check_runner_folder(&mut p, &list, Path::new("C:\\ProgramData\\Pitcrew\\runners\\r")).unwrap();
    }

    /// P1-2: `logs` inherited what `C:\ProgramData` gives users on an upgrade; it is locked by handle with the data folder, and a
    /// `blocked` folder in it (a user could have made it, with a junction inside) is moved aside and never used.
    #[test]
    fn the_logs_folder_an_older_pitcrew_made_is_locked_and_a_user_made_blocked_folder_in_it_is_moved_aside() {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\logs", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\logs\\blocked", user_owned());
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs"]).unwrap();
        assert!(matches!(out, RootOutcome::Locked(_)), "{out:?}");
        assert!(
            p.calls.contains(&"lock_dir C:\\ProgramData\\Pitcrew\\logs".to_string()),
            "{:?}",
            p.calls
        );
        assert!(
            p.calls
                .iter()
                .any(|c| c.starts_with("rename_aside C:\\ProgramData\\Pitcrew\\logs\\blocked")),
            "{:?}",
            p.calls
        );
        assert!(
            !p.nodes.contains_key(&p.key("C:\\ProgramData\\Pitcrew\\logs\\blocked")),
            "the user's folder is no longer where the service would use it"
        );
    }

    /// P2: `runners` is owner-checked and locked with the data folder like `logs` (an older Pitcrew's `runners` inherited what
    /// `C:\ProgramData` gives users), and in both every child that is a link or not SYSTEM's or Administrators' is moved aside.
    #[test]
    fn the_start_checks_the_files_and_folders_the_service_reads_and_writes() {
        assert!(TRUSTED_DIRS.contains(&"runners") && TRUSTED_DIRS.contains(&"logs"));
        for f in [crate::paths::CONFIG_FILE, crate::paths::CONFIG_BACKUP, ADOPTED_FILE] {
            assert!(TRUSTED_FILES.contains(&f), "{f}");
        }
    }

    #[test]
    fn runners_is_locked_like_logs_and_user_made_children_of_either_are_moved_aside() {
        let mut p = machine();
        let mut link = locked();
        link.kind = FileKind::Symlink;
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\runners\\good", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\evil", user_owned())
            .put("C:\\ProgramData\\Pitcrew\\logs", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\logs\\service.log", file(locked()))
            .put("C:\\ProgramData\\Pitcrew\\logs\\junction", link);
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs", "runners"]).unwrap();
        assert!(matches!(out, RootOutcome::Locked(_)), "{out:?}");
        for locked_dir in ["runners", "logs"] {
            assert!(
                p.calls.contains(&format!("lock_dir C:\\ProgramData\\Pitcrew\\{locked_dir}")),
                "{locked_dir}: {:?}",
                p.calls
            );
        }
        let aside: Vec<&String> = p.calls.iter().filter(|c| c.starts_with("rename_aside")).collect();
        assert_eq!(aside.len(), 2, "{aside:?}");
        assert!(
            aside.iter().any(|c| c.ends_with("runners\\evil")) && aside.iter().any(|c| c.ends_with("logs\\junction")),
            "{aside:?}"
        );
        assert!(
            p.nodes.contains_key(&p.key("C:\\ProgramData\\Pitcrew\\runners\\good")),
            "SYSTEM's own folder stays"
        );
        assert!(
            p.nodes.contains_key(&p.key("C:\\ProgramData\\Pitcrew\\logs\\service.log")),
            "SYSTEM's own file stays"
        );

        // A `runners` that a user made first (user-owned) is not trusted: the whole data folder is moved aside.
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", user_owned());
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs", "runners"]).unwrap();
        assert!(
            matches!(&out, RootOutcome::MovedAside(_, why) if why.contains("runners")),
            "{out:?}"
        );
    }

    fn data_folder_with_a_stranger() -> FakePort {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\runners\\evil", user_owned());
        p
    }

    /// Round 5 P1-2: every start moved the already moved entry again, so its name grew past 255 characters and the service stopped.
    #[test]
    fn starting_twice_moves_an_entry_aside_once_and_names_it_in_the_outcome() {
        let mut p = data_folder_with_a_stranger();
        let first = ensure(&mut p, &["pitcrew.toml"], &["runners"]).unwrap();
        let RootOutcome::Locked(swept) = first else { panic!("{first:?}") };
        assert_eq!(swept.moved.len(), 1, "{swept:?}");
        assert!(
            swept.moved[0].0.ends_with("runners\\evil") && swept.moved[0].2.contains("not owned"),
            "{swept:?}"
        );
        let before = p.calls.iter().filter(|c| c.starts_with("rename_aside")).count();
        let second = ensure(&mut p, &["pitcrew.toml"], &["runners"]).unwrap();
        let after = p.calls.iter().filter(|c| c.starts_with("rename_aside")).count();
        assert_eq!(before, after, "the second start must not move the aside again: {:?}", p.calls);
        assert!(matches!(second, RootOutcome::Locked(s) if s.moved.is_empty() && s.failed.is_empty()));
    }

    /// A port whose `rename_aside` fails for one path (a file held open), everything else the fake's.
    struct FailingRename<'a>(&'a mut FakePort, &'a str);
    impl paddock_service::privileged::trust::TrustPort for FailingRename<'_> {
        fn style(&self) -> Style {
            self.0.style()
        }
        fn node(&mut self, p: &str) -> std::io::Result<Option<paddock_service::privileged::trust::Node>> {
            self.0.node(p)
        }
        fn canonical(&mut self, p: &str) -> std::io::Result<String> {
            self.0.canonical(p)
        }
        fn walk(&mut self, p: &str, l: usize) -> std::io::Result<Vec<(String, paddock_service::privileged::trust::Node)>> {
            self.0.walk(p, l)
        }
        fn create_dir(&mut self, p: &str) -> std::io::Result<()> {
            self.0.create_dir(p)
        }
        fn lock_dir(&mut self, p: &str) -> std::io::Result<()> {
            self.0.lock_dir(p)
        }
        fn rename_aside(&mut self, p: &str) -> std::io::Result<String> {
            if p.ends_with(self.1) {
                return Err(std::io::Error::other("the file is in use"));
            }
            self.0.rename_aside(p)
        }
        fn sweep_aside(&mut self, a: &str, b: &str, k: usize) -> std::io::Result<Vec<String>> {
            self.0.sweep_aside(a, b, k)
        }
        fn remove_tree(&mut self, p: &str) -> std::io::Result<()> {
            self.0.remove_tree(p)
        }
    }

    /// Round 5 P1-2: a failed move stopped the service for good.
    #[test]
    fn a_failed_move_is_reported_and_never_stops_the_start() {
        let mut p = data_folder_with_a_stranger();
        p.put("C:\\ProgramData\\Pitcrew\\runners\\evil2", user_owned());
        let list = list_of(&p);
        let mut failing = FailingRename(&mut p, "runners\\evil");
        let out = ensure_protected_root(
            &mut failing,
            &list,
            Path::new("C:\\ProgramData\\Pitcrew"),
            &["pitcrew.toml"],
            &["runners"],
        )
        .expect("a failed move must not be an error");
        let RootOutcome::Locked(swept) = out else { panic!("{out:?}") };
        assert_eq!(swept.failed.len(), 1, "{swept:?}");
        assert!(swept.failed[0].1.contains("in use"), "{swept:?}");
        assert_eq!(swept.moved.len(), 1, "the other entry is still moved: {swept:?}");
        let lines = swept.lines().join("\n");
        assert!(
            lines.contains("could not be moved aside") && lines.contains("in use") && lines.contains("moved to"),
            "{lines}"
        );
    }

    /// Round 5 P2-3: the folder was made and then recorded, so a failed record left a protected folder nobody accepts. Recorded first,
    /// nothing is made when the record fails.
    #[test]
    fn a_runner_folder_is_recorded_before_it_is_made() {
        let mut p = machine();
        p.put(DATA, locked()).put("C:\\ProgramData\\Pitcrew\\runners", locked());
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\new");
        let e = prepare_runner_dir(&mut p, dir, &mut |_| Err("disk full".into())).unwrap_err();
        assert!(e.contains("disk full") && e.contains("could not be recorded"), "{e}");
        assert!(
            !p.calls.iter().any(|c| c.starts_with("create_dir")),
            "nothing is made when the record fails: {:?}",
            p.calls
        );
    }

    /// Round 5 P2-3: text that is not the list was silently no folders; now it is an error the caller logs.
    #[test]
    fn a_list_of_adopted_folders_that_does_not_parse_is_an_error_and_an_empty_text_is_not() {
        assert!(Adopted::try_parse("{\"folders\":[\"C:\\\\a\"]}")
            .unwrap()
            .contains(Path::new("C:\\a")));
        let e = Adopted::try_parse("not json").unwrap_err();
        assert!(e.contains("adopted-folders.json"), "{e}");
        assert!(Adopted::try_parse("").is_err(), "an empty file is not a list either");
    }

    /// Round 5 P1-2: a listing error was treated as an empty folder.
    #[test]
    fn a_listing_error_of_a_trusted_folder_is_an_error() {
        let mut p = data_folder_with_a_stranger();
        let list = |_: &str| -> std::io::Result<Vec<String>> { Err(std::io::Error::other("access denied")) };
        let e = ensure_protected_root(
            &mut p,
            &list,
            Path::new("C:\\ProgramData\\Pitcrew"),
            &["pitcrew.toml"],
            &["runners"],
        )
        .unwrap_err();
        assert!(e.contains("Listing") && e.contains("access denied"), "{e}");
    }

    /// P1-3: a new runner's folder does not exist yet. It is made protected at creation when the nearest existing folder is
    /// admin-only, and then the guard accepts it; under a folder a user can change it is refused and nothing is made.
    #[test]
    fn a_new_runner_folder_is_made_protected_where_the_folder_above_is_admin_only_and_never_elsewhere() {
        let mut p = machine();
        p.put(DATA, locked()).put("C:\\ProgramData\\Pitcrew\\runners", locked());
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\new");
        let mut adopted = Adopted::default();
        let list = list_of(&p);
        assert!(
            check_runner_folder(&mut p, &list, dir).is_err(),
            "before: missing, so blocked for good"
        );
        let line = prepare_runner_dir(&mut p, dir, &mut |d| {
            adopted.insert(d);
            Ok(())
        })
        .unwrap()
        .unwrap();
        assert!(line.contains("made the runner folder"), "{line}");
        assert_eq!(p.calls, ["create_dir C:\\ProgramData\\Pitcrew\\runners\\new"]);
        let list = list_of(&p);
        check_adopted_runner_folder(&mut p, &list, &adopted, dir).expect("after: the guard accepts the folder the service made");
        assert_eq!(
            prepare_runner_dir(&mut p, dir, &mut |_| panic!("nothing was made")),
            Ok(None),
            "an existing folder is left alone"
        );

        // Folders above are made too, each protected at creation; only the runner's own folder is recorded.
        let deeper = Path::new("C:\\ProgramData\\Pitcrew\\runners\\a\\b");
        let mut recorded = vec![];
        prepare_runner_dir(&mut p, deeper, &mut |d| {
            recorded.push(d.display().to_string());
            Ok(())
        })
        .unwrap();
        assert!(
            p.calls.ends_with(&[
                "create_dir C:\\ProgramData\\Pitcrew\\runners\\a".to_string(),
                "create_dir C:\\ProgramData\\Pitcrew\\runners\\a\\b".to_string()
            ]),
            "{:?}",
            p.calls
        );
        assert_eq!(recorded, ["C:\\ProgramData\\Pitcrew\\runners\\a\\b"]);

        // Under a folder a user can change: refused, nothing made, nothing recorded.
        let mut p = machine();
        p.put("C:\\Users\\someone", user_owned());
        let e = prepare_runner_dir(&mut p, Path::new("C:\\Users\\someone\\runner"), &mut |_| panic!("recorded")).unwrap_err();
        assert!(
            e.contains("does not exist yet") && e.contains("only administrators can change"),
            "{e}"
        );
        assert!(p.calls.is_empty(), "{:?}", p.calls);

        // A folder that could not be recorded is not made (round 5 P2-3: recorded first).
        let mut p = machine();
        p.put(DATA, locked()).put("C:\\ProgramData\\Pitcrew\\runners", locked());
        let e = prepare_runner_dir(&mut p, Path::new("C:\\ProgramData\\Pitcrew\\runners\\x"), &mut |_| {
            Err("disk full".into())
        })
        .unwrap_err();
        assert!(e.contains("could not be recorded") && e.contains("nothing was made"), "{e}");
        assert!(p.calls.is_empty(), "{:?}", p.calls);
    }

    /// P1-A, fail closed: a folder that only looks admin-only is not accepted unless the service made or adopted it.
    #[test]
    fn a_runner_folder_is_accepted_only_if_the_service_made_it() {
        let mut p = machine();
        p.put(DATA, locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r", locked());
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\r");
        let list = list_of(&p);
        check_runner_folder(&mut p, &list, dir).expect("it looks admin-only");
        let e = check_adopted_runner_folder(&mut p, &list, &Adopted::default(), dir).unwrap_err();
        assert!(
            e.contains("is not a folder this service made") && e.contains("blocked until it is adopted"),
            "{e}"
        );
        let mut adopted = Adopted::default();
        adopted.insert(Path::new("c:/programdata/pitcrew/runners/R/"));
        check_adopted_runner_folder(&mut p, &list, &adopted, dir).expect("recorded, whatever the case and separators");
        // A recorded folder that no longer looks admin-only is still refused.
        p.put("C:\\ProgramData\\Pitcrew\\runners\\r", inherited_from_program_data());
        assert!(check_adopted_runner_folder(&mut p, &list, &adopted, dir).is_err());
    }

    #[test]
    fn the_list_of_adopted_folders_round_trips_and_garbage_is_no_folders() {
        let mut a = Adopted::default();
        a.insert(Path::new("C:\\ProgramData\\Pitcrew\\runners\\one"));
        a.insert(Path::new("C:\\ProgramData\\Pitcrew\\runners\\two"));
        let again = Adopted::parse(&a.to_json());
        assert_eq!(again, a);
        assert!(again.contains(Path::new("c:\\programdata\\pitcrew\\runners\\ONE\\")));
        assert!(!again.contains(Path::new("C:\\ProgramData\\Pitcrew\\runners\\three")));
        for bad in ["", "not json", "{}", "{\"folders\": 5}", "[1,2]"] {
            assert_eq!(Adopted::parse(bad), Adopted::default(), "{bad:?}");
        }
    }

    /// P1-1 / P1-A: after an upgrade the folders deeper than the direct children (`work\job`) keep the right `C:\ProgramData` gave
    /// users to add to them, and content a user plants there (`work\repo\.git\hooks`) would run as SYSTEM. The structural check
    /// cannot see that deep, so the runner is **blocked** because the service did not make it: it stays blocked until the shared
    /// adopt-a-tree step adopts its tree.
    #[test]
    fn an_upgraded_service_runner_with_a_user_writable_grandchild_stays_blocked() {
        let mut p = old_runner_folder();
        p.put("C:\\ProgramData\\Pitcrew\\runners\\r\\work\\job", inherited_from_program_data());
        let dir = "C:\\ProgramData\\Pitcrew\\runners\\r";
        let _ = settle(&mut p, &runner(dir, "service"));
        let list = list_of(&p);
        // The structure alone is fooled (this is why the list exists)...
        check_runner_folder(&mut p, &list, Path::new(dir)).expect("direct children locked: the structural check passes");
        // ...so the guard that is used refuses it: the service never made this folder.
        let e = check_adopted_runner_folder(&mut p, &list, &Adopted::default(), Path::new(dir)).unwrap_err();
        assert!(e.contains("is not a folder this service made"), "{e}");
    }

    #[test]
    fn a_link_where_the_logs_folder_should_be_is_not_trusted() {
        let mut p = machine();
        let mut link = locked();
        link.kind = FileKind::Symlink;
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\logs", link);
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs"]).unwrap();
        assert!(matches!(&out, RootOutcome::MovedAside(_, r) if r.contains("logs")), "{out:?}");
    }

    #[test]
    fn a_folder_or_configuration_a_user_made_first_is_moved_aside_not_trusted() {
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\pitcrew.toml", file(user_owned()));
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs"]).unwrap();
        let RootOutcome::MovedAside(aside, reason) = out else {
            panic!("{out:?}");
        };
        assert!(aside.contains("untrusted") && reason.contains("pitcrew.toml"), "{aside} {reason}");
        assert!(p.calls.iter().any(|c| c.starts_with("create_dir")), "{:?}", p.calls);
        let _ = (SID_ADMINISTRATORS, SID_USERS);
    }
}

#[cfg(test)]
mod read_adopted_tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn scratch(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-adopted-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Round 5 P2-3: only NotFound means no list.
    #[test]
    fn only_a_missing_file_is_the_empty_list_and_garbage_or_a_folder_is_an_error() {
        let dir = scratch("read");
        assert_eq!(read_adopted(&dir.join("none.json")).unwrap(), Adopted::default());
        std::fs::write(dir.join("bad.json"), "garbage").unwrap();
        assert!(read_adopted(&dir.join("bad.json")).unwrap_err().contains("not the list"));
        assert!(read_adopted(&dir).is_err(), "a folder where the file should be cannot be read");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
