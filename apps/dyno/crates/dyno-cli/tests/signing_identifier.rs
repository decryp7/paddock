//! The dyno command's code-signing identifier must contain a dot, also after Tauri re-signs it with `codesign --force -s -`
//! (Dyno ADR 0026): on macOS 27.2 the kernel shows a process the whole socket list only then. Fails when the Info.plist
//! embedded by `build.rs` is lost.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[test]
fn the_dyno_command_keeps_a_dotted_identifier_when_it_is_re_signed_the_way_tauri_does() {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("dyno");
    std::fs::copy(env!("CARGO_BIN_EXE_dyno"), &copy).unwrap();
    #[allow(clippy::disallowed_methods)]
    let sign = std::process::Command::new("/usr/bin/codesign").args(["--force", "-s", "-"]).arg(&copy).output().unwrap();
    assert!(sign.status.success(), "{}", String::from_utf8_lossy(&sign.stderr));
    #[allow(clippy::disallowed_methods)]
    let shown = std::process::Command::new("/usr/bin/codesign").arg("-dv").arg(&copy).output().unwrap();
    let text = String::from_utf8_lossy(&shown.stderr).into_owned();
    let id = text.lines().find_map(|l| l.strip_prefix("Identifier=")).unwrap_or("");
    assert!(id.starts_with("net.decryptology.dyno."), "identifier is {id:?}: {text}");
}
