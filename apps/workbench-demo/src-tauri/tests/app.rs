//! The app on Tauri's mock runtime: no window reaches the desktop and nothing is written outside a temp folder (rule 7).
//! The two plugins with the demo's parts, the demo's guarded commands, the capabilities per window class resolved through
//! Tauri's own ACL, the drag ghost's page, and the sample job, Pitwall and assistant working without Pitwall.

use paddock_panels::ghost::{GHOST_LABEL, page};
use paddock_workbench_demo::{COMMANDS, Place, dialogs, invoke_handler, link, shell, workbench};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_utils::acl::capability::Capability;
use tauri_utils::acl::manifest::{Manifest, PermissionFile};
use tauri_utils::acl::resolved::Resolved;

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ui_dir() -> PathBuf {
    app_dir().join("../../../ui/projects/workbench-demo")
}

/// The app's capability files, as Tauri reads them.
fn capability_files() -> Vec<Capability> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(app_dir().join("capabilities"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| {
            serde_json::from_str(&std::fs::read_to_string(p).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        })
        .collect()
}

/// A plugin's permission files (Tauri's per-command ones and the generated sets), as its build step lists them.
fn plugin_permissions(list_file: &str) -> Vec<PermissionFile> {
    let list = std::fs::read_to_string(list_file).unwrap();
    let paths: Vec<PathBuf> = serde_json::from_str(&list).unwrap();
    assert!(
        paths.iter().any(|p| p.ends_with("window-classes.toml")),
        "the plugin lists its window-class sets: {paths:?}"
    );
    paths
        .iter()
        .map(|p| {
            toml::from_str(&std::fs::read_to_string(p).unwrap()).unwrap_or_else(|e| panic!("{e}"))
        })
        .collect()
}

/// The mock context with the app's own capabilities resolved through Tauri's ACL (core permissions left out: the mock
/// runtime has no core plugins to resolve them against).
fn context() -> tauri::Context<MockRuntime> {
    let mut ctx = mock_context(noop_assets());
    let acl = BTreeMap::from([
        (
            "paddock-shell".to_string(),
            Manifest::new(
                plugin_permissions(env!("PADDOCK_SHELL_PERMISSION_FILES")),
                None,
            ),
        ),
        (
            "paddock-workbench".to_string(),
            Manifest::new(
                plugin_permissions(env!("PADDOCK_WORKBENCH_PERMISSION_FILES")),
                None,
            ),
        ),
    ]);
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
    let resolved = Resolved::resolve(&acl, caps, tauri_utils::platform::Target::current()).unwrap();
    *ctx.runtime_authority_mut() = tauri::runtime_authority!(acl, resolved);
    ctx
}

fn app(dir: &Path) -> App<MockRuntime> {
    let place = Place::Test {
        dir: dir.to_path_buf(),
    };
    let link = link(&place).unwrap();
    mock_builder()
        .plugin(shell::<MockRuntime>(&place, link.clone()).build())
        .plugin(workbench::<MockRuntime>(&place, link).unwrap().build())
        .invoke_handler(invoke_handler())
        .build(context())
        .unwrap()
}

fn window(app: &App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .build()
        .unwrap()
}

fn invoke(w: &WebviewWindow<MockRuntime>, cmd: &str, args: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .unwrap(),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(w, request).map(|b| b.deserialize::<Value>().unwrap())
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[test]
fn the_drag_ghosts_page_is_served_from_the_uis_public_folder_and_is_the_crates() {
    for (name, content) in [
        (page::HTML_NAME, page::HTML),
        (page::CSS_NAME, page::CSS),
        (page::JS_NAME, page::JS),
    ] {
        let served = std::fs::read_to_string(ui_dir().join("public").join(name)).unwrap();
        assert_eq!(
            served, content,
            "{name}: copy it from crates/paddock-panels/assets"
        );
    }
}

#[test]
fn no_capability_matches_the_drag_ghosts_window() {
    for c in capability_files() {
        for pattern in &c.windows {
            let matches = match pattern.strip_suffix('*') {
                Some(prefix) => GHOST_LABEL.starts_with(prefix),
                None => GHOST_LABEL == pattern,
            };
            assert!(!matches, "{} matches {GHOST_LABEL}", c.identifier);
        }
    }
}

#[test]
fn the_capabilities_grant_each_window_class_its_plugin_sets() {
    let caps = capability_files();
    let grants = |window: &str| -> Vec<String> {
        let mut out: Vec<String> = caps
            .iter()
            .filter(|c| c.windows.iter().any(|w| w == window))
            .flat_map(|c| {
                serde_json::to_value(&c.permissions)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(text)
                    .collect::<Vec<_>>()
            })
            .filter(|p| p.starts_with("paddock-"))
            .collect();
        out.sort();
        out
    };
    assert_eq!(
        grants("main"),
        ["paddock-shell:main", "paddock-workbench:main"]
    );
    assert_eq!(
        grants("p-*"),
        ["paddock-shell:popout", "paddock-workbench:popout"]
    );
    assert_eq!(
        grants("d-*"),
        ["paddock-shell:dialog", "paddock-workbench:dialog"]
    );
}

#[test]
fn without_pitwall_the_component_gets_a_calm_snapshot_and_the_builtin_repair_is_not_offered() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let snapshot = invoke(&main, "pitwall_state", json!({})).unwrap();
    assert_eq!(snapshot["app"], "Paddock Workbench Demo", "{snapshot}");
    assert_eq!(snapshot["builtInAvailable"], false);
    assert!(!text(&snapshot["message"]).is_empty());
    assert!(snapshot["state"]["kind"].is_string(), "{snapshot}");
    let refused = invoke(&main, "pitwall_use_builtin", json!({})).unwrap_err();
    assert_eq!(refused["code"], "unavailable", "{refused}");
    assert_eq!(
        invoke(&main, "pitwall_cancel_pairing", json!({})).unwrap(),
        json!(false)
    );
    // A dialog never asks about Pitwall; a pop-out (the assistant can be popped out) does.
    let popout = window(&app, "p-main-1");
    assert!(invoke(&popout, "pitwall_state", json!({})).is_ok());
    let dialog = window(&app, "d-main-1");
    assert_eq!(
        invoke(&dialog, "pitwall_state", json!({})).unwrap_err()["code"],
        "denied"
    );
}

#[test]
fn the_assistant_lists_its_sample_tool_through_the_workbench_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let info = invoke(&main, "plugin:paddock-workbench|assistant_info", json!({})).unwrap();
    assert!(text(&info).contains("list_files"), "{info}");
    let none = invoke(
        &main,
        "plugin:paddock-workbench|assistant_conversations",
        json!({}),
    )
    .unwrap();
    assert_eq!(none, json!([]));
}

