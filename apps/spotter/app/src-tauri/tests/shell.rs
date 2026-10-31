//! The shell plugin with Spotter's parts, on Tauri's mock runtime: nothing is drawn, nothing outside the temp folder
//! is touched (rule 7).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_commands::testing::{sync_generated, WriteMode};
use spotter_app_lib::dialogs::DIALOGS;
use spotter_app_lib::health::Hub;
use spotter_app_lib::{shell, Place, COMMANDS, MANIFEST};

fn builder() -> paddock_shell::Builder<tauri::test::MockRuntime> {
    let dir = tempfile::tempdir().unwrap();
    shell(
        Place::Test {
            dir: dir.path().to_path_buf(),
        },
        &Hub::default(),
    )
}

/// The contract file, the manifest, Spotter's table and the shell's table agree (a dialog contract naming a missing or
/// closed command, a command both tables use, a service or settings section the UI does not know).
#[test]
fn the_dialog_contracts_the_manifest_and_the_tables_agree() {
    let problems = builder().problems(Some(MANIFEST));
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Spotter's services are the ones the UI registers, in the status bar's order.
#[test]
fn the_services_are_the_ones_the_ui_lists() {
    assert_eq!(builder().service_ids(), vec!["sp.scheduler", "sp.sources", "sp.renderer"]);
}

/// Spotter's table never shadows a shell command.
#[test]
fn no_command_is_in_both_tables() {
    for spec in COMMANDS.specs() {
        assert!(paddock_shell::COMMANDS.spec(spec.name()).is_none(), "{}", spec.name());
    }
}

const REGENERATE: &str = "cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell";

/// The UI's mock of the jobs takes the kinds, and which of them are quiet on success, from Rust's list (one source).
#[test]
fn the_uis_job_kinds_are_generated_from_the_kinds() {
    use spotter_app_lib::jobs::Kind;
    let rows: Vec<String> = Kind::ALL
        .iter()
        .map(|k| format!("  {{ slug: '{}', quietOnSuccess: {} }},", k.slug(), k.quiet_on_success()))
        .collect();
    let text = format!(
        "// Generated from `jobs::Kind` by the app's tests. Do not edit: {REGENERATE}.\n\n/** The kinds of job Spotter registers, and whether their success is left out of the notifications. */\nexport const JOB_KINDS = [\n{}\n] as const;\n\n/** The slugs of the kinds whose success is quiet. */\nexport const QUIET_ON_SUCCESS_KINDS: ReadonlySet<string> = new Set(JOB_KINDS.filter((k) => k.quietOnSuccess).map((k) => k.slug));\n",
        rows.join("\n")
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/job-kinds.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// The UI's command names are generated from the table, so a typo in a dialog contract is a compile error.
#[test]
fn the_uis_command_names_are_generated_from_the_table() {
    let text = paddock_commands::ts::names_module(&COMMANDS, "APP_COMMANDS", "AppCommand", REGENERATE).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/app-commands.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// The UI's dialog contracts are generated from the dialogs table, so Rust and TypeScript cannot disagree (ADR 0026 §3).
#[test]
fn the_uis_dialog_contracts_are_generated_from_the_table() {
    let text = paddock_dialogs::ts::contracts_module("the app", &DIALOGS, REGENERATE);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/app/generated/dialog-contracts.ts");
    sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}

/// Every command runs on the async runtime and returns at once (rule 10).
#[test]
fn every_command_is_async() {
    paddock_commands::testing::assert_async(&COMMANDS, &[&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")]);
}

// --- Capabilities per window class, resolved through Tauri's own ACL ---

use paddock_shell::PLUGIN_NAME;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri_utils::acl::capability::Capability;
use tauri_utils::acl::manifest::{Manifest, PermissionFile};
use tauri_utils::acl::resolved::Resolved;

fn capability_files() -> Vec<Capability> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .collect()
}

/// The plugin's permission files, as its build step lists them (`DEP_PADDOCK_SHELL_PERMISSION_FILES_PATH`, see build.rs).
fn plugin_permissions() -> Vec<PermissionFile> {
    let list = std::fs::read_to_string(env!("PADDOCK_SHELL_PERMISSION_FILES")).unwrap();
    let paths: Vec<PathBuf> = serde_json::from_str(&list).unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("window-classes.toml")),
        "the plugin lists its window-class sets: {paths:?}"
    );
    paths
        .iter()
        .map(|p| toml::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{e}")))
        .collect()
}

/// Every capability resolves against the plugin's permissions (a permission the plugin does not have fails here). Core
/// permissions are left out: the mock runtime has no core plugins to resolve them against.
#[test]
fn the_capabilities_resolve_against_the_shell_plugins_permissions() {
    let acl = BTreeMap::from([(PLUGIN_NAME.to_string(), Manifest::new(plugin_permissions(), None))]);
    let caps: BTreeMap<String, Capability> = capability_files()
        .into_iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c).unwrap();
            let perms: Vec<Value> = v["permissions"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| !p.as_str().is_some_and(|s| s.starts_with("core:")))
                .cloned()
                .collect();
            v["permissions"] = Value::Array(perms);
            (c.identifier.clone(), serde_json::from_value(v).unwrap())
        })
        .collect();
    Resolved::resolve(&acl, caps, tauri_utils::platform::Target::current()).unwrap();
}

/// Each window class gets exactly the shell's set for it, and nothing of its own.
#[test]
fn each_window_class_gets_exactly_the_shells_set_for_it() {
    let classes: BTreeMap<String, (Vec<String>, Vec<String>)> = capability_files()
        .into_iter()
        .map(|c| {
            let v = serde_json::to_value(&c).unwrap();
            let list = |k: &str| {
                v[k].as_array()
                    .unwrap()
                    .iter()
                    .map(|w| w.as_str().unwrap().to_string())
                    .collect::<Vec<_>>()
            };
            (c.identifier.clone(), (list("windows"), list("permissions")))
        })
        .collect();
    let owned = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        classes,
        BTreeMap::from([
            (
                "shell-dialog".to_string(),
                (owned(&["d-*"]), owned(&["core:event:default", "paddock-shell:dialog"]))
            ),
            (
                "shell-main".to_string(),
                (
                    owned(&["main", "w-*"]),
                    // Dragging by the overlay title bar (macOS) needs these two window permissions.
                    owned(&[
                        "core:event:default",
                        "core:window:allow-start-dragging",
                        "core:window:allow-internal-toggle-maximize",
                        "paddock-shell:main"
                    ])
                )
            ),
            (
                "shell-tray".to_string(),
                (owned(&["tray", "tray-*"]), owned(&["core:event:default", "paddock-shell:tray"]))
            ),
        ])
    );
}

