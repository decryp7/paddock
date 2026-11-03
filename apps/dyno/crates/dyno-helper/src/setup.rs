//! Installing and removing the helper, as the elevated `install` and `uninstall` commands and the daemon's own removal
//! run them. The registration, the protected copy and their checks are `paddock-service`'s (through
//! `dyno-helper-service`); this file adds what is the helper's own: the data folder and `config.json`, written before the
//! service is registered so that its first start finds them.

use crate::config::HelperConfig;
use crate::elevate::{helper_paths, this_host};
use crate::paths::HelperPaths;
use crate::serve::os_of;
use dyno_core::helper_wire::HelperUser;
use dyno_helper_service::{install_in_place, uninstall_in_place, uninstall_self_in_place, windows_roots};
use paddock_env::{Os, ProcessVars};
use paddock_service::privileged::{ProtectedFiles, RealFiles, Report};
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
    files
        .create_dir(&paths.data_dir, 0o700)
        .map_err(|e| format!("The helper's data folder {} could not be created: {e}", paths.data_dir.display()))?;
    let temp = paths.config.with_extension("json.new");
    write_private(&temp, &config.to_text()).map_err(|e| format!("{} could not be written: {e}", temp.display()))?;
    std::fs::rename(&temp, &paths.config).map_err(|e| format!("{} could not be replaced: {e}", paths.config.display()))
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
    #[cfg(unix)]
    if !crate::sys::is_elevated() {
        return Err("Installing the helper needs administrator rights; turn it on from Dyno (Settings > Helper).".into());
    }
    let source = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|e| format!("This program's own path cannot be read: {e}"))?;
    let (host, paths) = (this_host(), helper_paths());
    let mut files = RealFiles;
    let config = HelperConfig::new(owner, crate::VERSION).with_app(crate::orphan::app_location(&source));
    write_config(&paths, &config, &mut files)?;
    let roots = windows_roots(&ProcessVars);
    install_in_place(os_of(host), &roots, &source, &mut files, &system).map_err(|e| e.to_string())
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
    let report =
        uninstall_in_place(os_of(this_host()), &windows_roots(&ProcessVars), &mut RealFiles, &system).map_err(|e| e.to_string())?;
    // The program and the registration are gone: what the helper wrote next to them goes too (its log of the removal is the
    // installer's own phase list above, printed by the command).
    match remove_data_folder(&helper_paths()) {
        Ok(note) => log::info!(note = note.as_str(); "uninstall: the data folder"),
        Err(why) => log::warn!(why = why.as_str(); "uninstall: the data folder was kept"),
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
        Ok(()) => Ok("removed".into()),
        Err(_) => Ok(format!("left in place: {} holds something the helper did not write", dir.display())),
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
    uninstall_self_in_place(os, &windows_roots(&ProcessVars), &mut RealFiles, &system).map_err(|e| e.to_string())
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
        p
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
}
