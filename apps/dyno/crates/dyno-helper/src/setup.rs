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
    uninstall_in_place(os_of(this_host()), &windows_roots(&ProcessVars), &mut RealFiles, &system).map_err(|e| e.to_string())
}

/// The daemon removing itself after an `Uninstall` request: the files first, the stop of its own job last.
///
/// # Errors
/// What went wrong.
pub fn uninstall_self(os: Os) -> Result<Report, String> {
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
}
