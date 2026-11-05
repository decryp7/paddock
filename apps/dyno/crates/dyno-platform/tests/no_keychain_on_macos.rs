#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The macOS build of Dyno has no Keychain adapter at all (ADR 0032 amendments, 2026-11-05): a Keychain read waits behind a password
//! dialog with no time limit after every update of an ad-hoc signed build, so no code path may be able to make one.
//!
//! The probe asks `cargo tree -i keyring` for each target. Its precondition (ER-33) is checked first: the Windows
//! target must show `keyring`, otherwise the probe cannot see the thing and the check reports `check broken`, never a pass.

use std::path::Path;
use std::time::Duration;

/// Whether `cargo tree -i keyring` finds the crate in what `apps/dyno` builds for `target`. An error means the probe itself failed.
fn has_keyring(target: &str) -> Result<bool, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    // Offline first (the packages of a built tree are cached); online when a target's packages were never fetched here.
    let run = |offline: bool| {
        let mut cmd = dyno_platform::process::command(&cargo);
        cmd.args(["tree", "--invert", "keyring", "--prefix", "none", "--locked", "--target", target, "--manifest-path"]).arg(&manifest);
        if offline {
            cmd.arg("--offline");
        }
        dyno_platform::process::run_with_timeout(cmd, Duration::from_secs(300)).map_err(|e| format!("cargo tree could not run: {e}"))
    };
    let mut out = run(true)?;
    if !out.status.success() {
        out = run(false)?;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    // A graph without the crate prints "nothing to print" (exit 0) or says the id matched nothing; any other failure is the probe's.
    if stdout.lines().any(|l| l.starts_with("keyring v")) {
        return Ok(true);
    }
    if out.status.success() || stderr.contains("did not match any packages") || stderr.contains("nothing to print") {
        return Ok(false);
    }
    Err(format!("cargo tree failed for {target}: {}", stderr.trim()))
}

#[test]
fn the_probe_sees_the_keychain_adapter_where_it_is_built_and_the_macos_graph_has_none() {
    // Positive control: where the OS store is used, the probe must see it.
    assert_eq!(has_keyring("x86_64-pc-windows-msvc"), Ok(true), "check broken: the probe cannot see `keyring` for Windows");
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assert_eq!(
            has_keyring(target),
            Ok(false),
            "`keyring` is built into Dyno for {target}: a Keychain read would wait behind a password dialog after every update. \
             Find who enables paddock-secrets' `keyring` feature with `cargo tree -i keyring --target {target}` and remove it"
        );
    }
}
