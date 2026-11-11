//! `boot_volume`: the shared rule (`conformance/ci/boot-volume.sh`) with fake `df`, `diskutil` and `getconf` first on
//! `PATH`, so every case is decided by the inputs the test gives, not by where the caller's `TMPDIR` is (ER-14: the real
//! `diskutil` and `getconf` are not run).
#![cfg(unix)]

use paddock_testkit::Scratch;
use paddock_testkit::boot_volume::{boot_volume_temp_with, rule_script};
use std::ffi::OsString;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

/// The marker in a folder name that the fake `diskutil` reports as an external volume.
const EXTERNAL: &str = "EXTERNAL-VOLUME";

/// A `PATH` whose `df` names the folder itself as its mount, whose `diskutil` says Internal=false only for a mount whose
/// path holds [`EXTERNAL`], and whose `getconf` prints `user_temp` (or fails when it is `None`).
fn fake_path(s: &Scratch, user_temp: Option<&Path>) -> OsString {
    let bin = s.path("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = |name: &str, body: String| {
        let p = bin.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    tool(
        "df",
        "echo 'Filesystem 512-blocks Used Available Capacity Mounted on'\necho \"/dev/fake 1 1 1 1% $2\"\n".into(),
    );
    tool(
        "diskutil",
        format!(
            "case \"$3\" in *{EXTERNAL}*) v=false ;; *) v=true ;; esac\necho '<plist><dict><key>Internal</key>'\necho \"<$v/>\"\necho '</dict></plist>'\n"
        ),
    );
    tool(
        "getconf",
        match user_temp {
            Some(u) => format!("echo '{}/'\n", u.display()),
            None => "exit 1\n".into(),
        },
    );
    let mut path = OsString::from(bin.as_os_str());
    path.push(":/usr/bin:/bin");
    path
}

#[test]
fn a_temp_folder_on_an_external_volume_is_replaced_by_the_boot_volume_user_temp_folder() {
    let s = Scratch::new();
    let ext = s.path(EXTERNAL);
    let user = s.path("user-temp");
    std::fs::create_dir_all(&ext).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    let choice = boot_volume_temp_with(&rule_script(), &ext, Some(&fake_path(&s, Some(&user))));
    assert_eq!(choice.dir, user);
    let note = choice.note.unwrap();
    assert!(
        note.contains("external volume") && note.contains("TCC"),
        "the reason is logged (ER-27): {note}"
    );
}

#[test]
fn a_link_to_an_external_volume_is_resolved_before_the_volume_is_judged() {
    let s = Scratch::new();
    let ext = s.path(EXTERNAL);
    let user = s.path("user-temp");
    std::fs::create_dir_all(&ext).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    // The link's own path does not hold the marker; only its target does (a TMPDIR that is a link into /Volumes/...).
    let link = s.path("tmp-link");
    symlink(&ext, &link).unwrap();
    let choice = boot_volume_temp_with(&rule_script(), &link, Some(&fake_path(&s, Some(&user))));
    assert_eq!(choice.dir, user, "{:?}", choice.note);
}

#[test]
fn an_internal_temp_folder_is_kept_and_nothing_is_said() {
    let s = Scratch::new();
    let internal = s.path("internal");
    let user = s.path("user-temp");
    std::fs::create_dir_all(&internal).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    let choice =
        boot_volume_temp_with(&rule_script(), &internal, Some(&fake_path(&s, Some(&user))));
    assert_eq!((choice.dir, choice.note), (internal, None));
}

#[test]
fn without_a_boot_volume_folder_the_temp_folder_is_kept_with_a_warning_that_macos_may_prompt() {
    let s = Scratch::new();
    let ext = s.path(EXTERNAL);
    std::fs::create_dir_all(&ext).unwrap();
    for user in [None, Some(s.path(&format!("{EXTERNAL}-user")))] {
        if let Some(u) = &user {
            std::fs::create_dir_all(u).unwrap();
        }
        let choice =
            boot_volume_temp_with(&rule_script(), &ext, Some(&fake_path(&s, user.as_deref())));
        assert_eq!(choice.dir, ext);
        let note = choice.note.unwrap();
        assert!(
            note.contains("::warning::") && note.contains("may prompt"),
            "{note}"
        );
    }
}

#[test]
fn a_rule_that_cannot_run_keeps_the_temp_folder_and_says_so() {
    let s = Scratch::new();
    let temp = s.path("t");
    let choice = boot_volume_temp_with(&s.path("missing.sh"), &temp, None);
    assert_eq!(choice.dir, temp);
    assert!(choice.note.unwrap().contains("may prompt"));
}
