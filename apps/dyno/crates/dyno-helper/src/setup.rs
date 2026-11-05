//! Installing and removing the helper, as the elevated `install` and `uninstall` commands and the daemon's own removal
//! run them. The registration, the protected copy and their checks are `paddock-service`'s (through
//! `dyno-helper-service`); this file adds what is the helper's own: the data folder and `config.json`, written before the
//! service is registered so that its first start finds them.

use crate::config::HelperConfig;
use crate::elevate::{helper_paths, this_host};
use crate::paths::HelperPaths;
use crate::serve::os_of;
use dyno_core::helper_wire::HelperUser;
use dyno_helper_service::source_check::FamilyKey;
use dyno_helper_service::{install_in_place, install_roots, preflight_in_place, uninstall_in_place, uninstall_self_in_place};
use paddock_env::Os;
use paddock_service::privileged::trust_real::RealTrust;
use paddock_service::privileged::{OsKnownFolders, ProtectedFiles, RealFiles, Report};
use paddock_service::{RealSystem, System};
use std::path::Path;

fn system() -> Box<dyn System> {
    Box::new(RealSystem::new())
}

/// Creates the data folder (root's, 0700 on Unix; on Windows the Program Files ACL: SYSTEM and Administrators full, Users
/// read) and writes `config.json` in it, replacing an older one in one step.
///
/// # Errors
/// What could not be done, in words.
pub fn write_config(paths: &HelperPaths, config: &HelperConfig, files: &mut dyn ProtectedFiles) -> Result<(), String> {
    make_lifecycle_folder_public(paths, files);
    files
        .create_dir(&paths.data_dir, 0o700)
        .map_err(|e| format!("The helper's data folder {} could not be created: {e}", paths.data_dir.display()))?;
    let temp = paths.config.with_extension("json.new");
    write_private(&temp, &config.to_text()).map_err(|e| format!("{} could not be written: {e}", temp.display()))?;
    std::fs::rename(&temp, &paths.config).map_err(|e| format!("{} could not be replaced: {e}", paths.config.display()))
}

/// The folder above the private data folder (macOS: `/Library/Application Support/Dyno`) holds the lifecycle file a standard user's
/// report reads, so it must be readable by everyone. Creating the private folder with a recursive 0700 would make its new parents
/// private too, so the parent is made first at 0755, and an older install's private parent is repaired. Linux's parent is `/var/lib`
/// (never touched); the lifecycle file lives in the unit's logs folder there.
fn make_lifecycle_folder_public(paths: &HelperPaths, files: &mut dyn ProtectedFiles) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Some(parent) = paths.data_dir.parent().filter(|p| p.file_name().is_some_and(|n| n == "Dyno")) else {
            return;
        };
        let made = files
            .create_dir(parent, 0o755)
            .and_then(|()| std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755)));
        if let Err(e) = made {
            log::warn!(Error = e.to_string().as_str(); "Helper: {} could not be made readable by everyone; a standard user's report will not show the lifecycle file", parent.display());
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (paths, files);
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// The elevated install for `owner`: the configuration first, then the protected copy of this program and its
/// registration. Run again (an update, or over an older helper) it takes the old registration over.
///
/// # Errors
/// What went wrong, with the next step.
pub fn install(owner: HelperUser) -> Result<Report, String> {
    install_recorded(owner, None)
}

/// The version `config.json` names while an install runs: the one a refresh passes (what it replaces), else what is recorded
/// now (Settings' "Update the helper" over an older helper), else this program's (a first install). Only after the install
/// succeeded does the file name this program's version, so a failed install is retried by the next refresh.
fn recorded_for_install(explicit: Option<&str>, existing: Option<String>) -> String {
    explicit
        .map(String::from)
        .or(existing)
        .unwrap_or_else(|| crate::VERSION.to_string())
}

/// [`install`], recording `recorded` as the version in `config.json` until the install has succeeded (a refresh passes the
/// version it replaces, so a failed refresh is tried again at the next update; a first install passes its own).
fn install_recorded(owner: HelperUser, recorded: Option<&str>) -> Result<Report, String> {
    #[cfg(unix)]
    if !crate::sys::is_elevated() {
        return Err("Installing the helper needs administrator rights; turn it on from Dyno (Settings > Helper).".into());
    }
    audit_run("install", "");
    let source = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|e| format!("This program's own path cannot be read: {e}"))?;
    let (host, paths) = (this_host(), helper_paths());
    let mut files = RealFiles;
    // First of all: is this the helper that may be installed (its place on Windows, its signature on macOS)? A refusal leaves no
    // folder made and no configuration written. The folders come from the OS, never from the environment (an elevated process
    // may inherit a user-writable one).
    let roots = install_roots(&OsKnownFolders).map_err(|e| e.to_string())?;
    let verifier = FamilyKey::compiled_in();
    preflight_in_place(os_of(host), &roots, &source, &mut files, &mut RealTrust, &verifier, &system).map_err(|e| e.to_string())?;
    // Repair: the data root and its tools and staging folders are locked first; one a user made is removed (links are
    // removed, never followed), so nothing in it is trusted. Then the configuration is written into the locked root.
    #[cfg(windows)]
    crate::protect::TrustGuard::system(host, &paths.data_dir)
        .prepare_folders(&paths.data_dir, crate::protect::OnUntrusted::Purge)
        .map_err(|e| e.to_string())?;
    let recorded = recorded_for_install(recorded, HelperConfig::load(&paths.config).ok().map(|c| c.version));
    let recorded = recorded.as_str();
    let app = crate::orphan::app_location(&source);
    let make = |version: &str| HelperConfig::new(owner.clone(), version).with_app(app.clone());
    // The configuration goes in before the service is registered (its first start finds it).
    let files = std::cell::RefCell::new(files);
    install_sequence(
        recorded,
        &make,
        &mut |c| write_config(&paths, c, &mut *files.borrow_mut()),
        &mut || {
            install_in_place(
                os_of(host),
                &roots,
                &source,
                &mut *files.borrow_mut(),
                &mut RealTrust,
                &verifier,
                &system,
            )
            .map_err(|e| e.to_string())
        },
    )
}

