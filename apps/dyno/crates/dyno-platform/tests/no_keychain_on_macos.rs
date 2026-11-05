#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The macOS build of Dyno has no Keychain adapter at all (ADR 0032 amendments, 2026-11-05): a Keychain read waits behind a password
//! dialog with no time limit after every update of an ad-hoc signed build, so no code path may be able to make one.
//!
//! Two probes, neither of which uses the network (a cold runner would fetch hundreds of MB, and a blip must not fail CI):
//! 1. A scan of the manifests of the crates Dyno builds (the path packages of `Cargo.lock`): every `paddock-secrets` dependency that
//!    turns on `keyring` must sit in a table for another OS. Any line naming `keyring` counts, and only the exact cfgs `not(target_os = "macos")`, `windows`, `target_os = "windows"` and
//!    `target_os = "linux"` are accepted. Its positive control (ER-33) is that it finds `dyno-platform`'s own entry.
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

/// `(manifest, table header)` of every manifest line in the Dyno graph that names `keyring`: a dependency (also renamed or inherited
/// from a virtual workspace manifest), a feature entry such as `default = ["paddock-secrets/keyring"]` or `paddock-secrets?/keyring`,
/// or the crate itself. `paddock-secrets`' own manifest defines the feature and is the one that is left out.
fn keyring_mentions() -> Vec<(String, String)> {
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
        let virtual_workspace = !text.contains("[package]") && text.contains("[workspace");
        if own.as_deref() == Some("paddock-secrets") || (!virtual_workspace && !own.is_some_and(|n| local.contains(&n))) {
            continue;
        }
        found.extend(tables_naming_keyring(&text).into_iter().map(|t| (file.display().to_string(), t)));
    }
    found
}

/// The table header of every line of a manifest that names `keyring` (the `[package]` table's prose does not count).
fn tables_naming_keyring(text: &str) -> Vec<String> {
    let mut table = String::new();
    let mut found = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        if line.starts_with('[') {
            table = line.to_string();
        } else if table != "[package]" && line.contains("keyring") {
            found.push(table.clone());
        }
    }
    found
}

/// The cfg of a `[target.'cfg(..)'.dependencies]` header without its `cfg(` wrapper, or `None` for any other table.
fn target_cfg(table: &str) -> Option<&str> {
    let rest = table.strip_prefix("[target.")?;
    let quote = rest.chars().next().filter(|q| *q == '\'' || *q == '"')?;
    let end = rest[1..].find(quote)? + 1;
    rest[1..end].strip_prefix("cfg(")?.strip_suffix(')')
}

/// Whether a table reaches no macOS build: exactly one of the cfgs below, nothing cleverer (`not(windows)` and `any(windows, macos)`
/// both reach a Mac).
fn other_os_only(table: &str) -> bool {
    const OTHER: [&str; 4] = [
        "not(target_os = \"macos\")",
        "windows",
        "target_os = \"windows\"",
        "target_os = \"linux\"",
    ];
    target_cfg(table).is_some_and(|c| OTHER.contains(&c))
}

#[test]
fn only_tables_for_other_systems_turn_the_keychain_adapter_on() {
    let found = keyring_mentions();
    assert!(
        found
            .iter()
            .any(|(f, t)| f.contains("dyno-platform") && t.contains("not(target_os = \"macos\")")),
        "check broken: the scan cannot see dyno-platform's own `keyring` entry: {found:?}"
    );
    for (file, table) in &found {
        assert!(
            other_os_only(table),
            "{file} names `keyring` in `{table}`, which reaches macOS: a Keychain read would wait behind a password \
             dialog after every update. Move it under a `cfg(not(target_os = \"macos\"))` table"
        );
    }
}

/// `Some(true)` when `cargo tree -i <krate>` finds the crate for `target`, `None` when it cannot run without the network.
fn tree_has(krate: &str, target: &str) -> Option<bool> {
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
    if stdout.lines().any(|l| l.starts_with(&format!("{krate} v"))) {
        return Some(true);
    }
    (out.status.success() || stderr.contains("did not match any packages") || stderr.contains("nothing to print")).then_some(false)
}

#[test]
fn cargo_tree_finds_no_keychain_adapter_for_macos() {
    let macos = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
    if cfg!(target_os = "macos") {
        // On a Mac the macOS graph is fetched, so the probe must run. Its positive control is a crate every Dyno build has.
        for target in macos {
            assert_eq!(
                tree_has("paddock-secrets", target),
                Some(true),
                "check broken: `cargo tree` cannot run or cannot see paddock-secrets for {target}"
            );
            assert_eq!(
                tree_has("keyring", target),
                Some(false),
                "`keyring` is built into Dyno for {target}: `cargo tree -i keyring --target {target}` shows who enables it"
            );
        }
        return;
    }
    // Elsewhere the macOS packages may not be fetched. Positive control: the Windows graph has `keyring`.
    let Some(windows) = tree_has("keyring", "x86_64-pc-windows-msvc") else {
        eprintln!("not measured: cargo tree cannot run offline here; the manifest scan stands alone");
        return;
    };
    assert!(windows, "check broken: `cargo tree` cannot see `keyring` in the Windows graph");
    for target in macos {
        assert_eq!(
            tree_has("keyring", target),
            Some(false),
            "`keyring` is built into Dyno for {target}: find who enables it with `cargo tree -i keyring --target {target}`"
        );
    }
}

#[test]
fn the_scan_judges_each_tricky_manifest_shape() {
    let ok = |text: &str| tables_naming_keyring(text).iter().all(|t| other_os_only(t));
    let dep = "paddock-secrets = { path = \"x\", features = [\"keyring\"] }";
    for cfg in [
        "not(target_os = \"macos\")",
        "windows",
        "target_os = \"windows\"",
        "target_os = \"linux\"",
    ] {
        assert!(ok(&format!("[target.'cfg({cfg})'.dependencies]\n{dep}\n")), "{cfg} reaches no Mac");
    }
    for cfg in [
        "not(windows)",
        "any(windows, target_os = \"macos\")",
        "unix",
        "all(unix, not(target_os = \"macos\"))",
        "target_os = \"macos\"",
        "not(not(target_os = \"macos\"))",
    ] {
        assert!(
            !ok(&format!("[target.'cfg({cfg})'.dependencies]\n{dep}\n")),
            "{cfg} must not be accepted"
        );
    }
    for text in [
        format!("[dependencies]\n{dep}\n"),
        "[features]\ndefault = [\"paddock-secrets/keyring\"]\n".into(),
        "[features]\nsecure = [\"paddock-secrets?/keyring\"]\n".into(),
        "[dependencies]\nsecrets = { package = \"paddock-secrets\", path = \"x\", features = [\"keyring\"] }\n".into(),
        "[dependencies]\nkeyring = \"4\"\n".into(),
        "[dependencies.paddock-secrets]\npath = \"x\"\nfeatures = [\"keyring\"]\n".into(),
        "[workspace.dependencies]\npaddock-secrets = { path = \"x\", features = [\"keyring\"] }\n".into(),
    ] {
        assert!(!ok(&text), "must be flagged: {text}");
    }
    assert!(
        ok("[package]\ndescription = \"the keyring adapter\"\n[dependencies]\npaddock-secrets = { path = \"x\" }\n"),
        "prose and plain dependencies are fine"
    );
}
