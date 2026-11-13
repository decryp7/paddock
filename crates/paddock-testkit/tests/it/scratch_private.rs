//! A `Scratch` is private on every OS, so a test that gives its folders to code that refuses a folder other accounts can write
//! (a data folder, ADR 0092) works on a CI runner whose temp folder is open to `Authenticated Users` (Dyno CI run 8035).

use paddock_testkit::Scratch;

#[test]
fn windows_a_scratch_under_a_folder_other_accounts_can_write_is_still_private() {
    let parent = Scratch::new();
    let open = parent.work().join("open");
    std::fs::create_dir(&open).unwrap();
    let grant = std::process::Command::new("icacls")
        .arg(&open)
        .args(["/grant", "*S-1-5-11:(OI)(CI)M"])
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "{}",
        String::from_utf8_lossy(&grant.stdout)
    );

    let s = Scratch::try_new_in(&open).unwrap();
    for folder in [s.work(), s.home(), s.tmp()] {
        paddock_env::private::check_named_folder(&folder.join("data"))
            .unwrap_or_else(|e| panic!("{e}"));
    }
}