/// The order of an install: the configuration naming `recorded` first, then the install, and only after it succeeded the
/// configuration naming this program's version. A failed install leaves the old version recorded, so the next refresh
/// retries instead of believing the helper is current.
fn install_sequence(
    recorded: &str,
    make: &dyn Fn(&str) -> HelperConfig,
    write: &mut dyn FnMut(&HelperConfig) -> Result<(), String>,
    do_install: &mut dyn FnMut() -> Result<Report, String>,
) -> Result<Report, String> {
    write(&make(recorded))?;
    let report = do_install()?;
    if recorded != crate::VERSION {
        write(&make(crate::VERSION))?;
    }
    Ok(report)
}

/// What `refresh` does, decided from the version the installed helper recorded in its `config.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refresh {
    /// No configuration: the helper is not installed. A refresh never turns the helper on.
    NotInstalled,
    /// The installed helper is this version or newer: nothing to do.
    Current,
    /// The installed helper is older (or its version is unreadable): install this program in its place.
    Refresh {
        /// The version the installed helper recorded.
        from: String,
    },
}

/// The refresh decision: `recorded` is the version in `config.json` (`None`: no configuration), `this` the version of this
/// program.
pub fn refresh_decision(recorded: Option<&str>, this: &str) -> Refresh {
    match recorded {
        None => Refresh::NotInstalled,
        Some(r) if dyno_core::helper_wire::version_at_least(r, this) => Refresh::Current,
        Some(r) => Refresh::Refresh { from: r.to_string() },
    }
}

/// The elevated refresh after an update (`dyno-helper refresh`, run by the installer from the newly installed copy): when the
/// helper is installed and older than this program, installs this program over it for the owner recorded in `config.json`
/// (the policy file and the data folder stay). Nothing recorded means nothing installed: nothing is done and the helper is not
/// turned on. The same trust checks as `install` apply (the program must be in its place on Windows, signed on macOS).
///
/// # Errors
/// What went wrong, with the next step; the installer logs it and carries on (a failed refresh never fails the update).
pub fn refresh() -> Result<Report, RefreshError> {
    let paths = helper_paths();
    let host = this_host();
    // Like the daemon at start (serve.rs `lock_folders`): the data root must be protected before `config.json` is read. On
    // Windows `%ProgramData%` lets any user create folders, and a planted `config.json` would name its own owner. `ensure_root`
    // makes the data folder, empty and locked, when it is missing (a refresh on a PC without the helper leaves that empty folder
    // behind; it holds nothing and does not turn the helper on).
    let trusted = || -> Result<(), String> {
        if host != dyno_core::helper::HelperHost::Windows {
            return Ok(());
        }
        crate::protect::TrustGuard::system(host, &paths.data_dir)
            .ensure_root(&paths.data_dir, crate::protect::OnUntrusted::Refuse)
            .map_err(|e| e.to_string())
    };
    refresh_with(&trusted, &|| HelperConfig::load(&paths.config), &mut |owner, from| {
        install_recorded(owner, Some(from))
    })
}

/// Why a refresh did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshError {
    /// The helper's data folder is not protected, so `config.json` was not read and nothing was changed.
    Refused(String),
    /// Anything else (a damaged configuration, a failed install).
    Failed(String),
}

impl std::fmt::Display for RefreshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RefreshError::Refused(why) => write!(
                f,
                "The helper was not refreshed: {why}. It is left as it is; Settings > Helper offers Update the helper, which repairs the folder."
            ),
            RefreshError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for RefreshError {}

