//! The plugin's generated files (checked on every run; `PADDOCK_WRITE_TS=1` rewrites them): the command list of the
//! build step, the permission sets per window class, and the TypeScript of the command names and wire types.

use crate::{COMMANDS, permissions};
use paddock_commands::testing::{WriteMode, sync_generated};
use std::path::{Path, PathBuf};

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib generated";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The `shell` entry point of `@decryp7/paddock-ui`, where the plugin's command names are used.
fn ui_dir() -> PathBuf {
    crate_dir().join("../../ui/projects/paddock-ui/shell/generated")
}

fn shell_ts() -> String {
    use ts_rs::TS;
    let decls = [crate::AppIdentity::decl(), crate::VersionReply::decl()];
    let mut out = format!(
        "// Generated from the Rust wire types of paddock-shell (ts-rs). Do not edit: {REGENERATE}.\n\n\
         /** The plugin's name: its commands are `plugin:{name}|<command>`. */\n\
         export const SHELL_PLUGIN = '{name}';\n\n\
         /** The shell's version: the plugin's `shell_version` must answer it (one version across the boundary). */\n\
         export const SHELL_VERSION = '{version}';\n\n",
        name = crate::PLUGIN_NAME,
        version = crate::VERSION
    );
    for d in decls {
        out.push_str(&format!("export {d}\n\n"));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

fn generated() -> Vec<(PathBuf, String)> {
    vec![
        (
            crate_dir().join("src/command_names.rs"),
            permissions::command_names_rs(&COMMANDS, REGENERATE),
        ),
        (
            crate_dir().join("permissions").join(permissions::FILE),
            permissions::window_classes_toml(&COMMANDS, REGENERATE),
        ),
        (
            ui_dir().join("shell-commands.ts"),
            paddock_commands::ts::names_module(
                &COMMANDS,
                "SHELL_COMMANDS",
                "ShellCommand",
                REGENERATE,
            )
            .unwrap(),
        ),
        (ui_dir().join("shell.ts"), shell_ts()),
    ]
}

#[test]
fn generated_files_are_current() {
    let mode = WriteMode::from_env();
    let stale: Vec<String> = generated()
        .into_iter()
        .filter_map(|(path, text)| {
            sync_generated(&path, &text, mode)
                .err()
                .map(|e| e.to_string())
        })
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
}

#[test]
fn the_build_step_lists_every_command_of_the_table() {
    let mut names: Vec<&str> = COMMANDS.specs().iter().map(|s| s.name()).collect();
    names.sort_unstable();
    assert_eq!(crate::command_names::COMMAND_NAMES, names.as_slice());
}

#[test]
fn every_command_is_async() {
    let roots: Vec<PathBuf> = [
        "paddock-shell",
        "paddock-startup",
        "paddock-health",
        "paddock-dialogs",
        "paddock-diag",
        "paddock-files",
        "paddock-storage",
        "paddock-service",
        "paddock-packs",
        "paddock-channels",
        "paddock-mcp",
        "paddock-jobs",
        "paddock-settings",
        "paddock-updater",
        "paddock-window",
        "paddock-menu",
    ]
    .iter()
    .map(|c| crate_dir().join("..").join(c).join("src"))
    .collect();
    let refs: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    paddock_commands::testing::assert_async(&COMMANDS, &refs);
}

/// The shell's own dialog contracts (`pd.confirm`) as TypeScript, in the `shell/confirm` entry point (ADR 0026 §3, §5).
#[test]
fn generated_dialog_contracts_are_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/shell/confirm/generated/dialog-contracts.ts");
    let text = paddock_dialogs::ts::contracts_module("paddock-shell", &crate::DIALOGS, REGENERATE);
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// ADR 0019: the shared limit registries are known to `LimitId::parse` once a shell is built, so their ids resolve to the
/// registered rows (the same text, not a copy that a flood of unknown ids could refuse).
#[test]
fn building_a_shell_registers_the_shared_limit_registries() {
    use paddock_limits::{LimitId, Registry};
    let _ = crate::Builder::<tauri::test::MockRuntime>::new(crate::AppIdentity::new(
        "net.example.limits",
        "Limits",
        "0.0.0",
    ));
    for id in [
        "download.size",
        "storage.walk_entries",
        "updater.download_bytes",
        "diag.files",
        "net.redirects",
        "bounded.cuts",
    ] {
        assert!(LimitId::is_registered(id), "{id} was not registered");
    }
    // An app's own registry joins through the builder.
    static APP: [paddock_limits::LimitRow; 1] = [paddock_limits::LimitRow::stop(
        "app.limit_registered_by_the_builder",
        paddock_limits::Quantity::Count(1),
        paddock_limits::Quantity::Count(1),
        paddock_limits::Quantity::Count(1),
        paddock_limits::RowValue::Hard,
        paddock_limits::Remedy::None,
    )];
    let _ = crate::Builder::<tauri::test::MockRuntime>::new(crate::AppIdentity::new(
        "net.example.limits",
        "Limits",
        "0.0.0",
    ))
    .limits(Registry::new(&APP));
    assert!(LimitId::is_registered(
        "app.limit_registered_by_the_builder"
    ));
}

/// Every crate the shell links that has a limit registry is registered by `register_shared_limits` (a registry left out would
/// make its ids unknown to `LimitId::parse`, and a flood of them could refuse the known ones). The linked crates come from
/// `cargo metadata` (the normal dependencies of this crate, transitively); a registry is a `pub const REGISTRY` in the crate's
/// `limits.rs` or `limit_rows.rs`.
#[test]
fn every_limit_registry_the_shell_links_is_registered() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::process::Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
        .arg(manifest.join("Cargo.toml"))
        .output()
        .expect("cargo metadata runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let nodes = meta["resolve"]["nodes"].as_array().unwrap();
    let find = |id: &str| nodes.iter().find(|n| n["id"] == id).unwrap();
    let root = meta["resolve"]["root"].as_str().expect("a package, not a virtual workspace");
    let mut seen = std::collections::BTreeSet::new();
    let mut todo = vec![root.to_owned()];
    while let Some(id) = todo.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for dep in find(&id)["deps"].as_array().unwrap() {
            let normal = dep["dep_kinds"].as_array().unwrap().iter().any(|k| k["kind"].is_null());
            if normal {
                todo.push(dep["pkg"].as_str().unwrap().to_owned());
            }
        }
    }
    let builder = include_str!("builder.rs");
    let mut missing = Vec::new();
    let mut found = 0;
    for pkg in meta["packages"].as_array().unwrap() {
        if !seen.contains(pkg["id"].as_str().unwrap()) {
            continue;
        }
        let dir = std::path::Path::new(pkg["manifest_path"].as_str().unwrap()).parent().unwrap().to_owned();
        let name = pkg["name"].as_str().unwrap().replace('-', "_");
        for module in ["limits", "limit_rows"] {
            let file = dir.join("src").join(format!("{module}.rs"));
            if std::fs::read_to_string(&file).is_ok_and(|t| t.contains("pub const REGISTRY")) {
                found += 1;
                if !builder.contains(&format!("{name}::{module}::REGISTRY")) {
                    missing.push(format!("{name}::{module}::REGISTRY"));
                }
            }
        }
    }
    assert!(found >= 4, "the scan found only {found} registries: it cannot see the graph");
    assert!(missing.is_empty(), "register_shared_limits misses: {missing:?}");
}