#[test]
fn layouts_are_kept_in_the_config_folder() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    invoke(
        &main,
        "plugin:paddock-workbench|layout_set",
        json!({ "key": "wd", "value": { "version": 1 } }),
    )
    .unwrap();
    let back = invoke(
        &main,
        "plugin:paddock-workbench|layout_get",
        json!({ "key": "wd" }),
    )
    .unwrap();
    assert_eq!(back, json!({ "version": 1 }));
    assert!(
        std::fs::read_dir(dir.path()).unwrap().count() > 0,
        "the file lives under the test folder"
    );
}

#[test]
fn the_sample_reindex_starts_a_job_on_the_shells_registry() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path());
    let main = window(&app, "main");
    let id = invoke(&main, "demo_reindex", json!({})).unwrap();
    assert!(id.as_u64().is_some(), "{id}");
    let popout = window(&app, "p-main-1");
    assert_eq!(
        invoke(&popout, "demo_reindex", json!({})).unwrap_err()["code"],
        "denied"
    );
}

#[test]
fn every_command_of_the_demo_is_async_and_in_its_table() {
    assert_eq!(COMMANDS.specs().len(), 5);
    paddock_commands::testing::assert_async(&COMMANDS, &[&app_dir().join("src")]);
}

/// The demo's dialog base set as TypeScript, which its dialog windows hand to `dialogWindowConfig` (ADR 0026 §3).
#[test]
fn the_uis_dialog_contracts_are_generated_from_the_apps_table() {
    const REGENERATE: &str =
        "cd apps/workbench-demo/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app";
    let text = paddock_dialogs::ts::contracts_module("the app", &dialogs::DIALOGS, REGENERATE);
    let path = ui_dir().join("src/app/generated/dialog-contracts.ts");
    paddock_commands::testing::sync_generated(
        &path,
        &text,
        paddock_commands::testing::WriteMode::from_env(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
}

/// The demo's start as `run` composes it (rule 43), on the mock runtime: the main window opens, after every plugin is set
/// up (a window built inside a plugin hook deadlocks the start). The demo has no tray, so `--hidden` still opens it (there
/// would be no way back to the app).
#[test]
fn the_start_opens_the_main_window_even_when_hidden_without_a_tray() {
    use paddock_shell::testing::{Started, start};
    for hidden in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let place = Place::Test {
            dir: dir.path().to_path_buf(),
        };
        let launch = paddock_window::launch::LaunchArgs {
            hidden,
            ..Default::default()
        };
        let started = start(move || {
            let link = link(&place).unwrap();
            paddock_workbench_demo::app(
                mock_builder(),
                shell::<MockRuntime>(&place, link.clone()),
                workbench::<MockRuntime>(&place, link).unwrap(),
                launch,
            )
        });
        assert_eq!(
            started,
            Ok(Started {
                main_window: true,
                tray: false
            }),
            "hidden: {hidden}"
        );
    }
}