/// Says a refusal where the installer's details and the OS log can show it (the command line has no logger).
pub fn log_refusal(e: &RefreshError) {
    log::warn!("{e}");
    paddock_service::OsLog::write(
        paddock_service::os_log::open(&dyno_helper_service::spec()).as_ref(),
        log::Level::Warn,
        &e.to_string(),
    );
}

/// [`refresh`] over its three steps, so each can be replaced in a test: the trust check of the data folder, the read of
/// `config.json` (only after the check passed), and the install.
///
/// # Errors
/// [`RefreshError::Refused`] when the folder is not protected (distinct, so the installer's note can say so),
/// [`RefreshError::Failed`] for the rest.
pub fn refresh_with(
    trusted: &dyn Fn() -> Result<(), String>,
    load: &dyn Fn() -> Result<HelperConfig, crate::config::ConfigError>,
    install: &mut dyn FnMut(HelperUser, &str) -> Result<Report, String>,
) -> Result<Report, RefreshError> {
    if let Err(why) = trusted() {
        return Err(RefreshError::Refused(why));
    }
    let recorded = match load() {
        Ok(c) => Some(c),
        Err(crate::config::ConfigError::Unreadable(why)) => {
            log::info!(why = why.as_str(); "refresh: no helper configuration, nothing to refresh");
            None
        }
        Err(e @ crate::config::ConfigError::Invalid(_)) => {
            return Err(RefreshError::Failed(format!(
                "{e} The helper was left as it is; turn it off and on again in Settings > Helper."
            )));
        }
    };
    match refresh_decision(recorded.as_ref().map(|c| c.version.as_str()), crate::VERSION) {
        Refresh::NotInstalled => Ok(Report::default()),
        Refresh::Current => {
            log::info!("refresh: the installed helper is current");
            Ok(Report::default())
        }
        Refresh::Refresh { from } => {
            let owner = recorded
                .map(|c| c.owner)
                .ok_or_else(|| RefreshError::Failed("The helper's owner is not recorded.".into()))?;
            log::warn!(from = from.as_str(), to = crate::VERSION; "refresh: replacing the installed helper");
            install(owner, &from).map_err(RefreshError::Failed)
        }
    }
}

/// Says in the log and the OS log that an elevated `install` or `uninstall` is running, and from which program: a service
/// that vanishes after a restart is only explained by knowing who asked for its removal or replacement (ER-27).
fn audit_run(verb: &str, what: &str) {
    let exe = std::env::current_exe().map_or_else(|_| "?".into(), |p| p.display().to_string());
    let text = format!(
        "The Dyno helper's elevated {verb} {what} is running from {exe} (process {}).",
        std::process::id()
    );
    log::warn!("{text}");
    paddock_service::OsLog::write(
        paddock_service::os_log::open(&dyno_helper_service::spec()).as_ref(),
        log::Level::Info,
        &text,
    );
}

/// The elevated removal (the daemon could not be asked).
///
/// # Errors
/// What went wrong.
pub fn uninstall() -> Result<Report, String> {
    #[cfg(unix)]
    if !crate::sys::is_elevated() {
        return Err("Removing the helper needs administrator rights; use Dyno's Settings > Helper, or run this with sudo.".into());
    }
    audit_run("uninstall", "");
    let report = uninstall_in_place(
        os_of(this_host()),
        &install_roots(&OsKnownFolders).map_err(|e| e.to_string())?,
        &mut RealFiles,
        &system,
    )
    .map_err(|e| e.to_string())?;
    // The program and the registration are gone: what the helper wrote next to them goes too (its log of the removal is the
    // installer's own phase list above, printed by the command).
    match remove_data_folder(&helper_paths()) {
        Ok(note) => log::info!(note = note.as_str(); "uninstall: the data folder"),
        Err(why) => log::warn!(why = why.as_str(); "uninstall: the data folder was kept"),
    }
    if let Err(why) = remove_endpoint(&helper_paths()) {
        log::warn!(why = why.as_str(); "uninstall: the endpoint was kept");
    }
    Ok(report)
}

/// The helper's own files in its data folder (a name or a prefix of the rotated logs).
const OWN_FILES: [&str; 3] = ["config.json", "audit.jsonl", "helper.log"];
/// The helper's own folders in its data folder (the installed tools and the install staging).
const OWN_DIRS: [&str; 2] = ["tools", "staging"];

/// Removes what the helper wrote into its data folder (`config.json`, the audit log, the logs, the tools and the staging
/// folder) and then the folder itself when nothing else is in it. A folder that is a link is refused (never followed), anything
/// else in it stays and is said so. Returns what happened in words for the log.
///
/// # Errors
/// What could not be removed.
pub fn remove_data_folder(paths: &HelperPaths) -> Result<String, String> {
    remove_data_folder_with(paths, &crate::logfile::quiesce_installed)
}

