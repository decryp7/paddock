//! `dev setup-harness build` against `crates/paddock-setup/nsis/test/build.sh`: the files staged beside the build, the generated
//! include and the three makensis runs (each in the output folder, `-V2`, in the script's order; two without a stub setup when
//! `SETUP_EXE` names a real one). makensis is a shim that records its arguments and folder. Unix only: the shim is a shell script.
// windows-twin: none: waived until 2026-12-31; the shim makensis is a shell script and a Windows twin needs a compiled shim
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::TestCommand;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn fixture(root: &Path) {
    let nsis = root.join("crates/paddock-setup/nsis");
    std::fs::create_dir_all(nsis.join("test")).unwrap();
    std::fs::write(nsis.join("paddock-per-machine.nsh"), "include").unwrap();
    for f in [
        "harness-hooks.nsh",
        "harness.nsi",
        "harness.txt",
        "stub-setup.nsi",
        "stub-server.nsi",
    ] {
        std::fs::write(nsis.join("test").join(f), f).unwrap();
    }
}

fn makensis(bin: &Path, log: &Path, exit: i32) {
    let p = bin.join("makensis");
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\necho \"$* in $(basename \"$(pwd)\")\" >> '{}'\nexit {exit}\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(root: &Path, bin: &Path, out: &Path, setup_exe: Option<&Path>) -> (i32, String) {
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.path_first(bin)
        .args(["setup-harness", "build", "--root", root.to_str().unwrap(), out.to_str().unwrap()]);
    if let Some(s) = setup_exe {
        cmd.env("SETUP_EXE", s);
    }
    let o = cmd.output().unwrap();
    (
        o.status.code().unwrap(),
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
    )
}

#[test]
fn the_stub_the_server_and_the_harness_are_built_from_staged_files() {
    let d = tempfile::tempdir().unwrap();
    let (root, bin, out, log) = (
        d.path().join("repo"),
        d.path().join("bin"),
        d.path().join("out"),
        d.path().join("calls.log"),
    );
    std::fs::create_dir(&bin).unwrap();
    fixture(&root);
    makensis(&bin, &log, 0);
    let (code, text) = run(&root, &bin, &out, None);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains(&format!("built in {}", out.display())));
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "-V2 stub-setup.nsi in out\n-V2 stub-server.nsi in out\n-V2 harness.nsi in out\n"
    );
    for f in [
        "paddock-per-machine.nsh",
        "harness-hooks.nsh",
        "harness.nsi",
        "harness.txt",
        "stub-setup.nsi",
        "stub-server.nsi",
    ] {
        assert!(out.join(f).is_file(), "{f} is staged");
    }
    assert_eq!(
        std::fs::read_to_string(out.join("paddock-per-machine.generated.nsh")).unwrap(),
        "!define PADDOCK_STOP_IMAGES \"harness.exe,harness-agent.exe\"\n"
    );
}

#[test]
fn a_real_setup_exe_replaces_the_stub_build() {
    let d = tempfile::tempdir().unwrap();
    let (root, bin, out, log) = (
        d.path().join("repo"),
        d.path().join("bin"),
        d.path().join("out"),
        d.path().join("calls.log"),
    );
    std::fs::create_dir(&bin).unwrap();
    fixture(&root);
    makensis(&bin, &log, 0);
    let real = d.path().join("real.exe");
    std::fs::write(&real, "REAL").unwrap();
    let (code, text) = run(&root, &bin, &out, Some(&real));
    assert_eq!(code, 0, "{text}");
    assert_eq!(std::fs::read_to_string(out.join("paddock-setup.exe")).unwrap(), "REAL");
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "-V2 stub-server.nsi in out\n-V2 harness.nsi in out\n"
    );
}

#[test]
fn a_failing_makensis_stops_the_build_with_one_error_that_names_the_script() {
    let d = tempfile::tempdir().unwrap();
    let (root, bin, out, log) = (
        d.path().join("repo"),
        d.path().join("bin"),
        d.path().join("out"),
        d.path().join("calls.log"),
    );
    std::fs::create_dir(&bin).unwrap();
    fixture(&root);
    makensis(&bin, &log, 1);
    let (code, text) = run(&root, &bin, &out, None);
    assert_eq!(code, 1);
    assert!(
        text.contains("::error::makensis failed on stub-setup.nsi: exit 1") && text.contains("Next step:"),
        "{text}"
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().lines().count(),
        1,
        "nothing runs after the failure"
    );
}
