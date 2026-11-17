#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The macOS build of Dyno has no Keychain adapter at all (ADR 0032 amendments, 2026-11-05): a Keychain read waits behind a password
//! dialog with no time limit after every update of an ad-hoc signed build, so no code path may be able to make one.
//!
//! Two probes, neither of which uses the network (a cold runner would fetch hundreds of MB, and a blip must not fail CI):
//! 1. A scan of the manifests of the crates Dyno builds (the path packages of `Cargo.lock`): every `paddock-secrets` dependency that
//!    turns on `keyring` must sit in a table for another OS. Any line naming `keyring` counts, and only the exact cfgs `not(target_os = "macos")`, `windows`, `target_os = "windows"` and
//!    `target_os = "linux"` are accepted. Its positive control (ER-33) is that it finds `dyno-platform`'s own entry.
//! 2. `cargo tree --offline --all-features -i keyring` per target, fetching the target's packages with `cargo fetch --locked` first when
//!    they are not there. It is authoritative, with the Windows graph (or `paddock-secrets` on a Mac) as its positive control. When it
//!    cannot run at all the test fails with `check broken`: a check that did not measure never passes (ER-33).

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
        let words: &[&str] = if file.starts_with(root().join("apps/dyno")) {
            &DYNO_WORDS
        } else {
            &SHARED_WORDS
        };
        found.extend(tables_naming(&text, words).into_iter().map(|t| (file.display().to_string(), t)));
    }
    found
}

/// What every manifest in the Dyno graph may not name outside a table for another OS, and what Dyno's own manifests may not name at all
/// (the other ways to reach the Keychain from Rust).
const SHARED_WORDS: [&str; 1] = ["keyring"];
const DYNO_WORDS: [&str; 4] = ["keyring", "security-framework", "apple-native-keyring-store", "keyring-core"];

/// The table header of every header or line of a manifest that names one of `words` (the `[package]` table's prose does not count).
fn tables_naming(text: &str, words: &[&str]) -> Vec<String> {
    let mut table = String::new();
    let mut found = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
        let names = words.iter().any(|w| line.contains(w));
        if line.starts_with('[') {
            table = line.to_string();
            if names {
                found.push(table.clone());
            }
        } else if table != "[package]" && names {
            found.push(table.clone());
        }
    }
    found
}