/// [`remove_data_folder`] with the step that stops the helper's own log writing, which runs before anything is removed: a log
/// line from another thread would otherwise recreate `helper.log` between the unlink and the folder's removal and leave the
/// folder behind.
///
/// # Errors
/// What could not be removed.
pub fn remove_data_folder_with(paths: &HelperPaths, quiesce_log: &dyn Fn()) -> Result<String, String> {
    quiesce_log();
    // The lifecycle file lives beside the data folder (macOS) or in the logs folder (Linux): it goes with the helper, before the
    // folder above it is tried.
    if let Some(file) = &paths.lifecycle {
        // Best effort: a file that cannot be removed never stops the data folder and the audit log from going.
        match crate::lifecycle::remove(file) {
            Ok(()) => {
                if let Some(parent) = file.parent().filter(|p| p.file_name().is_some_and(|n| n == "dyno-helper")) {
                    // Linux: the unit's LogsDirectory (empty now); a folder with something else in it stays.
                    if let Err(e) = std::fs::remove_dir(parent) {
                        log::debug!(Error = e.to_string().as_str(); "Helper: the logs folder stays");
                    }
                }
            }
            Err(e) => log::warn!(Error = e.to_string().as_str(); "Helper: {} could not be removed", file.display()),
        }
    }
    let dir = &paths.data_dir;
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok("was not there".into()),
        Err(e) => return Err(format!("{} cannot be read: {e}", dir.display())),
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "{} is not the helper's own folder (a link or not a folder), so it was not touched",
            dir.display()
        ));
    }
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{} cannot be read: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let own_file = kind.is_file() && OWN_FILES.iter().any(|n| name == *n || name.starts_with(&format!("{n}.")));
        let own_dir = kind.is_dir() && OWN_DIRS.contains(&name.as_str());
        let path = entry.path();
        if own_file {
            std::fs::remove_file(&path).map_err(|e| format!("{} could not be removed: {e}", path.display()))?;
        } else if own_dir {
            std::fs::remove_dir_all(&path).map_err(|e| format!("{} could not be removed: {e}", path.display()))?;
        }
    }
    match std::fs::remove_dir(dir) {
        Ok(()) => {
            // `.../Dyno` holds the helper's folder and nothing else of ours: it goes when it is empty (never with something in it).
            if let Some(parent) = dir.parent().filter(|p| p.file_name().is_some_and(|n| n == "Dyno")) {
                let _ = std::fs::remove_dir(parent);
            }
            Ok("removed".into())
        }
        Err(e) => Ok(left_in_place_note(dir, &e, std::fs::read_dir(dir).map(Iterator::count))),
    }
}

/// What the log says when the data folder could not be removed: the real error, and whether the folder holds items (someone
/// else's), is empty (a service sandbox with a read-only parent: the unit has the service manager remove it after the stop), or
/// could not be listed (then nothing is claimed about its content).
fn left_in_place_note(dir: &Path, err: &std::io::Error, remaining: std::io::Result<usize>) -> String {
    let dir = dir.display();
    match remaining {
        Ok(0) => format!("left in place: {dir} is empty but could not be removed from here ({err}); systemd removes it after the stop"),
        Ok(n) => format!("left in place: {dir} holds {n} item(s) the helper did not write ({err})"),
        Err(list) => format!("left in place: {dir} could not be removed ({err}) and could not be listed ({list})"),
    }
}

/// Removes the helper's socket file (only a socket) and its folder when empty: a killed service cannot unlink its own socket.
///
/// # Errors
/// What could not be removed.
pub fn remove_endpoint(paths: &HelperPaths) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let socket = Path::new(&paths.endpoint);
        match std::fs::symlink_metadata(socket) {
            Ok(m) if m.file_type().is_socket() => {
                std::fs::remove_file(socket).map_err(|e| format!("{} could not be removed: {e}", socket.display()))?;
            }
            Ok(_) => return Ok(format!("left in place: {} is not a socket", socket.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} cannot be read: {e}", socket.display())),
        }
        if let Some(folder) = &paths.endpoint_dir {
            let _ = std::fs::remove_dir(folder);
        }
        Ok("removed".into())
    }
    #[cfg(not(unix))]
    {
        let _ = paths;
        Ok("nothing to remove (a pipe goes with its process)".into())
    }
}

