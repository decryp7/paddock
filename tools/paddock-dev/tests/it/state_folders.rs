//! ER-14, ER-51, ER-21: a tool's state folder that would default under the repository's git common dir (shared by every worktree and
//! held by real gates) has a test-only setting in the register, and the test harness sets every one of them to the test's own folder.
//! The pre-push state folder was one: a test waited 900 s for a build-folder lock a real gate held (train aa member 6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_ci_config::settings::declared::{DECLARED, Scope};
use paddock_testkit::TestCommand;
use paddock_testkit::process::STATE_FOLDERS;

/// A state-folder setting is a test-only path row whose name ends in `_STATE`.
fn declared_state_folders() -> Vec<&'static str> {
    DECLARED
        .iter()
        .filter(|d| d.scope == Scope::TestOnly && d.name.ends_with("_STATE"))
        .map(|d| d.name)
        .collect()
}

#[test]
fn every_declared_state_folder_setting_is_one_the_harness_sets() {
    let declared = declared_state_folders();
    // Positive control: the register does declare some, so an empty answer is the check being blind.
    assert!(declared.len() >= 2, "no state-folder settings found in the register: {declared:?}");
    for name in &declared {
        assert!(
            STATE_FOLDERS.contains(name),
            "{name} is a declared state-folder setting the test harness does not set: add it to paddock_testkit::process::STATE_FOLDERS"
        );
    }
    for name in STATE_FOLDERS {
        assert!(
            declared.contains(name),
            "{name} is set by the harness but not declared (TestOnly, `_STATE`) in the settings register"
        );
    }
}

#[test]
fn a_test_command_gets_every_state_folder_inside_its_own_private_folder() {
    let cmd = TestCommand::new("true");
    let envs: std::collections::HashMap<_, _> = cmd.get_envs().collect();
    for name in STATE_FOLDERS {
        let value = envs
            .get(std::ffi::OsStr::new(name))
            .and_then(|v| *v)
            .unwrap_or_else(|| panic!("{name} is not set by the harness"));
        let path = std::path::Path::new(value);
        assert!(
            path.starts_with(cmd.private_dir()),
            "{name} = {} is outside the private folder",
            path.display()
        );
        assert!(path.is_dir(), "{name} = {} does not exist", path.display());
    }
}
