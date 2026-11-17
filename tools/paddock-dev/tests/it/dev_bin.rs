//! The one way a test starts the `paddock-dev` binary under test (ER-45).
//!
//! The binary prepares the local sccache wrapper (installs nothing, but starts the one shared server on the default port with the
//! machine's cache folder) the first time a cargo step runs through it. A test must never do that on the machine it runs on, so
//! every spawn goes through [`command`] / [`harden`], which set `PADDOCK_SCCACHE=off`, and no test file names the binary any other
//! way: the audit below fails on a direct use of `CARGO_BIN_EXE_paddock-dev`. A review found real tests (merge_check's hakari and
//! member-gate modules) that started the shared server because they spawned the binary by hand.
#![allow(clippy::unwrap_used)]
use paddock_testkit::TestCommand;

/// The path of the binary under test. For an argument or an environment value; to start it use [`command`].
// conformance: test-env — a path for arguments and environment values; `command` below starts the binary through a TestCommand
pub const PATH: &str = env!("CARGO_BIN_EXE_paddock-dev");

/// The variable that turns the wrapper off.
pub const OFF: (&str, &str) = ("PADDOCK_SCCACHE", "off");

/// Sets the wrapper off on a command that starts the binary (or a fixture's command that may).
pub fn harden(cmd: &mut TestCommand) -> &mut TestCommand {
    cmd.env(OFF.0, OFF.1)
}

/// A command that starts the binary under test: a [`TestCommand`] (ADR 0100: a listed environment and private folders, started
/// through `paddock_env::command::command` like `paddock_proc::command`), with the wrapper off.
pub fn command() -> TestCommand {
    let mut cmd = TestCommand::new(PATH);
    harden(&mut cmd);
    cmd
}

#[test]
fn a_spawned_binary_has_the_wrapper_off() {
    let cmd = command();
    let envs: Vec<_> = cmd.get_envs().collect();
    assert!(
        envs.iter().any(|(k, v)| *k == OFF.0 && *v == Some(std::ffi::OsStr::new(OFF.1))),
        "{envs:?}"
    );
    // And it really declines: no warning, no server, nothing printed.
    let out = command().args(["sccache", "env"]).env_remove("CI").output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("::warning::local-sccache"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The audit: only this file names the binary, so every spawn is hardened. A new test file that names it directly fails here and
/// is told what to use.
#[test]
fn no_test_file_starts_the_binary_without_the_helper() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let needle = ["CARGO_BIN_EXE", "paddock-dev"].join("_");
    let mut bad = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "dev_bin.rs") {
            let text = std::fs::read_to_string(&p).unwrap();
            if text.contains(&needle) || text.contains("CARGO_BIN_EXE_paddock-dev") {
                bad.push(p.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    bad.sort();
    assert!(
        bad.is_empty(),
        "these test files name the binary directly; use crate::dev_bin::command() (or PATH for an argument): {bad:?}"
    );
}