/// The daemon removing itself after an `Uninstall` request: the files first, the stop of its own job last.
///
/// # Errors
/// What went wrong.
pub fn uninstall_self(os: Os) -> Result<Report, String> {
    // The last step ends this process, so what the helper wrote goes first (a file this process holds open may stay: said in the log).
    match remove_data_folder(&helper_paths()) {
        Ok(note) => log::info!(note = note.as_str(); "self-uninstall: the data folder"),
        Err(why) => log::warn!(why = why.as_str(); "self-uninstall: the data folder was kept"),
    }
    // The stop that ends this process cannot unlink the socket it serves on: remove it now (the listener's next accept fails once).
    if let Err(why) = remove_endpoint(&helper_paths()) {
        log::warn!(why = why.as_str(); "self-uninstall: the endpoint was kept");
    }
    let roots = install_roots(&OsKnownFolders).map_err(|e| e.to_string())?;
    uninstall_self_in_place(os, &roots, &mut RealFiles, &system).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper::HelperHost;
    use dyno_core::helper_paths::{HelperPaths, WindowsRoots};
    use paddock_service::testing::FakeFiles;
    use std::path::PathBuf;

    fn paths_in(dir: &Path) -> HelperPaths {
        let mut p = HelperPaths::for_host(
            HelperHost::Linux,
            &WindowsRoots {
                program_files: PathBuf::new(),
                program_data: PathBuf::new(),
                system_root: PathBuf::new(),
            },
        );
        p.data_dir = dir.join("data");
        p.config = dir.join("data").join("config.json");
        // Never the machine's real lifecycle file (ER-14): removal tests delete it.
        p.lifecycle = Some(dir.join("lifecycle").join("lifecycle.log"));
        p
    }

    #[cfg(unix)]
    #[test]
    fn the_folder_above_the_private_one_is_readable_by_everyone_so_a_standard_user_can_read_the_lifecycle_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut paths = paths_in(dir.path());
        // The macOS layout: /Library/Application Support/Dyno/Helper (private) beside Dyno/helper-lifecycle.log (public).
        paths.data_dir = dir.path().join("Dyno").join("Helper");
        paths.config = paths.data_dir.join("config.json");
        paths.lifecycle = Some(dir.path().join("Dyno").join("helper-lifecycle.log"));
        write_config(&paths, &HelperConfig::new(HelperUser::Uid(501), "1.1.0"), &mut RealFiles).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&paths.data_dir), 0o700, "the helper's own folder stays private");
        assert_eq!(
            mode(paths.data_dir.parent().unwrap()),
            0o755,
            "the folder the lifecycle file sits in is readable by all"
        );
        // An install over an older one that left the parent private repairs it.
        std::fs::set_permissions(paths.data_dir.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
        write_config(&paths, &HelperConfig::new(HelperUser::Uid(501), "1.1.1"), &mut RealFiles).unwrap();
        assert_eq!(mode(paths.data_dir.parent().unwrap()), 0o755);
    }

    #[test]
    fn a_lifecycle_file_that_cannot_be_removed_does_not_stop_the_rest_of_the_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = paths_in(dir.path());
        // A directory in the place of the file: removing it as a file fails.
        let blocked = dir.path().join("blocked-lifecycle");
        std::fs::create_dir_all(blocked.join("inside")).unwrap();
        paths.lifecycle = Some(blocked);
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::write(paths.data_dir.join("config.json"), "{}").unwrap();
        std::fs::write(paths.data_dir.join("audit.jsonl"), "").unwrap();
        remove_data_folder_with(&paths, &|| {}).expect("a best-effort file never aborts the removal");
        assert!(!paths.data_dir.exists(), "the data folder and the audit log still went");
    }

    #[test]
    fn the_logs_folder_the_unit_made_goes_with_the_lifecycle_file_but_not_with_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = paths_in(dir.path());
        let logs = dir.path().join("var-log").join("dyno-helper");
        std::fs::create_dir_all(&logs).unwrap();
        paths.lifecycle = Some(logs.join("lifecycle.log"));
        std::fs::write(logs.join("lifecycle.log"), "t=1 stopped pid=1 version=1\n").unwrap();
        remove_data_folder_with(&paths, &|| {}).unwrap();
        assert!(!logs.exists(), "an empty dyno-helper logs folder is removed");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("lifecycle.log"), "x").unwrap();
        std::fs::write(logs.join("someone-elses.log"), "keep").unwrap();
        remove_data_folder_with(&paths, &|| {}).unwrap();
        assert!(
            logs.join("someone-elses.log").exists() && logs.exists(),
            "another file keeps the folder"
        );
    }

    #[test]
    fn uninstall_removes_the_lifecycle_file_and_its_empty_logs_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let file = paths.lifecycle.clone().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "t=1 stopped pid=1 version=1\n").unwrap();
        remove_data_folder_with(&paths, &|| {}).unwrap();
        assert!(!file.exists());
        // A missing file is fine (a second uninstall).
        remove_data_folder_with(&paths, &|| {}).unwrap();
    }

    #[test]
    fn the_configuration_is_written_private_and_replaced_in_one_step() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        let files = FakeFiles::new();
        let mut fake = files.clone();
        let cfg = HelperConfig::new(HelperUser::Uid(501), "1.1.0");
        write_config(&paths, &cfg, &mut fake).unwrap();
        assert_eq!(HelperConfig::load(&paths.config).unwrap(), cfg);
        assert!(files.calls().iter().any(|c| c.starts_with("create_dir ") && c.ends_with(" 700")));
        let newer = HelperConfig::new(HelperUser::Sid("S-1-5-21-1".into()), "1.2.0");
        write_config(&paths, &newer, &mut fake).unwrap();
        assert_eq!(HelperConfig::load(&paths.config).unwrap(), newer);
        assert!(!paths.config.with_extension("json.new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&paths.config).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn a_folder_that_cannot_be_made_or_a_config_that_cannot_be_written_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let files = FakeFiles::new();
        files.fail_with(Some(std::io::ErrorKind::PermissionDenied));
        let e = write_config(&paths, &HelperConfig::new(HelperUser::Uid(1), "1"), &mut files.clone()).unwrap_err();
        assert!(e.contains("data folder"), "{e}");
        // The folder step is fine but the real disk has no such folder: the file cannot be written.
        let e = write_config(&paths, &HelperConfig::new(HelperUser::Uid(1), "1"), &mut FakeFiles::new()).unwrap_err();
        assert!(e.contains("could not be written"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn install_and_uninstall_refuse_to_run_without_administrator_rights() {
        if crate::sys::is_elevated() {
            return;
        }
        assert!(install(HelperUser::Uid(1)).unwrap_err().contains("administrator"));
        assert!(uninstall().unwrap_err().contains("administrator"));
    }

    #[test]
    fn the_hosts_map_to_the_oses() {
        assert_eq!(os_of(HelperHost::Macos), Os::MacOs);
        assert_eq!(os_of(HelperHost::Windows), Os::Windows);
        assert_eq!(os_of(HelperHost::Linux), Os::Linux);
    }

    #[test]
    fn a_thread_logging_while_the_data_folder_is_removed_cannot_leave_the_folder_behind() {
        // Another thread's log line recreating helper.log between the unlink and the folder's removal left the folder behind
        // at Turn off. The removal stops the log first.
        use log::{Level, Log, Record};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        for round in 0..40 {
            let dir = tempfile::tempdir().unwrap();
            let mut paths = paths_in(dir.path());
            paths.log = paths.data_dir.join("helper.log");
            std::fs::create_dir_all(&paths.data_dir).unwrap();
            std::fs::write(paths.data_dir.join("config.json"), b"x").unwrap();
            let logger = Arc::new(crate::logfile::HelperLog::new(paths.log.clone()).with(1 << 20, || 7));
            let gate = logger.gate();
            let stop = Arc::new(AtomicBool::new(false));
            // A failing assertion must not leave the logging thread running.
            struct StopOnDrop(Arc<AtomicBool>);
            impl Drop for StopOnDrop {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::SeqCst);
                }
            }
            let _stop_on_exit = StopOnDrop(stop.clone());
            let worker = {
                let (logger, stop) = (logger.clone(), stop.clone());
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        logger.log(&Record::builder().args(format_args!("a line")).level(Level::Info).build());
                    }
                })
            };
            // The thread is writing when the removal starts.
            let waited = std::time::Instant::now();
            while !paths.log.exists() {
                assert!(waited.elapsed().as_secs() < 20, "the logging thread never wrote its file");
                std::thread::yield_now();
            }
            let note = remove_data_folder_with(&paths, &|| gate.quiesce()).unwrap();
            stop.store(true, Ordering::SeqCst);
            worker.join().unwrap();
            assert_eq!(note, "removed", "round {round}");
            assert!(!paths.data_dir.exists(), "round {round}: the folder is gone");
        }
    }

    /// VM check 7538: the folder was empty but could not be removed (the service's `/var/lib` is read-only), and the log said it
    /// "holds something the helper did not write": the error was swallowed and the cause hidden.
    #[cfg(unix)]
    #[test]
    fn an_empty_folder_that_cannot_be_removed_says_the_real_error_not_that_it_holds_something() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        // The parent refuses the removal of an entry (as the service's read-only `/var/lib` does).
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::create_dir(dir.path().join("probe")).is_ok() {
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            return; // running as root: a read-only parent does not stop it
        }
        let note = remove_data_folder_with(&paths, &|| {});
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let note = note.unwrap();
        assert!(note.contains("left in place"), "{note}");
        assert!(!note.contains("holds something"), "the folder is empty: {note}");
        assert!(
            note.contains("empty") && note.contains("os error"),
            "the real error is named: {note}"
        );
    }

    #[test]
    fn a_folder_that_cannot_be_listed_is_not_called_empty_and_an_empty_one_names_systemd() {
        let dir = Path::new("/var/lib/dyno-helper");
        let err = std::io::Error::from_raw_os_error(30);
        let unlisted = left_in_place_note(dir, &err, Err(std::io::Error::from_raw_os_error(13)));
        assert!(
            unlisted.contains("could not be listed") && !unlisted.contains("empty"),
            "{unlisted}"
        );
        let empty = left_in_place_note(dir, &err, Ok(0));
        assert!(empty.contains("systemd removes it after the stop"), "{empty}");
        assert!(left_in_place_note(dir, &err, Ok(2)).contains("holds 2 item(s)"));
    }

    #[test]
    fn a_foreign_file_still_keeps_the_folder_in_place_with_the_log_quiesced() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::write(paths.data_dir.join("notes.txt"), b"someone else's").unwrap();
        let note = remove_data_folder_with(&paths, &|| {}).unwrap();
        assert!(note.contains("left in place"), "{note}");
    }

    #[test]
    fn removing_the_data_folder_takes_only_the_helpers_own_files_and_the_folder_when_it_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        std::fs::create_dir_all(paths.data_dir.join("tools").join("smartmontools-7.5")).unwrap();
        std::fs::create_dir_all(paths.data_dir.join("staging")).unwrap();
        for f in [
            "config.json",
            "audit.jsonl",
            "helper.log",
            "helper.log.1",
            "tools/smartmontools-7.5/smartctl",
            "staging/setup.exe",
        ] {
            std::fs::write(paths.data_dir.join(f), b"x").unwrap();
        }
        std::fs::write(paths.data_dir.join("notes.txt"), b"someone else's").unwrap();
        let note = remove_data_folder(&paths).unwrap();
        assert!(note.contains("left in place"), "{note}");
        let left: Vec<_> = std::fs::read_dir(&paths.data_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["notes.txt"], "only what the helper wrote went: {left:?}");
        std::fs::remove_file(paths.data_dir.join("notes.txt")).unwrap();
        assert_eq!(remove_data_folder(&paths).unwrap(), "removed");
        assert!(!paths.data_dir.exists());
        assert_eq!(remove_data_folder(&paths).unwrap(), "was not there");
    }

    #[cfg(unix)]
    #[test]
    fn a_data_folder_that_is_a_link_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(dir.path());
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("config.json"), b"not ours").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &paths.data_dir).unwrap();
        assert!(remove_data_folder(&paths).is_err());
        assert!(elsewhere.join("config.json").exists());
    }

    #[test]
    fn the_empty_dyno_parent_folder_goes_with_the_data_but_never_something_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = paths_in(dir.path());
        paths.data_dir = dir.path().join("Dyno").join("Helper");
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::write(paths.data_dir.join("config.json"), b"x").unwrap();
        assert_eq!(remove_data_folder(&paths).unwrap(), "removed");
        assert!(!dir.path().join("Dyno").exists(), "the empty parent went too");
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::write(dir.path().join("Dyno").join("Other app"), b"y").unwrap();
        assert_eq!(remove_data_folder(&paths).unwrap(), "removed");
        assert!(dir.path().join("Dyno").join("Other app").exists(), "what else lives there stays");
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_socket_and_its_empty_folder_are_removed_but_a_file_that_is_not_a_socket_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = paths_in(dir.path());
        let run = dir.path().join("run").join("dyno-helper");
        std::fs::create_dir_all(&run).unwrap();
        paths.endpoint = run.join("helper.sock").to_string_lossy().into_owned();
        paths.endpoint_dir = Some(run.clone());
        let listener = std::os::unix::net::UnixListener::bind(&paths.endpoint).unwrap();
        drop(listener); // the file stays, as when the process is killed
        assert_eq!(remove_endpoint(&paths).unwrap(), "removed");
        assert!(!run.exists());
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(&paths.endpoint, b"not a socket").unwrap();
        assert!(remove_endpoint(&paths).unwrap().starts_with("left in place"));
        assert!(std::path::Path::new(&paths.endpoint).exists());
    }
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    #[test]
    fn nothing_recorded_means_the_helper_is_not_installed_and_is_never_turned_on() {
        assert_eq!(refresh_decision(None, "1.1.0"), Refresh::NotInstalled);
    }

    #[test]
    fn the_same_version_is_current_and_left_alone() {
        assert_eq!(refresh_decision(Some("1.1.0"), "1.1.0"), Refresh::Current);
    }

    #[test]
    fn an_older_or_unreadable_recorded_version_is_refreshed() {
        assert_eq!(refresh_decision(Some("1.0.0"), "1.1.0"), Refresh::Refresh { from: "1.0.0".into() });
        assert_eq!(refresh_decision(Some("junk"), "1.1.0"), Refresh::Refresh { from: "junk".into() });
    }

    #[test]
    fn a_newer_recorded_version_is_not_downgraded() {
        assert_eq!(refresh_decision(Some("1.2.0"), "1.1.0"), Refresh::Current);
    }
}

