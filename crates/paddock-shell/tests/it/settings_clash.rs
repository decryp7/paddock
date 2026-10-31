//! The shell never touches a settings file another owner writes (Dyno's agent keeps a flat `settings.json` in the same
//! folder): the log installer reads the shell's own file and leaves `settings.json` as it is. Its own binary: it
//! installs the process-wide log.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_shell::AppIdentity;
use paddock_shell::logging::{SHELL_SETTINGS_FILE, install_in, shell_settings_path};

fn dirs_in(root: &std::path::Path) -> paddock_env::AppDirs {
    let id = paddock_env::AppId::new("demo", "Shell Demo").unwrap();
    paddock_env::AppDirs::resolve(
        &id,
        paddock_env::Os::current(),
        &paddock_env::MapVars::new().with(&id.home_var(), root),
        None,
        true,
    )
    .unwrap()
}

#[test]
fn a_foreign_format_settings_file_is_left_untouched() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_in(root.path());
    std::fs::create_dir_all(dirs.config()).unwrap();
    let foreign = dirs.config().join("settings.json");
    let flat = br#"{ "version": 1, "launchAtLogin": true }"#;
    std::fs::write(&foreign, flat).unwrap();

    let installed = install_in(
        &AppIdentity::new("demo", "Shell Demo", "1.2.3"),
        &dirs,
        false,
    );
    assert!(installed.is_some(), "the log installs");

    assert_eq!(
        std::fs::read(&foreign).unwrap(),
        flat,
        "the other owner's file is unchanged"
    );
    let leftovers: Vec<_> = std::fs::read_dir(dirs.config())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("corrupt"))
        .collect();
    assert!(leftovers.is_empty(), "nothing moved aside: {leftovers:?}");
}

#[test]
fn the_shells_file_has_its_own_name() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs_in(root.path());
    assert_eq!(
        shell_settings_path(&dirs),
        dirs.config().join(SHELL_SETTINGS_FILE)
    );
    assert_ne!(SHELL_SETTINGS_FILE, paddock_settings::FILE_NAME);
}
