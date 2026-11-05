#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The macOS build of Dyno has no Keychain adapter at all (ADR 0032 amendments, 2026-11-05): a Keychain read waits behind a password
//! dialog with no time limit after every update of an ad-hoc signed build, so no code path may be able to make one.
//!
//! Two probes, neither of which uses the network (a cold runner would fetch hundreds of MB, and a blip must not fail CI):
//! 1. A scan of the manifests of the crates Dyno builds (the path packages of `Cargo.lock`): every `paddock-secrets` dependency that
//!    turns on `keyring` must sit in a table for another OS. Its positive control (ER-33) is that it finds `dyno-platform`'s own entry.
//! 2. `cargo tree --offline -i keyring` per target, when everything is already fetched. It is authoritative then, with the Windows
//!    graph as its positive control. When it cannot run offline it says `not measured` and probe 1 stands alone.

use std::path::{Path, PathBuf};
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

/// Names of the packages of `apps/dyno/Cargo.lock` that are not from a registry (the workspace's own crates and path dependencies).
fn local_crates() -> Vec<String> {
    let lock = std::fs::read_to_string(root().join("apps/dyno/Cargo.lock")).unwrap();
    lock.split("[[package]]")
        .skip(1)
        .filter(|block| !block.contains("\nsource = "))
        .filter_map(|block| {
            block
                .lines()
                .find_map(|l| l.strip_prefix("name = "))
                .map(|n| n.trim_matches('"').to_string())
        })
        .collect()
}

fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        let name = entry.file_name();
        if p.is_dir() && name != "target" && name != "node_modules" && name != ".git" {
            manifests(&p, out);
        } else if name == "Cargo.toml" {
            out.push(p);
        }
    }
}

/// `(manifest, table header)` of every `paddock-secrets` dependency in the Dyno graph that enables `keyring`.
fn keyring_enablers() -> Vec<(String, String)> {
    let local = local_crates();
    let mut files = Vec::new();
    manifests(&root().join("crates"), &mut files);
    manifests(&root().join("apps/dyno"), &mut files);
    let mut found = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let own = text
            .lines()
            .find_map(|l| l.strip_prefix("name = "))
            .map(|n| n.trim_matches('"').to_string());
        if !own.is_some_and(|n| local.contains(&n)) {
            continue;
        }
        let mut table = String::new();
        let mut in_secrets_table = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                table = line.to_string();
                in_secrets_table = line.ends_with(".paddock-secrets]");
            } else if (in_secrets_table || line.starts_with("paddock-secrets")) && line.contains("keyring") && !line.starts_with('#') {
                found.push((file.display().to_string(), table.clone()));
            }
        }
    }
    found
}

#[test]
fn only_tables_for_other_systems_turn_the_keychain_adapter_on() {
    let found = keyring_enablers();
    assert!(
        found
            .iter()
            .any(|(f, t)| f.contains("dyno-platform") && t.contains("not(target_os = \"macos\")")),
        "check broken: the scan cannot see dyno-platform's own `keyring` entry: {found:?}"
    );
    for (file, table) in &found {
        let other_os = table.contains("not(target_os = \"macos\")") || table.contains("windows") || table.contains("target_os = \"linux\"");
        assert!(
            other_os,
            "{file} turns on paddock-secrets' `keyring` in `{table}`, which reaches macOS: a Keychain read would wait behind a password \
             dialog after every update. Move it under a `cfg(not(target_os = \"macos\"))` table"
        );
    }
}

/// `Some(true)` when `cargo tree -i keyring` finds the crate for `target`, `None` when it cannot run without the network.
fn tree_has_keyring(target: &str) -> Option<bool> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = dyno_platform::process::command(&cargo);
    cmd.args([
        "tree",
        "--offline",
        "--locked",
        "--invert",
        "keyring",
        "--prefix",
        "none",
        "--target",
        target,
        "--manifest-path",
    ])
    .arg(root().join("apps/dyno/Cargo.toml"));
    let out = dyno_platform::process::run_with_timeout(cmd, Duration::from_secs(120)).ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stdout.lines().any(|l| l.starts_with("keyring v")) {
        return Some(true);
    }
    (out.status.success() || stderr.contains("did not match any packages") || stderr.contains("nothing to print")).then_some(false)
}

#[test]
fn when_everything_is_fetched_cargo_tree_finds_no_keychain_adapter_for_macos() {
    // Positive control: the Windows graph has it. If the probe cannot see that, it has not measured anything.
    let Some(windows) = tree_has_keyring("x86_64-pc-windows-msvc") else {
        eprintln!("not measured: cargo tree cannot run offline here; the manifest scan stands alone");
        return;
    };
    assert!(windows, "check broken: `cargo tree` cannot see `keyring` in the Windows graph");
    for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
        assert_eq!(
            tree_has_keyring(target),
            Some(false),
            "`keyring` is built into Dyno for {target}: find who enables it with `cargo tree -i keyring --target {target}`"
        );
    }
}