#[cfg(test)]
mod refresh_flow_tests {
    use super::*;
    use crate::config::ConfigError;
    use std::cell::{Cell, RefCell};

    fn config(version: &str) -> HelperConfig {
        HelperConfig::new(HelperUser::Uid(501), version)
    }

    #[test]
    fn a_config_in_an_untrusted_folder_is_never_read_and_nothing_is_installed() {
        let loaded = Cell::new(false);
        let installed = Cell::new(false);
        let r = refresh_with(
            &|| Err("the data folder is not protected".to_string()),
            &|| {
                loaded.set(true);
                Ok(config("0.0.1"))
            },
            &mut |_, _| {
                installed.set(true);
                Ok(Report::default())
            },
        );
        let Err(RefreshError::Refused(why)) = r else {
            panic!("a refused refresh must not look like success: {r:?}")
        };
        assert!(why.contains("not protected"), "{why}");
        assert!(!loaded.get(), "a planted config.json is not even read");
        assert!(!installed.get(), "a planted owner never becomes the helper's owner");
    }

    #[test]
    fn a_refusal_has_its_own_exit_code_and_a_sentence_for_the_installer_to_print() {
        use crate::cli::refresh_exit;
        assert_eq!(refresh_exit(&Ok(Report::default())), 0);
        assert_eq!(refresh_exit(&Err(RefreshError::Failed("x".into()))), 1);
        let refused = RefreshError::Refused("the data folder is not protected".into());
        assert_eq!(
            refresh_exit(&Err(refused.clone())),
            3,
            "distinct and non-zero, so the hook's NOTE runs"
        );
        let s = refused.to_string();
        assert!(s.contains("not protected") && s.contains("Settings"), "{s}");
    }

