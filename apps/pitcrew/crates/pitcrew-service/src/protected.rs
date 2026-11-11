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
use paddock_service::privileged::trust::{
    is_trusted_sid, is_within, parent_of, verify_chain, FileIdentity, Identity, Reason, Security, TrustPort, Untrusted,
};
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
/// configuration), never in a runner's folder. A missing file is the empty list; a file that cannot be read or is not the list is an
/// error ([`read_adopted`]), never silently no folders. Each entry is bound to the file the folder is ([`Adopted::bind`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Adopted(std::collections::BTreeMap<String, Option<FileIdentity>>);

fn adopted_key(p: &Path) -> String {
    text(p).replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// One entry of the list file: the folder and, once the folder exists, which folder it is (volume and file index).
#[derive(serde::Serialize, serde::Deserialize)]
struct AdoptedEntry {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    volume: Option<u64>,
    /// Hex: a 128-bit id does not fit a JSON number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct AdoptedFile {
    folders: Vec<AdoptedEntry>,
}

impl Adopted {
    /// Reads the file's text; anything that is not the expected JSON is no folders at all.
    pub fn parse(text: &str) -> Self {
        Adopted::try_parse(text).unwrap_or_default()
    }

    /// The file's text as a list: an error (to be logged) when it is not the expected JSON, never silently no folders. The caller
    /// decides what a missing file means (see [`read_adopted`]).
    ///
    /// # Errors
    /// The text is not the list's JSON.
    pub fn try_parse(text: &str) -> Result<Self, String> {
        serde_json::from_str::<AdoptedFile>(text)
            .map(|f| {
                Adopted(
                    f.folders
                        .into_iter()
                        .map(|e| {
                            let id = e
                                .volume
                                .zip(e.index.and_then(|h| u128::from_str_radix(&h, 16).ok()))
                                .map(|(volume, index)| FileIdentity { volume, index, links: 1 });
                            (adopted_key(Path::new(&e.path)), id)
                        })
                        .collect(),
                )
            })
            .map_err(|e| format!("{ADOPTED_FILE} is not the list of folders the service made ({e})"))
    }

    /// The file's text.
    pub fn to_json(&self) -> String {
        let folders = self
            .0
            .iter()
            .map(|(path, id)| AdoptedEntry {
                path: path.clone(),
                volume: id.map(|i| i.volume),
                index: id.map(|i| format!("{:x}", i.index)),
            })
            .collect();
        serde_json::to_string(&AdoptedFile { folders }).unwrap_or_default()
    }

    /// Adds a folder, not yet bound to a file (paths compare without case or trailing separators, as on Windows). Where the port can
    /// tell files apart, an entry without a file is **not accepted** until [`Adopted::bind`] gives it one.
    pub fn insert(&mut self, dir: &Path) {
        self.0.entry(adopted_key(dir)).or_insert(None);
    }

    /// Binds the folder's entry to the file it is (replacing what was there).
    pub fn bind(&mut self, dir: &Path, id: FileIdentity) {
        self.0.insert(adopted_key(dir), Some(id));
    }

    /// Forgets a folder; whether it was there.
    pub fn remove(&mut self, dir: &Path) -> bool {
        self.0.remove(&adopted_key(dir)).is_some()
    }

    /// Whether the service made (or adopted) the folder.
    pub fn contains(&self, dir: &Path) -> bool {
        self.0.contains_key(&adopted_key(dir))
    }

    /// The file the folder's entry is bound to (`None`: not in the list, or not bound).
    pub fn bound(&self, dir: &Path) -> Option<FileIdentity> {
        self.0.get(&adopted_key(dir)).copied().flatten()
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
    // The entry names a path; it must also be this folder. A folder swapped in under the recorded name (renamed away and replaced) is a
    // different file, and an entry that was never bound to a file is not accepted either. A port that cannot tell files apart
    // (`identity` is `None`) leaves the path as the only evidence, as the fakes do.
    let identity = port
        .identity(&text(dir))
        .map_err(|e| format!("{} could not be identified ({e}); the service did not touch it", text(dir)))?;
    match identity {
        Identity::Known(actual) => match adopted.bound(dir) {
            Some(recorded) if (recorded.volume, recorded.index) == (actual.volume, actual.index) => {}
            Some(_) => {
                return Err(format!(
                "{} is not the folder this service made under that name (it was replaced); the service accepts only the folder it made. \
                     Remove or rename the folder, then add the runner again",
                text(dir)
            ))
            }
            None => {
                return Err(format!(
                    "{} is recorded as made by this service but was never bound to the folder it made; it stays blocked. Remove or rename \
                     the folder, then add the runner again",
                    text(dir)
                ))
            }
        },
        // Nothing there: the folder check below says "missing".
        Identity::Missing => {}
        // A port that cannot tell files apart is a gap, not a pass (a fake that models none is the one exception).
        Identity::Unknown if port.models_identity() => {
            return Err(format!(
                "{} could not be identified (the machine did not say which file it is), so the service did not touch it",
                text(dir)
            ))
        }
        Identity::Unknown => {}
    }
    check_runner_folder(port, list, dir)
}

/// What records a runner folder in the list of folders the service made: the folder, and the file it is once it exists.
pub type Record<'a> = dyn FnMut(&Path, Entry) -> Result<(), String> + 'a;

/// What happens to a runner folder's entry in the list of folders the service made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// Recorded before the folder is made: not accepted until bound.
    Pending,
    /// Bound to the file the folder is.
    Bound(FileIdentity),
    /// The folder could not be bound and was removed: the entry goes too.
    Forget,
}

/// Makes the folder `dir` and any folders above it that do not exist yet, each with the protected rules **at creation**
/// (`TrustPort::create_dir`: no moment in which a user could add to it), but only when the nearest folder that does exist is itself
/// admin-only: a runner under a folder a user can change is refused, not made. A folder that exists is left to the guard.
/// Without this a new runner's default folder (`data\runners\<name>`) would be "missing" to the guard for good and `Register`
/// refused. Returns a line for the log when something was made.
///
/// # Errors
/// The nearest existing folder is one a user can change, or a folder could not be made.
pub fn prepare_runner_dir(port: &mut dyn TrustPort, dir: &Path, record: &mut Record) -> Result<Option<String>, String> {
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
    record(dir, Entry::Pending).map_err(|e| format!("{d} could not be recorded as the service's own ({e}); nothing was made"))?;
    for p in missing.iter().rev() {
        port.create_dir(p).map_err(|e| format!("{p} could not be made ({e})"))?;
    }
    // Then the entry is bound to the folder just made: which file it is, so a folder put there later under the same name is not it. If
    // that cannot be done the empty folder and its entry are removed, so nothing is left that nobody accepts.
    let undo = |port: &mut dyn TrustPort, record: &mut Record<'_>, why: String| -> String {
        let removed = missing.iter().all(|p| port.remove_tree(p).is_ok());
        let forgotten = record(dir, Entry::Forget);
        let tail = if removed {
            "it was removed again"
        } else {
            "it could not be removed again: remove or rename it"
        };
        match forgotten {
            Ok(()) => format!("{d} was made but {why}; {tail}"),
            Err(e) => format!("{d} was made but {why}; {tail}; its entry could not be removed either ({e})"),
        }
    };
    match port.identity(&d) {
        Ok(Identity::Known(id)) => {
            if let Err(e) = record(dir, Entry::Bound(id)) {
                return Err(undo(port, record, format!("its entry could not be bound to it ({e})")));
            }
        }
        Ok(Identity::Unknown) if !port.models_identity() => {}
        Ok(other) => return Err(undo(port, record, format!("the machine did not say which file it is ({other:?})"))),
        Err(e) => return Err(undo(port, record, format!("it could not be identified ({e})"))),
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
            let mut record = |made: &Path, entry: Entry| -> Result<(), String> {
                let file = data_dir.join(ADOPTED_FILE);
                let mut list = read_adopted(&file)?;
                match entry {
                    Entry::Bound(id) => list.bind(made, id),
                    Entry::Pending => list.insert(made),
                    Entry::Forget => {
                        list.remove(made);
                    }
                }
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

/// Removes `dir` from the list of folders the service made, in `file` (the data folder's `adopted-folders.json`). Returns whether it
/// was there. A missing file is nothing to remove.
///
/// # Errors
/// The list cannot be read, is not the list, or cannot be written; the cause is in the message.
pub fn forget_adopted(file: &Path, dir: &Path) -> Result<bool, String> {
    let mut list = read_adopted(file)?;
    if !list.remove(dir) {
        return Ok(false);
    }
    crate::store::write_atomic(file, &list.to_json()).map_err(|e| e.to_string())?;
    Ok(true)
}

/// What forgets a removed runner's folder: it leaves the list, so a folder put at the same path later is not accepted on the old
/// entry. Returns a line for the service log when an entry was removed.
pub type ForgetHook = Arc<dyn Fn(&Path) -> Result<Option<String>, String> + Send + Sync>;

/// The forget hook of this OS: on Windows as LocalSystem, [`forget_adopted`] over the data folder's list; nothing elsewhere.
pub fn os_forget(data_dir: std::path::PathBuf) -> ForgetHook {
    #[cfg(windows)]
    {
        Arc::new(move |dir| {
            if !crate::platform::win_session::is_local_system() {
                return Ok(None);
            }
            forget_adopted(&data_dir.join(ADOPTED_FILE), dir)
                .map(|was| was.then(|| format!("{} is no longer a folder the service made (the runner was removed)", dir.display())))
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
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // The real names are `<secs>-<n>`; the fake machine of the tests counts with one number.
    name.rsplit_once(".untrusted-")
        .is_some_and(|(base, tail)| !base.is_empty() && tail.split('-').count() <= 2 && tail.split('-').all(digits))
}

/// A name the service opens for writing in `logs` (the service log and its rotation names): if one of them cannot be moved away the
/// service must not start, because it would open it (round 6 P1-B).
fn is_service_log_name(name: &str) -> bool {
    name == "service.log"
        || name
            .strip_prefix("service.log.")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
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
                    // A file with a second name is a hard link a user may hold from outside the folder: a write the service makes
                    // through this name would reach their file, and their name keeps reaching what the service wrote (round 5 P2-9).
                    let hard_linked = n.kind == FileKind::File
                        && match port.identity(&entry).map_err(|e| io("Identifying", e))? {
                            Identity::Known(id) => id.links > 1,
                            Identity::Unknown => port.models_identity(),
                            Identity::Missing => false,
                        };
                    if n.kind == FileKind::Symlink || !ours || hard_linked {
                        let why = if n.kind == FileKind::Symlink {
                            "a link"
                        } else if !ours {
                            "not owned by SYSTEM or Administrators"
                        } else {
                            "a file with more than one hard link"
                        };
                        match port.rename_aside(&entry) {
                            Ok(aside) => swept.moved.push((entry, aside, why.to_string())),
                            // A name the service opens itself stays a hard stop: it would append to what a user holds.
                            Err(e) if is_service_log_name(&name) => {
                                return Err(format!(
                                    "{entry} is {why} and could not be moved aside ({e}); the service would write through it, so it does not start. \
                                     Close what holds it open, or remove or rename it"
                                ))
                            }
                            Err(e) => swept.failed.push((entry, e.to_string())),
                        }
                    }
                }
            }
        }
    }
    // The data folder's own other children (a stray `.tmp` or `.bak`, a planted link): a link or something not owned by SYSTEM or
    // Administrators is moved aside like the trusted folders' strangers; the trusted names were judged above.
    if is_folder && verdict.is_ok() {
        let names = list(&path).map_err(|e| io("Listing", e))?;
        for name in names.into_iter().filter(|n| {
            !is_moved_aside(n)
                && !trusted_files.iter().any(|f| f.eq_ignore_ascii_case(n))
                && !trusted_dirs.iter().any(|d| d.eq_ignore_ascii_case(n))
        }) {
            let entry = format!("{}\\{name}", path.trim_end_matches('\\'));
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
    struct FailingRename<'a>(&'a mut dyn TrustPort, &'a str);
    impl paddock_service::privileged::trust::TrustPort for FailingRename<'_> {
        fn style(&self) -> Style {
            self.0.style()
        }
        fn node(&mut self, p: &str) -> std::io::Result<Option<paddock_service::privileged::trust::Node>> {
            self.0.node(p)
        }
        fn identity(&mut self, p: &str) -> std::io::Result<Identity> {
            self.0.identity(p)
        }
        fn models_identity(&self) -> bool {
            self.0.models_identity()
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

    /// A port that also knows which file each path is (the fake models no identities): `ids` by lower-case path.
    struct WithIds<'a>(&'a mut dyn TrustPort, std::collections::HashMap<String, FileIdentity>);
    impl paddock_service::privileged::trust::TrustPort for WithIds<'_> {
        fn style(&self) -> Style {
            self.0.style()
        }
        fn node(&mut self, p: &str) -> std::io::Result<Option<paddock_service::privileged::trust::Node>> {
            self.0.node(p)
        }
        fn identity(&mut self, p: &str) -> std::io::Result<Identity> {
            let key = p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
            Ok(match self.1.get(&key) {
                Some(id) => Identity::Known(*id),
                None if self.0.node(p)?.is_none() => Identity::Missing,
                None => Identity::Unknown,
            })
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
            self.0.rename_aside(p)
        }
        fn sweep_aside(&mut self, a: &str, b: &str, k: usize) -> std::io::Result<Vec<String>> {
            self.0.sweep_aside(a, b, k)
        }
        fn remove_tree(&mut self, p: &str) -> std::io::Result<()> {
            self.0.remove_tree(p)
        }
    }

    fn id(index: u128, links: u32) -> FileIdentity {
        FileIdentity { volume: 7, index, links }
    }

    /// Round 5 P2-9: a file in `logs` with a second hard link is moved aside, one with a single link stays.
    #[test]
    fn a_file_in_logs_with_a_second_hard_link_is_moved_aside() {
        let mut fake = machine();
        fake.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\logs", locked())
            .put("C:\\ProgramData\\Pitcrew\\logs\\service.log", file(locked()))
            .put("C:\\ProgramData\\Pitcrew\\logs\\planted.log", file(locked()));
        let list = list_of(&fake);
        let ids = [
            ("c:\\programdata\\pitcrew\\logs\\service.log".to_string(), id(1, 1)),
            ("c:\\programdata\\pitcrew\\logs\\planted.log".to_string(), id(2, 2)),
        ];
        let mut port = WithIds(&mut fake, ids.into_iter().collect());
        let out = ensure_protected_root(
            &mut port,
            &list,
            Path::new("C:\\ProgramData\\Pitcrew"),
            &["pitcrew.toml"],
            &["logs"],
        )
        .unwrap();
        let RootOutcome::Locked(swept) = out else { panic!("{out:?}") };
        assert_eq!(swept.moved.len(), 1, "{swept:?}");
        assert!(
            swept.moved[0].0.ends_with("planted.log") && swept.moved[0].2.contains("hard link"),
            "{swept:?}"
        );
    }

    /// Round 5 P2-3: the entry is bound to the folder the service made; a folder put there under the same name is a different file.
    #[test]
    fn a_runner_folder_replaced_under_its_recorded_name_is_not_accepted() {
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\r");
        let mut fake = machine();
        fake.put(DATA, locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r", locked());
        let key = "c:\\programdata\\pitcrew\\runners\\r".to_string();
        let list = list_of(&fake);
        let mut adopted = Adopted::default();
        adopted.bind(dir, id(10, 1));
        // The same file: accepted.
        let mut same = WithIds(&mut fake, [(key.clone(), id(10, 1))].into_iter().collect());
        check_adopted_runner_folder(&mut same, &list, &adopted, dir).expect("the folder the service made");
        // Another file under the name: refused.
        let mut swapped = WithIds(&mut fake, [(key.clone(), id(11, 1))].into_iter().collect());
        let e = check_adopted_runner_folder(&mut swapped, &list, &adopted, dir).unwrap_err();
        assert!(e.contains("replaced"), "{e}");
        // An entry never bound to a file: refused where the port can tell files apart.
        let mut unbound = Adopted::default();
        unbound.insert(dir);
        let mut port = WithIds(&mut fake, [(key, id(10, 1))].into_iter().collect());
        let e = check_adopted_runner_folder(&mut port, &list, &unbound, dir).unwrap_err();
        assert!(e.contains("never bound"), "{e}");
    }

    /// Round 5 P2-3: after making the folder the entry is bound to it.
    #[test]
    fn a_made_runner_folder_is_bound_to_its_entry() {
        let mut fake = machine();
        fake.put(DATA, locked()).put("C:\\ProgramData\\Pitcrew\\runners", locked());
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\new");
        let key = "c:\\programdata\\pitcrew\\runners\\new".to_string();
        let mut port = WithIds(&mut fake, [(key, id(5, 1))].into_iter().collect());
        let mut records: Vec<Entry> = vec![];
        prepare_runner_dir(&mut port, dir, &mut |_, entry| {
            records.push(entry);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            records,
            [Entry::Pending, Entry::Bound(id(5, 1))],
            "recorded first, then bound to the folder that was made"
        );
    }

    /// Round 5 P2-3: a removed runner's entry leaves the list.
    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn a_removed_runners_folder_leaves_the_list_of_folders_the_service_made() {
        let root = std::env::temp_dir().join(format!("pitcrew-it-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join(ADOPTED_FILE);
        assert_eq!(forget_adopted(&file, Path::new("C:\\a")), Ok(false), "no list, nothing to forget");
        let mut list = Adopted::default();
        list.bind(Path::new("C:\\a"), id(1, 1));
        list.insert(Path::new("C:\\b"));
        std::fs::write(&file, list.to_json()).unwrap();
        assert_eq!(forget_adopted(&file, Path::new("c:/A/")), Ok(true));
        let after = read_adopted(&file).unwrap();
        assert!(!after.contains(Path::new("C:\\a")) && after.contains(Path::new("C:\\b")));
        assert_eq!(forget_adopted(&file, Path::new("C:\\a")), Ok(false));
        std::fs::write(&file, "garbage").unwrap();
        assert!(forget_adopted(&file, Path::new("C:\\b")).unwrap_err().contains("not the list"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Round 6 P1-B: a hard-linked `service.log` that cannot be moved aside stopped being a hard stop: SYSTEM then opened it and
    /// appended to the link target. A name the service opens is an error again; other names stay reported.
    #[test]
    fn a_hard_linked_service_log_that_cannot_be_moved_stops_the_start() {
        let build = || {
            let mut fake = machine();
            fake.put("C:\\ProgramData\\Pitcrew", locked())
                .put("C:\\ProgramData\\Pitcrew\\logs", locked())
                .put("C:\\ProgramData\\Pitcrew\\logs\\service.log", file(locked()))
                .put("C:\\ProgramData\\Pitcrew\\logs\\service.log.2", file(locked()))
                .put("C:\\ProgramData\\Pitcrew\\logs\\other.log", file(locked()));
            fake
        };
        let links = |name: &str, index: u128| (format!("c:\\programdata\\pitcrew\\logs\\{name}"), id(index, 2));
        let ids: std::collections::HashMap<_, _> = [links("service.log", 1), links("service.log.2", 2), links("other.log", 3)]
            .into_iter()
            .collect();
        let root = Path::new("C:\\ProgramData\\Pitcrew");
        for stuck in ["service.log", "service.log.2"] {
            let mut fake = build();
            let list = list_of(&fake);
            let mut failing = FailingRename(&mut fake, stuck);
            let mut port = WithIds(&mut failing, ids.clone());
            let e = ensure_protected_root(&mut port, &list, root, &["pitcrew.toml"], &["logs"]).unwrap_err();
            assert!(e.contains(stuck) && e.contains("does not start"), "{e}");
        }
        // Another name that cannot be moved is reported and the start goes on.
        let mut fake = build();
        let list = list_of(&fake);
        let mut failing = FailingRename(&mut fake, "other.log");
        let mut port = WithIds(&mut failing, ids);
        let out = ensure_protected_root(&mut port, &list, root, &["pitcrew.toml"], &["logs"]).unwrap();
        let RootOutcome::Locked(swept) = out else { panic!("{out:?}") };
        assert_eq!(swept.failed.len(), 1, "{swept:?}");
        assert_eq!(swept.moved.len(), 2, "the two service log names moved: {swept:?}");
    }

    /// Round 6 P2-1: a port that models identities and cannot say which file a path is does not pass a security decision.
    #[test]
    fn an_unknown_identity_from_a_port_that_models_identities_is_a_refusal() {
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\r");
        let mut fake = machine();
        fake.put(DATA, locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners\\r", locked());
        let list = list_of(&fake);
        let mut adopted = Adopted::default();
        adopted.bind(dir, id(1, 1));
        let mut port = WithIds(&mut fake, std::collections::HashMap::new());
        let e = check_adopted_runner_folder(&mut port, &list, &adopted, dir).unwrap_err();
        assert!(e.contains("could not be identified"), "{e}");
        // The same through the fake that models none: the path is the evidence (the existing tests rely on it).
        check_adopted_runner_folder(&mut fake, &list, &adopted, dir).expect("a fake that models no identities");
    }

    /// Round 6 P2-6: when the made folder cannot be bound the empty folder and its entry are removed again.
    #[test]
    fn a_made_folder_that_cannot_be_identified_is_removed_with_its_entry() {
        let mut fake = machine();
        fake.put(DATA, locked()).put("C:\\ProgramData\\Pitcrew\\runners", locked());
        let dir = Path::new("C:\\ProgramData\\Pitcrew\\runners\\new");
        let mut port = WithIds(&mut fake, std::collections::HashMap::new());
        let mut records: Vec<Entry> = vec![];
        let e = prepare_runner_dir(&mut port, dir, &mut |_, entry| {
            records.push(entry);
            Ok(())
        })
        .unwrap_err();
        assert!(e.contains("removed again"), "{e}");
        assert_eq!(records, [Entry::Pending, Entry::Forget]);
        assert!(
            !fake.nodes.contains_key(&fake.key("C:\\ProgramData\\Pitcrew\\runners\\new")),
            "the empty folder is gone"
        );
    }

    /// Round 6 P2-7: only the exact `<name>.untrusted-<secs>-<n>` is skipped; a stranger's look-alike is swept and reported.
    #[test]
    fn only_the_exact_aside_pattern_is_skipped_by_the_sweep() {
        for ok in ["evil.untrusted-1760000000-0", "a.b.untrusted-5-12"] {
            assert!(is_moved_aside(ok), "{ok}");
        }
        for bad in [
            "evil.untrusted-x",
            "evil.untrusted-1-",
            ".untrusted-1-2",
            "evil.untrusted-1-2-3",
            "untrusted-1-2",
            "evil.untrusted-1-2x",
        ] {
            assert!(!is_moved_aside(bad), "{bad}");
        }
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\runners", inherited_from_program_data())
            .put("C:\\ProgramData\\Pitcrew\\runners\\evil.untrusted-x", user_owned());
        let out = ensure(&mut p, &["pitcrew.toml"], &["runners"]).unwrap();
        let RootOutcome::Locked(swept) = out else { panic!("{out:?}") };
        assert_eq!(swept.moved.len(), 1, "{swept:?}");
    }

    /// Round 6 P2-8: strays in the data folder itself (a planted `.tmp`, a link) are swept like the trusted folders' children.
    #[test]
    fn a_stranger_or_link_beside_the_trusted_names_in_the_data_folder_is_moved_aside() {
        let mut link = locked();
        link.kind = FileKind::Symlink;
        let mut p = machine();
        p.put("C:\\ProgramData\\Pitcrew", locked())
            .put("C:\\ProgramData\\Pitcrew\\pitcrew.toml", file(locked()))
            .put("C:\\ProgramData\\Pitcrew\\pitcrew.toml.tmp", file(user_owned()))
            .put("C:\\ProgramData\\Pitcrew\\state.json.bak", link)
            .put("C:\\ProgramData\\Pitcrew\\fine.txt", file(locked()));
        let out = ensure(&mut p, &["pitcrew.toml"], &["logs"]).unwrap();
        let RootOutcome::Locked(swept) = out else { panic!("{out:?}") };
        let names: Vec<&str> = swept.moved.iter().map(|m| m.0.rsplit('\\').next().unwrap()).collect();
        assert_eq!(names.len(), 2, "{swept:?}");
        assert!(
            names.contains(&"pitcrew.toml.tmp") && names.contains(&"state.json.bak"),
            "{names:?}"
        );
    }

    /// A 128-bit file id does not fit a JSON number: it round-trips as hex.
    #[test]
    fn a_128_bit_file_id_survives_the_list_file() {
        let mut a = Adopted::default();
        let big = FileIdentity {
            volume: u64::MAX,
            index: u128::MAX - 5,
            links: 1,
        };
        a.bind(Path::new("C:\\a"), big);
        let back = Adopted::try_parse(&a.to_json()).unwrap();
        assert_eq!(
            back.bound(Path::new("C:\\a")).map(|i| (i.volume, i.index)),
            Some((big.volume, big.index))
        );
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
        let e = prepare_runner_dir(&mut p, dir, &mut |_, _| Err("disk full".into())).unwrap_err();
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
        assert!(Adopted::try_parse("{\"folders\":[{\"path\":\"C:\\\\a\"}]}")
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
        let line = prepare_runner_dir(&mut p, dir, &mut |d, _| {
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
            prepare_runner_dir(&mut p, dir, &mut |_, _| panic!("nothing was made")),
            Ok(None),
            "an existing folder is left alone"
        );

        // Folders above are made too, each protected at creation; only the runner's own folder is recorded.
        let deeper = Path::new("C:\\ProgramData\\Pitcrew\\runners\\a\\b");
        let mut recorded = vec![];
        prepare_runner_dir(&mut p, deeper, &mut |d, _| {
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
        let e = prepare_runner_dir(&mut p, Path::new("C:\\Users\\someone\\runner"), &mut |_, _| panic!("recorded")).unwrap_err();
        assert!(
            e.contains("does not exist yet") && e.contains("only administrators can change"),
            "{e}"
        );
        assert!(p.calls.is_empty(), "{:?}", p.calls);

        // A folder that could not be recorded is not made (round 5 P2-3: recorded first).
        let mut p = machine();
        p.put(DATA, locked()).put("C:\\ProgramData\\Pitcrew\\runners", locked());
        let e = prepare_runner_dir(&mut p, Path::new("C:\\ProgramData\\Pitcrew\\runners\\x"), &mut |_, _| {
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
        a.bind(Path::new("C:\\ProgramData\\Pitcrew\\runners\\bound"), id(3, 1));
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