/// The update feed is the moving release that spotter-release.yml publishes latest.json on (installed copies read it).
#[test]
fn the_update_feed_is_the_release_the_workflow_publishes_latest_json_on() {
    let workflow =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../.gitea/workflows/spotter-release.yml"))
            .unwrap();
    let tag = workflow
        .lines()
        .find_map(|l| l.trim().strip_prefix("latest-tag:"))
        .map(|t| t.trim().to_string())
        .expect("the workflow names the moving release");
    assert_eq!(
        spotter_app_lib::UPDATE_FEED,
        format!("https://dev.decryptology.net/decryp7/paddock/releases/download/{tag}/latest.json")
    );
}

/// An unusable system proxy (a `socks5://` address) must not stop Spotter from starting: the downloader goes direct and
/// says why. A lookup that hangs is bounded the same way.
#[test]
fn an_unusable_system_proxy_means_a_direct_downloader_and_a_reason() {
    use paddock_download::ProxyRules;
    let socks = |name: &str| (name == "ALL_PROXY").then(|| "socks5://proxy.example:1080".to_string());
    let why = spotter_app_lib::net_with(move || ProxyRules::from_env_with(&socks), std::time::Duration::from_secs(5)).problem;
    let why = why.expect("the proxy was refused, so the downloader says why");
    assert!(why.to_lowercase().contains("socks"), "{why}");

    let why = spotter_app_lib::net_with(
        || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            Ok(ProxyRules::none())
        },
        std::time::Duration::from_millis(50),
    )
    .problem;
    assert!(why.is_some_and(|w| w.contains("too long")));

    let why = spotter_app_lib::net_with(|| Ok(ProxyRules::none()), std::time::Duration::from_secs(5)).problem;
    assert_eq!(why, None);
}

/// Services' degraded cause for the proxy names the supported forms.
#[test]
fn the_proxy_degradation_tells_the_user_what_to_fix() {
    let cause = spotter_app_lib::health::proxy_cause("socks");
    assert!(format!("{cause:?}").contains("http://host:port"));
}

/// Spotter's start as `run` composes it (rule 43), the engine's set-up included, on the mock runtime with a headless
/// tray: the tray and the main window open, after every plugin is set up (a window built inside a plugin hook
/// deadlocked Spotter's start).
#[test]
fn the_start_opens_the_tray_and_the_main_window() {
    use paddock_shell::testing::{start, Started};
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let started = start(move || composed(&data, paddock_window::launch::LaunchArgs::default()));
    assert_eq!(
        started,
        Ok(Started {
            main_window: true,
            tray: true
        })
    );
    assert!(dir.path().join("data").is_dir(), "the engine opened, in the test's folder");
}

/// Started at log-in (`--hidden`) with a tray: Spotter stays in the tray without a window.
#[test]
fn a_hidden_start_stays_in_the_tray() {
    use paddock_shell::testing::{start, Started};
    let launch = paddock_window::launch::LaunchArgs {
        hidden: true,
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    assert_eq!(
        start(move || composed(&data, launch)),
        Ok(Started {
            main_window: false,
            tray: true
        })
    );
}

/// Spotter as `run` composes it, the engine included (its data in `dir`, no OS toasts), without the OS plugins.
fn composed(dir: &std::path::Path, launch: paddock_window::launch::LaunchArgs) -> tauri::Builder<tauri::test::MockRuntime> {
    let shell = shell(Place::Test { dir: dir.to_path_buf() }, &Hub::default());
    spotter_app_lib::app(tauri::test::mock_builder(), shell, launch, paddock_window::tauri::HeadlessTray)
}