    #[test]
    fn the_version_recorded_during_an_install_is_the_old_one_when_there_is_one() {
        // A refresh passes the version it replaces; Settings' install reads what is recorded; a first install has none.
        assert_eq!(recorded_for_install(Some("0.0.1"), Some("9.9.9".into())), "0.0.1");
        assert_eq!(recorded_for_install(None, Some("0.5.0".into())), "0.5.0");
        assert_eq!(recorded_for_install(None, None), crate::VERSION);
    }

    #[test]
    fn a_trusted_older_helper_is_refreshed_for_the_recorded_owner() {
        let seen = RefCell::new(None);
        let r = refresh_with(&|| Ok(()), &|| Ok(config("0.0.1")), &mut |o, from| {
            *seen.borrow_mut() = Some((o, from.to_string()));
            Ok(Report::default())
        });
        assert!(r.is_ok());
        assert_eq!(seen.into_inner(), Some((HelperUser::Uid(501), "0.0.1".to_string())));
    }

    #[test]
    fn no_configuration_or_a_current_one_installs_nothing_and_a_damaged_one_is_reported() {
        let installed = Cell::new(0);
        let mut count = |_: HelperUser, _: &str| {
            installed.set(installed.get() + 1);
            Ok(Report::default())
        };
        assert!(refresh_with(&|| Ok(()), &|| Err(ConfigError::Unreadable("missing".into())), &mut count).is_ok());
        assert!(refresh_with(&|| Ok(()), &|| Ok(config(crate::VERSION)), &mut count).is_ok());
        let e = refresh_with(&|| Ok(()), &|| Err(ConfigError::Invalid("damaged".into())), &mut count)
            .unwrap_err()
            .to_string();
        assert!(e.contains("damaged") && e.contains("Settings"), "{e}");
        assert_eq!(installed.get(), 0);
    }

    #[test]
    fn the_new_version_is_recorded_only_after_the_install_succeeded() {
        let writes = RefCell::new(Vec::new());
        let mut write = |c: &HelperConfig| {
            writes.borrow_mut().push(c.version.clone());
            Ok(())
        };
        let make = |v: &str| config(v);
        // A refresh over 0.0.1: the old version stays on disk until the install is done.
        let ok = install_sequence("0.0.1", &make, &mut write, &mut || Ok(Report::default()));
        assert!(ok.is_ok());
        assert_eq!(*writes.borrow(), ["0.0.1".to_string(), crate::VERSION.to_string()]);
        // A failed install leaves the old version recorded, so the next update refreshes again.
        writes.borrow_mut().clear();
        let bad = install_sequence("0.0.1", &make, &mut write, &mut || Err("locked".to_string()));
        assert_eq!(bad, Err("locked".to_string()));
        assert_eq!(*writes.borrow(), ["0.0.1".to_string()]);
        // A first install records its own version once.
        writes.borrow_mut().clear();
        install_sequence(crate::VERSION, &make, &mut write, &mut || Ok(Report::default())).unwrap();
        assert_eq!(*writes.borrow(), [crate::VERSION.to_string()]);
    }
}
