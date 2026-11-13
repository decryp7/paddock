//! `cargo_layout`: the running test program is in one of the two build-folder layouts, and the lookups agree with it.
use paddock_testkit::cargo_layout::{example_bin, profile_dir, profile_dir_for, sibling_bin};
use std::path::Path;

#[test]
fn the_running_test_program_is_inside_the_profile_folder_it_reports() {
    let exe = std::env::current_exe().unwrap();
    let profile = profile_dir();
    assert!(
        exe.starts_with(&profile),
        "{} is not under {}",
        exe.display(),
        profile.display()
    );
    // `<profile>/examples` and `<profile>/<name>` hang off it.
    assert_eq!(sibling_bin("x").parent(), Some(profile.as_path()));
    assert_eq!(
        example_bin("x").parent().and_then(Path::parent),
        Some(profile.as_path())
    );
}

#[test]
fn a_program_outside_cargo_s_build_folders_is_refused_with_what_was_expected() {
    let e = profile_dir_for(Path::new("/usr/local/bin/t")).unwrap_err();
    assert!(
        e.contains("/usr/local/bin/t") && e.contains("<profile>/deps") && e.contains("<hash>/out"),
        "{e}"
    );
}
