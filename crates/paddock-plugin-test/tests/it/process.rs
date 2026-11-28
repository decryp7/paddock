//! The in-test build of a test program goes where the test program is, not into the workspace's default build folder.

use paddock_plugin_test::process::build_command;
use paddock_testkit::cargo_layout::profile_dir_for;
use std::ffi::OsStr;

#[test]
fn the_in_test_build_goes_to_the_build_folder_of_the_running_test() {
    let profile = profile_dir_for(&std::env::current_exe().unwrap()).unwrap();
    let cmd = build_command(OsStr::new("cargo"), "paddock-plugin-fixture", &profile);
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(
        args.windows(2)
            .any(|w| w == ["--bin", "paddock-plugin-fixture"]),
        "{args:?}"
    );
    // The parent's build folder, as cargo configuration: cargo exports none of the parent's `--config` to a test.
    let want = format!("build.target-dir='{}'", profile.parent().unwrap().display());
    assert!(
        args.windows(2).any(|w| w[0] == "--config" && w[1] == want),
        "the child cargo must build into the parent's build folder: want --config {want}, got {args:?}"
    );
    assert!(
        cmd.get_envs().all(|(k, _)| k != "CARGO_TARGET_DIR"),
        "sccache hashes CARGO_* variables"
    );
}