/// Whether Rust source runs the `security` tool to read the Keychain.
fn source_reads_keychain(text: &str) -> bool {
    text.contains("find-generic-password") || text.contains("find-internet-password")
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

/// Whether `cargo tree -i <krate>` finds the crate in what `apps/dyno` builds for `target`, with every feature on so that an optional
/// one cannot hide it. The probe never fetches (no network inside a 60 s test): packages that are not fetched are `Err`, "check broken"
/// (ER-33), and the gate's prepare step (`paddock.ci.json`) runs `cargo fetch --locked` for each target first. Each phase prints its
/// duration to stderr (ER-27). `Err` means the probe could not run.
fn tree_has(krate: &str, target: &str) -> Result<bool, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = root().join("apps/dyno/Cargo.toml");
    let started = std::time::Instant::now();
    let mut cmd = paddock_testkit::TestCommand::new(&cargo);
    cmd.args([
        "tree",
        "--offline",
        "--locked",
        "--all-features",
        "--invert",
        krate,
        "--prefix",
        "none",
        "--target",
        target,
        "--manifest-path",
    ])
    .arg(&manifest);
    let (cmd, _private) = cmd.into_parts();
    let out =
        dyno_platform::process::run_with_timeout(cmd, Duration::from_secs(120)).map_err(|e| format!("cargo tree could not run: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let found = stdout.lines().any(|l| l.starts_with(&format!("{krate} v")));
    let clean = out.status.success() || stderr.contains("did not match any packages") || stderr.contains("nothing to print");
    eprintln!(
        "cargo tree --invert {krate} --target {target}: exit {:?}, found {found}, {:?}",
        out.status.code(),
        started.elapsed()
    );
    if found {
        return Ok(true);
    }
    if clean {
        return Ok(false);
    }
    Err(format!(
        "packages not fetched (or cargo tree failed) for {target}; run `cargo fetch --locked --target {target} --manifest-path apps/dyno/Cargo.toml` first: {}",
        stderr.trim()
    ))
}

#[test]
fn cargo_tree_finds_no_keychain_adapter_for_macos() {
    let macos = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
    let ask = |krate: &str, target: &str| tree_has(krate, target).unwrap_or_else(|e| panic!("check broken: cargo tree could not run: {e}"));
    // Positive controls (ER-33): on a Mac a crate every Dyno build has; elsewhere the Windows graph, which has `keyring`.
    if cfg!(target_os = "macos") {
        for target in macos {
            assert!(
                ask("paddock-secrets", target),
                "check broken: `cargo tree` cannot see paddock-secrets for {target}"
            );
        }
    } else {
        assert!(
            ask("keyring", "x86_64-pc-windows-msvc"),
            "check broken: `cargo tree` cannot see `keyring` in the Windows graph"
        );
    }
    for target in macos {
        assert!(
            !ask("keyring", target),
            "`keyring` is built into Dyno for {target}: find who enables it with `cargo tree --all-features -i keyring --target {target}`"
        );
    }
}

/// Product source reads no Keychain item with the `security` tool either.
#[test]
fn no_dyno_source_reads_the_keychain_through_the_security_tool() {
    let mut files = Vec::new();
    for dir in ["apps/dyno/crates", "apps/dyno/app/src-tauri"] {
        rust_files(&root().join(dir), &mut files);
    }
    let me = Path::new(file!()).file_name().unwrap().to_owned();
    let offenders: Vec<_> = files
        .iter()
        .filter(|f| f.file_name() != Some(&me))
        .filter(|f| source_reads_keychain(&std::fs::read_to_string(f).unwrap_or_default()))
        .collect();
    assert!(
        offenders.is_empty(),
        "these run `security find-*-password`, which waits behind a dialog after an update: {offenders:?}"
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        let name = entry.file_name();
        if p.is_dir() && name != "target" && name != "node_modules" {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn the_scan_judges_each_tricky_manifest_shape() {
    let ok = |text: &str| tables_naming(text, &SHARED_WORDS).iter().all(|t| other_os_only(t));
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

    for text in [
        "[dependencies.keyring]\nversion = \"4.2\"\n",
        "[target.'cfg(unix)'.dependencies.keyring]\nversion = \"4.2\"\n",
        "[dependencies.paddock-secrets]\npath = \"x\"\n[dependencies.keyring-core]\nversion = \"1\"\n",
    ] {
        assert!(!ok(text), "a table header that names keyring must be flagged: {text}");
    }
    assert!(
        ok("[target.'cfg(not(target_os = \"macos\"))'.dependencies.keyring]\nversion = \"4.2\"\n"),
        "the exact other-OS table is fine"
    );
    // Dyno's own manifests also name no other way to the Keychain.
    for text in [
        "[dependencies]\nsecurity-framework = \"3\"\n",
        "[target.'cfg(target_os = \"macos\")'.dependencies]\napple-native-keyring-store = \"0.2\"\n",
        "[dependencies]\nkeyring-core = \"1\"\n",
    ] {
        assert!(
            !tables_naming(text, &DYNO_WORDS).iter().all(|t| other_os_only(t)),
            "must be flagged in Dyno's manifests: {text}"
        );
    }
    assert!(
        tables_naming("[dependencies]\nsecurity-framework = \"3\"\n", &SHARED_WORDS).is_empty(),
        "shared crates may use it; only Dyno's own manifests may not"
    );
    assert!(source_reads_keychain(
        "Command::new(\"security\").args([\"find-generic-password\", \"-s\", x])"
    ));
    assert!(!source_reads_keychain("let x = 1;"));
}
